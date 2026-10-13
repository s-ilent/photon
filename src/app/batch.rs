//! Batch conversion engine (Roadmap.md §5): file list × operation chain ×
//! target format → output folder.
//!
//! This is deliberately the GPU-free core: it decodes through
//! [`crate::image_loader::load_image`] (CPU), applies a chain of pure
//! operations, and encodes with the `image` crate. There is no UI, no
//! `PhotonApp`, and no wgpu here — the batch window and recipe import/export
//! land on top of these functions later.
//!
//! **[`run_batch`] fans the jobs out across CPU cores with `rayon`.**
//!
//! Scope note: this CPU engine covers the cheap chain steps (format, resize,
//! rotate/flip, grayscale/negative). Convolution/effect filters are intended to
//! move to a `wgpu` compute pass (`Roadmap.md` §5b); **this CPU path is the
//! retained fallback**, so a filter step may run here until that lands.

use std::io::{Seek, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ::image::codecs::bmp::BmpEncoder;
use ::image::codecs::gif::GifEncoder;
use ::image::codecs::jpeg::JpegEncoder;
use ::image::codecs::png::{CompressionType, PngEncoder};
use ::image::codecs::tga::TgaEncoder;
use ::image::codecs::tiff::TiffEncoder;
use ::image::codecs::webp::WebPEncoder;
use ::image::{ExtendedColorType, ImageEncoder};

use crate::app::image_op::{self, ImageOp};
use crate::image_loader::load_image;

/// Per-format encoder settings, threaded into [`OutputFormat::encode`] and the
/// conversion entry points. WebP output is lossless-only in `image` 0.25 and
/// `TiffEncoder` exposes no compression selector, so neither has a knob here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SaveOptions {
    /// JPEG quality, 1..=100.
    pub jpeg_quality: u8,
    /// PNG DEFLATE effort.
    pub png_compression: CompressionType,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 90,
            png_compression: CompressionType::Default,
        }
    }
}

/// Output format for a converted file, with the extension used for the target
/// path and the matching `image`-crate encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Tiff,
    Webp,
    Tga,
}

impl OutputFormat {
    pub(crate) fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Gif => "gif",
            OutputFormat::Bmp => "bmp",
            OutputFormat::Tiff => "tiff",
            OutputFormat::Webp => "webp",
            OutputFormat::Tga => "tga",
        }
    }

    /// Encode an RGBA8 buffer to `writer`, honouring the per-format knobs in
    /// `options`. GIF output is a single frame; an animated source only ever
    /// reaches here as its first frame because `load_image` decodes one frame.
    fn encode<W: Write + Seek>(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
        options: &SaveOptions,
        mut writer: W,
    ) -> Result<(), String> {
        let rgba_color = ExtendedColorType::Rgba8;
        let result = match self {
            OutputFormat::Png => PngEncoder::new_with_quality(
                writer,
                options.png_compression,
                ::image::codecs::png::FilterType::Adaptive,
            )
            .write_image(rgba, width, height, rgba_color),
            OutputFormat::Jpeg => {
                // JPEG has no alpha channel; drop it.
                let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
                for px in rgba.as_chunks::<4>().0 {
                    rgb.extend_from_slice(&px[..3]);
                }
                JpegEncoder::new_with_quality(writer, options.jpeg_quality).write_image(
                    &rgb,
                    width,
                    height,
                    ExtendedColorType::Rgb8,
                )
            }
            OutputFormat::Gif => {
                GifEncoder::new(writer).write_image(rgba, width, height, rgba_color)
            }
            OutputFormat::Bmp => {
                BmpEncoder::new(&mut writer).write_image(rgba, width, height, rgba_color)
            }
            OutputFormat::Tiff => {
                TiffEncoder::new(writer).write_image(rgba, width, height, rgba_color)
            }
            OutputFormat::Webp => {
                WebPEncoder::new_lossless(writer).write_image(rgba, width, height, rgba_color)
            }
            OutputFormat::Tga => {
                TgaEncoder::new(writer).write_image(rgba, width, height, rgba_color)
            }
        };
        result.map_err(|e| e.to_string())
    }
}

/// Outcome of one file's conversion. Mirrors the Roadmap Appendix A status
/// vocabulary (Success / Skipped / Failed) minus the interactive `Aborted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileStatus {
    Success,
    /// Intentionally not converted (e.g. target already exists under `Skip`).
    Skipped(String),
    /// The file could not be converted; the batch keeps going.
    Failed(String),
}

/// What to do when a target file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverwritePolicy {
    Overwrite,
    Skip,
    /// Resolved by the batch UI's dialog (yes / yes-to-all / no / cancel).
    /// Headlessly there is no one to ask, so a pre-existing target is treated
    /// as [`FileStatus::Skipped`].
    Ask,
}

/// One unit of work: convert `source` into `target_dir` as `format`, after
/// applying `operations`. Encoder settings come from [`SaveOptions`].
#[derive(Debug, Clone)]
pub(crate) struct ConversionJob {
    pub source: PathBuf,
    pub target_dir: PathBuf,
    pub format: OutputFormat,
    pub operations: Vec<ImageOp>,
}

