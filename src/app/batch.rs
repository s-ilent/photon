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
use ::image::imageops::{self, FilterType};
use ::image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};

use crate::image_loader::load_image;

/// Convolution filters (Roadmap Appendix A), applied by [`convolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FilterKind {
    Blur,
    Soften,
    Sharpen,
    Gaussian3,
    Gaussian5,
    Emboss,
    Edge,
}

impl FilterKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            FilterKind::Blur => "Blur",
            FilterKind::Soften => "Soften",
            FilterKind::Sharpen => "Sharpen",
            FilterKind::Gaussian3 => "Gaussian 3×3",
            FilterKind::Gaussian5 => "Gaussian 5×5",
            FilterKind::Emboss => "Emboss",
            FilterKind::Edge => "Edge",
        }
    }

    /// Row-major kernel, side length, divisor and bias (Roadmap Appendix A).
    /// Emboss and Edge sum to zero, so their divisor is a no-op placeholder and
    /// only the bias shifts the result; Appendix A does not state the Edge bias
    /// (marked unconfirmed), so +127 is used.
    fn parameters(self) -> (&'static [f32], usize, f32, f32) {
        match self {
            FilterKind::Blur => (&[1.0; 9], 3, 9.0, 0.0),
            FilterKind::Soften => (&[1.0, 1.0, 1.0, 1.0, 8.0, 1.0, 1.0, 1.0, 1.0], 3, 16.0, 0.0),
            FilterKind::Sharpen => (
                &[-1.0, -1.0, -1.0, -1.0, 15.0, -1.0, -1.0, -1.0, -1.0],
                3,
                7.0,
                0.0,
            ),
            FilterKind::Gaussian3 => (&[1.0, 2.0, 1.0, 2.0, 4.0, 2.0, 1.0, 2.0, 1.0], 3, 16.0, 0.0),
            FilterKind::Gaussian5 => (
                &[
                    0.0, 1.0, 2.0, 1.0, 0.0, 1.0, 3.0, 4.0, 3.0, 1.0, 2.0, 4.0, 8.0, 4.0, 2.0, 1.0,
                    3.0, 4.0, 3.0, 1.0, 0.0, 1.0, 2.0, 1.0, 0.0,
                ],
                5,
                52.0,
                0.0,
            ),
            FilterKind::Emboss => (
                &[0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                3,
                1.0,
                127.0,
            ),
            FilterKind::Edge => (
                &[-1.0, -1.0, -1.0, -1.0, 8.0, -1.0, -1.0, -1.0, -1.0],
                3,
                1.0,
                127.0,
            ),
        }
    }
}

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
    /// Convolution filter (Appendix A), applied via [`convolve`].
    Filter(FilterKind),
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
            Operation::Filter(kind) => {
                let (kernel, k, divisor, bias) = kind.parameters();
                img = convolve(&img, kernel, k, divisor, bias);
            }
        }
    }

    let (w, h) = (img.width(), img.height());
    (img.into_raw(), w, h)
}

