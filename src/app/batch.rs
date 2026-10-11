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
//! rotate/flip, grayscale/negative). **Image filters are compute-shader work**
//! (Roadmap §5b) — the convolution kernels are implemented as a `wgpu` compute
//! pass and are deliberately NOT a CPU routine here.

use std::io::{Seek, Write};
use std::path::PathBuf;

use ::image::codecs::bmp::BmpEncoder;
use ::image::codecs::gif::GifEncoder;
use ::image::codecs::jpeg::JpegEncoder;
use ::image::codecs::png::PngEncoder;
use ::image::codecs::tga::TgaEncoder;
use ::image::codecs::tiff::TiffEncoder;
use ::image::codecs::webp::WebPEncoder;
use ::image::imageops::{self, FilterType};
use ::image::{ExtendedColorType, ImageEncoder, RgbaImage};

use crate::image_loader::load_image;

/// A single step in a batch operation chain. Applied left-to-right, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Operation {
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
    /// Integer luma conversion (Appendix A): `gray = (77·R + 150·G + 29·B) >> 8`.
    Grayscale,
    /// Inverts R, G and B, preserving alpha.
    Negative,
    /// Resize to an explicit size or, when only one axis is given, preserving
    /// the source aspect ratio. `None` for both axes is a no-op.
    Resize {
        width: Option<u32>,
        height: Option<u32>,
        filter: ResampleFilter,
    },
}

/// Resample kernels offered for [`Operation::Resize`], mapped to the `image`
/// crate's [`FilterType`] (Roadmap Appendix A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResampleFilter {
    Nearest,
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

impl ResampleFilter {
    fn to_filter_type(self) -> FilterType {
        match self {
            ResampleFilter::Nearest => FilterType::Nearest,
            ResampleFilter::Triangle => FilterType::Triangle,
            ResampleFilter::CatmullRom => FilterType::CatmullRom,
            ResampleFilter::Gaussian => FilterType::Gaussian,
            ResampleFilter::Lanczos3 => FilterType::Lanczos3,
        }
    }
}

/// Apply `ops` in order to an RGBA8 buffer, returning the resulting pixels and
/// dimensions. Pure: no I/O, no GPU.
pub(crate) fn apply_operations(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    ops: &[Operation],
) -> (Vec<u8>, u32, u32) {
    // `load_image` always produces exactly `width * height * 4` bytes, so this
    // is a defensive guard rather than an expected path.
    if rgba.len() != width as usize * height as usize * 4 {
        return (rgba, width, height);
    }
    let mut img = RgbaImage::from_raw(width, height, rgba)
        .expect("RGBA buffer length was validated against the dimensions");

    for op in ops {
        match op {
            Operation::Rotate90 => img = imageops::rotate90(&img),
            Operation::Rotate180 => img = imageops::rotate180(&img),
            Operation::Rotate270 => img = imageops::rotate270(&img),
            Operation::FlipHorizontal => img = imageops::flip_horizontal(&img),
            Operation::FlipVertical => img = imageops::flip_vertical(&img),
            Operation::Grayscale => {
                for px in img.pixels_mut() {
                    let [r, g, b, a] = px.0;
                    let gray = ((77u32 * r as u32 + 150u32 * g as u32 + 29u32 * b as u32) >> 8)
                        as u8;
                    px.0 = [gray, gray, gray, a];
                }
            }
            Operation::Negative => {
                for px in img.pixels_mut() {
                    let [r, g, b, a] = px.0;
                    px.0 = [255 - r, 255 - g, 255 - b, a];
                }
            }
            Operation::Resize {
                width: w,
                height: h,
                filter,
            } => {
                if let Some((tw, th)) = target_dimensions(img.width(), img.height(), *w, *h) {
                    img = imageops::resize(&img, tw, th, filter.to_filter_type());
                }
            }
        }
    }

    let (w, h) = (img.width(), img.height());
    (img.into_raw(), w, h)
}