/// Convert a single file. Never panics on I/O or decode failure — every failure
/// comes back as [`FileStatus::Failed`] (or [`FileStatus::Skipped`]) so one bad
/// file cannot abort a batch.
pub(crate) fn convert_file(
    job: &ConversionJob,
    policy: OverwritePolicy,
    options: &SaveOptions,
) -> FileStatus {
    let stem = match job.source.file_stem() {
        Some(stem) => stem.to_os_string(),
        None => {
            return FileStatus::Failed(format!("source has no file name: {}", job.source.display()))
        }
    };

    // Build the target name by appending the extension rather than using
    // `with_extension`, which would drop a dot in the stem (e.g. `a.v2`).
    let mut file_name = stem;
    file_name.push(".");
    file_name.push(job.format.extension());
    let target = job.target_dir.join(file_name);

    // Never write over an input, however the overwrite policy is set: the
    // alongside-input default collides when the format is unchanged.
    if target == job.source {
        return FileStatus::Skipped("output equals input".to_string());
    }

    if target.exists() {
        match policy {
            OverwritePolicy::Overwrite => {}
            // `Ask` cannot be answered headlessly; treat it as `Skip`.
            OverwritePolicy::Skip | OverwritePolicy::Ask => {
                return FileStatus::Skipped("exists".to_string());
            }
        }
    }

    let decoded = match load_image(&job.source) {
        Ok(decoded) => decoded,
        Err(e) => return FileStatus::Failed(e.to_string()),
    };

    let Some(layer) = decoded.layers.into_iter().next() else {
        return FileStatus::Failed("decoded image has no layers".to_string());
    };
    let (rgba, width, height) =
        image_op::apply(layer, decoded.width, decoded.height, &job.operations);

    if let Err(e) = std::fs::create_dir_all(&job.target_dir) {
        return FileStatus::Failed(format!(
            "failed to create {}: {}",
            job.target_dir.display(),
            e
        ));
    }

    let file = match std::fs::File::create(&target) {
        Ok(file) => file,
        Err(e) => {
            return FileStatus::Failed(format!("failed to create {}: {}", target.display(), e))
        }
    };
    let mut writer = std::io::BufWriter::new(file);
    if let Err(e) = job
        .format
        .encode(&rgba, width, height, options, &mut writer)
    {
        return FileStatus::Failed(e);
    }
    // `BufWriter` swallows write errors on drop, so flush explicitly to surface
    // them as a failed status.
    if let Err(e) = writer.flush() {
        return FileStatus::Failed(format!("failed to write {}: {}", target.display(), e));
    }

    FileStatus::Success
}

/// Convert every job, in parallel, returning one status per job in the same
/// order as `jobs`. Failures are per-file and never abort the batch.
///
/// `rayon`'s work-stealing `par_iter` bounds (and reuses) the thread pool;
/// `collect` preserves input order, so callers can zip statuses to jobs.
///
/// The Batch Convert window drives [`run_batch_with_progress`] directly so it
/// can show a progress bar; this headless entry stays for tests and callers
/// with no UI.
#[allow(dead_code)]
pub(crate) fn run_batch(
    jobs: &[ConversionJob],
    policy: OverwritePolicy,
    options: &SaveOptions,
) -> Vec<FileStatus> {
    run_batch_with_progress(jobs, policy, options, |_, _| {})
}