/// Apply a `k`×`k` row-major `kernel` to `src`. Sample coordinates are clamped
/// into the image bounds (edge pixels repeat outward; nothing wraps). Each RGB
/// channel becomes `clamp(sum(weight·sample) / divisor + bias, 0, 255)`; the
/// source alpha is copied through unchanged. `divisor` 0 is treated as 1.
pub(crate) fn convolve(
    src: &RgbaImage,
    kernel: &[f32],
    k: usize,
    divisor: f32,
    bias: f32,
) -> RgbaImage {
    debug_assert_eq!(kernel.len(), k * k, "kernel must be k×k");
    let (w, h) = (src.width(), src.height());
    let mut out = RgbaImage::new(w, h);
    if w == 0 || h == 0 {
        return out;
    }
    let divisor = if divisor == 0.0 { 1.0 } else { divisor };
    let half = (k / 2) as i64;
    let (wi, hi) = (w as i64, h as i64);
    for y in 0..hi {
        for x in 0..wi {
            let mut acc = [0.0f32; 3];
            for ky in 0..k as i64 {
                for kx in 0..k as i64 {
                    let sx = (x + kx - half).clamp(0, wi - 1) as u32;
                    let sy = (y + ky - half).clamp(0, hi - 1) as u32;
                    let px = src.get_pixel(sx, sy).0;
                    let weight = kernel[(ky * k as i64 + kx) as usize];
                    acc[0] += px[0] as f32 * weight;
                    acc[1] += px[1] as f32 * weight;
                    acc[2] += px[2] as f32 * weight;
                }
            }
            let alpha = src.get_pixel(x as u32, y as u32).0[3];
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([
                    ((acc[0] / divisor) + bias).clamp(0.0, 255.0).round() as u8,
                    ((acc[1] / divisor) + bias).clamp(0.0, 255.0).round() as u8,
                    ((acc[2] / divisor) + bias).clamp(0.0, 255.0).round() as u8,
                    alpha,
                ]),
            );
        }
    }
    out
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
    pub operations: Vec<Operation>,
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
    fn convolve_blur_keeps_a_uniform_field_uniform() {
        let src = RgbaImage::from_pixel(4, 3, Rgba([100, 100, 100, 255]));
        let out = convolve(&src, &[1.0; 9], 3, 9.0, 0.0);
        assert_eq!(out.dimensions(), (4, 3));
        assert!(out.pixels().all(|px| px.0 == [100, 100, 100, 255]));
    }

    #[test]
    fn convolve_blur_spreads_an_isolated_bright_pixel() {
        let mut src = RgbaImage::from_pixel(5, 5, Rgba([0, 0, 0, 255]));
        src.put_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let out = convolve(&src, &[1.0; 9], 3, 9.0, 0.0);
        // 255 / 9 rounds to 28; only the 3×3 block around the centre gains it.
        assert_eq!(out.get_pixel(2, 2).0[0], 28);
        assert_eq!(out.get_pixel(1, 1).0[0], 28);
        assert_eq!(out.get_pixel(3, 3).0[0], 28);
        assert_eq!(out.get_pixel(0, 0).0[0], 0);
        assert_eq!(out.get_pixel(4, 4).0[0], 0);
    }

    #[test]
    fn convolve_identity_kernel_round_trips() {
        let mut src = RgbaImage::new(2, 1);
        src.put_pixel(0, 0, Rgba([1, 2, 3, 4]));
        src.put_pixel(1, 0, Rgba([250, 128, 0, 9]));
        let out = convolve(&src, &[1.0], 1, 1.0, 0.0);
        assert_eq!(out.get_pixel(0, 0).0, [1, 2, 3, 4]);
        assert_eq!(out.get_pixel(1, 0).0, [250, 128, 0, 9]);
    }

    #[test]
    fn convolve_preserves_source_alpha() {
        let mut src = RgbaImage::new(3, 1);
        src.put_pixel(0, 0, Rgba([10, 10, 10, 11]));
        src.put_pixel(1, 0, Rgba([255, 255, 255, 22]));
        src.put_pixel(2, 0, Rgba([10, 10, 10, 33]));
        let out = convolve(&src, &[1.0; 9], 3, 9.0, 0.0);
        assert_eq!(out.get_pixel(0, 0).0[3], 11);
        assert_eq!(out.get_pixel(1, 0).0[3], 22);
        assert_eq!(out.get_pixel(2, 0).0[3], 33);
    }

    #[test]
    fn filter_operation_applies_through_apply_operations() {
        let src = gray_pixels(&[100; 9]); // uniform 3x3
        let (out, w, h) = apply_operations(src, 3, 3, &[Operation::Filter(FilterKind::Blur)]);
        assert_eq!((w, h), (3, 3));
        assert!(gray_levels(&out).iter().all(|&v| v == 100));
    }

    #[test]
    fn filter_edge_applies_bias_to_a_flat_field() {
        // Edge sums to zero, so a flat field becomes the +127 bias.
        let (out, ..) = apply_operations(
            gray_pixels(&[0; 4]),
            2,
            2,
            &[Operation::Filter(FilterKind::Edge)],
        );
        assert!(gray_levels(&out).iter().all(|&v| v == 127));
    }

    #[test]
    fn filter_labels_are_distinct() {
        let kinds = [
            FilterKind::Blur,
            FilterKind::Soften,
            FilterKind::Sharpen,
            FilterKind::Gaussian3,
            FilterKind::Gaussian5,
            FilterKind::Emboss,
            FilterKind::Edge,
        ];
        let mut labels: Vec<&str> = kinds.iter().map(|k| k.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), kinds.len());
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