/// Resolve a possibly-partial resize request into concrete target dimensions.
/// A single given axis scales the other proportionally; both `None` yields
/// `None` (nothing to do). Dims are clamped to at least 1.
fn target_dimensions(
    src_w: u32,
    src_h: u32,
    width: Option<u32>,
    height: Option<u32>,
) -> Option<(u32, u32)> {
    let src_w = src_w.max(1);
    let src_h = src_h.max(1);
    match (width, height) {
        (Some(w), Some(h)) => Some((w.max(1), h.max(1))),
        (Some(w), None) => {
            let scaled = (src_h as f64 * (w as f64 / src_w as f64)).round() as u32;
            Some((w.max(1), scaled.max(1)))
        }
        (None, Some(h)) => {
            let scaled = (src_w as f64 * (h as f64 / src_h as f64)).round() as u32;
            Some((scaled.max(1), h.max(1)))
        }
        (None, None) => None,
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

    /// Encode an RGBA8 buffer to `writer`. `jpeg_quality` is only consulted for
    /// [`OutputFormat::Jpeg`]. GIF output is a single frame; an animated source
    /// only ever reaches here as its first frame because `load_image` decodes
    /// one frame.
    fn encode<W: Write + Seek>(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
        jpeg_quality: u8,
        mut writer: W,
    ) -> Result<(), String> {
        let rgba_color = ExtendedColorType::Rgba8;
        let result = match self {
            OutputFormat::Png => {
                PngEncoder::new(writer).write_image(rgba, width, height, rgba_color)
            }
            OutputFormat::Jpeg => {
                // JPEG has no alpha channel; drop it.
                let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
                for px in rgba.as_chunks::<4>().0 {
                    rgb.extend_from_slice(&px[..3]);
                }
                JpegEncoder::new_with_quality(writer, jpeg_quality).write_image(
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
/// applying `operations`.
#[derive(Debug, Clone)]
pub(crate) struct ConversionJob {
    pub source: PathBuf,
    pub target_dir: PathBuf,
    pub format: OutputFormat,
    pub operations: Vec<Operation>,
    pub jpeg_quality: u8,
}

/// Convert a single file. Never panics on I/O or decode failure — every failure
/// comes back as [`FileStatus::Failed`] (or [`FileStatus::Skipped`]) so one bad
/// file cannot abort a batch.
pub(crate) fn convert_file(job: &ConversionJob, policy: OverwritePolicy) -> FileStatus {
    let stem = match job.source.file_stem() {
        Some(stem) => stem.to_os_string(),
        None => {
            return FileStatus::Failed(format!(
                "source has no file name: {}",
                job.source.display()
            ))
        }
    };

    // Build the target name by appending the extension rather than using
    // `with_extension`, which would drop a dot in the stem (e.g. `a.v2`).
    let mut file_name = stem;
    file_name.push(".");
    file_name.push(job.format.extension());
    let target = job.target_dir.join(file_name);

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
        apply_operations(layer, decoded.width, decoded.height, &job.operations);

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
        .encode(&rgba, width, height, job.jpeg_quality, &mut writer)
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
pub(crate) fn run_batch(jobs: &[ConversionJob], policy: OverwritePolicy) -> Vec<FileStatus> {
    use rayon::prelude::*;
    jobs.par_iter()
        .map(|job| convert_file(job, policy))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::{Rgba, RgbaImage};

    /// Build a row-major RGBA buffer from grayscale values (alpha = 255).
    fn gray_pixels(values: &[u8]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|&v| [v, v, v, 255u8])
            .collect()
    }

    /// The source pixel sequence, as gray levels, from an RGBA buffer.
    fn gray_levels(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
    }

    #[test]
    fn rotate90_swaps_dimensions_and_moves_pixels() {
        // 3x1 row [1, 2, 3] becomes a 1x3 column.
        let (out, w, h) = apply_operations(gray_pixels(&[1, 2, 3]), 3, 1, &[Operation::Rotate90]);
        assert_eq!((w, h), (1, 3));
        let mut levels = gray_levels(&out);
        levels.sort_unstable();
        assert_eq!(levels, vec![1, 2, 3]);
    }

    #[test]
    fn rotate180_reverses_pixels() {
        let (out, w, h) =
            apply_operations(gray_pixels(&[1, 2, 3, 4]), 2, 2, &[Operation::Rotate180]);
        assert_eq!((w, h), (2, 2));
        assert_eq!(gray_levels(&out), vec![4, 3, 2, 1]);
    }

    #[test]
    fn rotate270_swaps_dimensions_and_preserves_pixels() {
        let (out, w, h) = apply_operations(gray_pixels(&[1, 2, 3]), 3, 1, &[Operation::Rotate270]);
        assert_eq!((w, h), (1, 3));
        let mut levels = gray_levels(&out);
        levels.sort_unstable();
        assert_eq!(levels, vec![1, 2, 3]);
    }

    #[test]
    fn flip_horizontal_reverses_row() {
        let (out, w, h) =
            apply_operations(gray_pixels(&[1, 2, 3]), 3, 1, &[Operation::FlipHorizontal]);
        assert_eq!((w, h), (3, 1));
        assert_eq!(gray_levels(&out), vec![3, 2, 1]);
    }

    #[test]
    fn flip_vertical_reverses_rows() {
        // 2x2 [[1, 2], [3, 4]] -> [[3, 4], [1, 2]].
        let (out, w, h) =
            apply_operations(gray_pixels(&[1, 2, 3, 4]), 2, 2, &[Operation::FlipVertical]);
        assert_eq!((w, h), (2, 2));
        assert_eq!(gray_levels(&out), vec![3, 4, 1, 2]);
    }

    #[test]
    fn grayscale_uses_documented_luma_weights() {
        // (77*100 + 150*150 + 29*200) >> 8 = 36000 >> 8 = 140.
        let rgba = vec![100, 150, 200, 255];
        let (out, w, h) = apply_operations(rgba, 1, 1, &[Operation::Grayscale]);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![140, 140, 140, 255]);
    }

    #[test]
    fn negative_inverts_rgb_but_preserves_alpha() {
        let rgba = vec![10, 20, 30, 40];
        let (out, w, h) = apply_operations(rgba, 1, 1, &[Operation::Negative]);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![245, 235, 225, 40]);
    }

    #[test]
    fn resize_changes_dimensions() {
        let src = gray_pixels(&[128; 16]); // 4x4
        let (out, w, h) = apply_operations(
            src,
            4,
            4,
            &[Operation::Resize {
                width: Some(2),
                height: Some(2),
                filter: ResampleFilter::Nearest,
            }],
        );
        assert_eq!((w, h), (2, 2));
        assert_eq!(out.len(), 2 * 2 * 4);
    }

    #[test]
    fn resize_single_axis_preserves_aspect() {
        let src = gray_pixels(&[0; 8]); // 4x2
        let (out, w, h) = apply_operations(
            src,
            4,
            2,
            &[Operation::Resize {
                width: Some(2),
                height: None,
                filter: ResampleFilter::Lanczos3,
            }],
        );
        assert_eq!((w, h), (2, 1));
        assert_eq!(out.len(), 8); // 2x1 RGBA
    }

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
            jpeg_quality: 90,
        };

        assert_eq!(
            convert_file(&job, OverwritePolicy::Skip),
            FileStatus::Skipped("exists".to_string())
        );
        assert_eq!(
            std::fs::read(&existing).expect("read target"),
            b"already here",
            "skip must not modify the existing target"
        );
        assert_eq!(
            convert_file(&job, OverwritePolicy::Ask),
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
            jpeg_quality: 85,
        };

        assert_eq!(convert_file(&job, OverwritePolicy::Overwrite), FileStatus::Success);

        let target = out_dir.join("photo.jpg");
        assert!(target.exists(), "target {} was not written", target.display());
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
                jpeg_quality: 90,
            })
            .collect();
        jobs.push(ConversionJob {
            source: missing,
            target_dir: out_dir.clone(),
            format: OutputFormat::Png,
            operations: Vec::new(),
            jpeg_quality: 90,
        });

        let statuses = run_batch(&jobs, OverwritePolicy::Overwrite);
        assert_eq!(statuses.len(), jobs.len());
        assert!(matches!(statuses[0], FileStatus::Success));
        assert!(matches!(statuses[1], FileStatus::Success));
        assert!(matches!(statuses[2], FileStatus::Success));
        assert!(matches!(statuses[3], FileStatus::Success));
        assert!(matches!(statuses[4], FileStatus::Failed(_)));
    }
}