/// As [`run_batch`], but calls `on_progress(done, total)` from the worker
/// threads as each job completes. `done` counts completions across all
/// threads (not per-thread), so progress is determinate and monotonic.
pub(crate) fn run_batch_with_progress<F: Fn(usize, usize) + Sync>(
    jobs: &[ConversionJob],
    policy: OverwritePolicy,
    options: &SaveOptions,
    on_progress: F,
) -> Vec<FileStatus> {
    use rayon::prelude::*;
    let total = jobs.len();
    let done = AtomicUsize::new(0);
    jobs.par_iter()
        .map(|job| {
            let status = convert_file(job, policy, options);
            let completed = done.fetch_add(1, Ordering::Relaxed) + 1;
            on_progress(completed, total);
            status
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::{Rgba, RgbaImage};

    #[test]
    fn output_format_extensions() {
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Jpeg.extension(), "jpg");
        assert_eq!(OutputFormat::Gif.extension(), "gif");
        assert_eq!(OutputFormat::Bmp.extension(), "bmp");
        assert_eq!(OutputFormat::Tiff.extension(), "tiff");
        assert_eq!(OutputFormat::Webp.extension(), "webp");
        assert_eq!(OutputFormat::Tga.extension(), "tga");
    }

    #[test]
    fn convert_never_writes_over_its_own_input() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("in.png");
        RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]))
            .save(&source)
            .expect("save source");

        // Same folder, same format: the target IS the input.
        let job = ConversionJob {
            source: source.clone(),
            target_dir: dir.path().to_path_buf(),
            format: OutputFormat::Png,
            operations: Vec::new(),
        };

        assert_eq!(
            convert_file(&job, OverwritePolicy::Overwrite, &SaveOptions::default()),
            FileStatus::Skipped("output equals input".to_string()),
            "even Overwrite must not touch the input"
        );
        let before = std::fs::read(&source).expect("read source");
        assert_eq!(
            std::fs::metadata(&source).expect("source still there").len() as usize,
            before.len()
        );
    }

    #[test]
    fn overwrite_skip_leaves_existing_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("in.png");
        RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]))
            .save(&source)
            .expect("save source");

        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).expect("create out dir");
        let existing = out_dir.join("in.jpg");
        std::fs::write(&existing, b"already here").expect("seed target");

        let job = ConversionJob {
            source,
            target_dir: out_dir,
            format: OutputFormat::Jpeg,
            operations: Vec::new(),
        };

        assert_eq!(
            convert_file(&job, OverwritePolicy::Skip, &SaveOptions::default()),
            FileStatus::Skipped("exists".to_string())
        );
        assert_eq!(
            std::fs::read(&existing).expect("read target"),
            b"already here",
            "skip must not modify the existing target"
        );
        assert_eq!(
            convert_file(&job, OverwritePolicy::Ask, &SaveOptions::default()),
            FileStatus::Skipped("exists".to_string())
        );
    }

    #[test]
    fn convert_file_png_to_jpeg_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("photo.png");
        RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]))
            .save(&source)
            .expect("save source");

        let out_dir = dir.path().join("converted");
        let job = ConversionJob {
            source,
            target_dir: out_dir.clone(),
            format: OutputFormat::Jpeg,
            operations: Vec::new(),
        };

        let options = SaveOptions {
            jpeg_quality: 85,
            ..Default::default()
        };
        assert_eq!(
            convert_file(&job, OverwritePolicy::Overwrite, &options),
            FileStatus::Success
        );

        let target = out_dir.join("photo.jpg");
        assert!(
            target.exists(),
            "target {} was not written",
            target.display()
        );
        let decoded = ::image::open(&target).expect("decode jpeg result");
        assert_eq!((decoded.width(), decoded.height()), (4, 3));
    }

    #[test]
    fn run_batch_reports_one_status_per_job() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out_dir = dir.path().join("out");

        let sources: Vec<PathBuf> = (0..4)
            .map(|i| {
                let path = dir.path().join(format!("img{i}.png"));
                RgbaImage::from_pixel(2, 2, Rgba([i as u8, 0, 0, 255]))
                    .save(&path)
                    .expect("save source");
                path
            })
            .collect();
        // Last job points at a missing file: it must fail without aborting.
        let missing = dir.path().join("missing.png");

        let mut jobs: Vec<ConversionJob> = sources
            .into_iter()
            .map(|source| ConversionJob {
                source,
                target_dir: out_dir.clone(),
                format: OutputFormat::Png,
                operations: Vec::new(),
            })
            .collect();
        jobs.push(ConversionJob {
            source: missing,
            target_dir: out_dir.clone(),
            format: OutputFormat::Png,
            operations: Vec::new(),
        });

        let statuses = run_batch(&jobs, OverwritePolicy::Overwrite, &SaveOptions::default());
        assert_eq!(statuses.len(), jobs.len());
        assert!(matches!(statuses[0], FileStatus::Success));
        assert!(matches!(statuses[1], FileStatus::Success));
        assert!(matches!(statuses[2], FileStatus::Success));
        assert!(matches!(statuses[3], FileStatus::Success));
        assert!(matches!(statuses[4], FileStatus::Failed(_)));
    }

    #[test]
    fn save_options_default_matches_the_ui_defaults() {
        let options = SaveOptions::default();
        assert_eq!(options.jpeg_quality, 90);
        assert_eq!(options.png_compression, CompressionType::Default);
    }

    #[test]
    fn run_batch_with_progress_counts_every_job() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out_dir = dir.path().join("out");
        let jobs: Vec<ConversionJob> = (0..3)
            .map(|i| {
                let source = dir.path().join(format!("p{i}.png"));
                RgbaImage::from_pixel(2, 2, Rgba([i as u8, 0, 0, 255]))
                    .save(&source)
                    .expect("save source");
                ConversionJob {
                    source,
                    target_dir: out_dir.clone(),
                    format: OutputFormat::Png,
                    operations: Vec::new(),
                }
            })
            .collect();

        let max_done = AtomicUsize::new(0);
        let seen_total = AtomicUsize::new(0);
        let statuses = run_batch_with_progress(
            &jobs,
            OverwritePolicy::Overwrite,
            &SaveOptions::default(),
            |done, total| {
                max_done.fetch_max(done, Ordering::Relaxed);
                seen_total.store(total, Ordering::Relaxed);
            },
        );

        assert_eq!(statuses.len(), jobs.len());
        assert!(statuses.iter().all(|s| matches!(s, FileStatus::Success)));
        assert_eq!(max_done.load(Ordering::Relaxed), 3);
        assert_eq!(seen_total.load(Ordering::Relaxed), 3);
    }
}
