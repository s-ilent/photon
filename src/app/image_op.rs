//! The shared image-operation vocabulary (Roadmap.md §1): one enum for the
//! transforms the interactive editor and the batch chain both apply, plus the
//! Appendix-B text form that batch recipes and the CLI serialise to.
//!
//! One implementation per operation: rotate/flip/grayscale/negative/crop
//! delegate to [`crate::app::edit::Rgba`]'s tested methods; resize and
//! convolution use the `image` crate here. The batch engine (Roadmap §5) drives
//! [`apply`]; the interactive editor applies single ops.

use ::image::imageops::{self, FilterType};
use ::image::{Rgba as Pixel, RgbaImage};

use crate::app::edit::{CropRect, Rgba};

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
    pub(crate) fn parameters(self) -> (&'static [f32], usize, f32, f32) {
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

    /// The `/filter:<name>` token (Appendix B), lowercase.
    fn cli_name(self) -> &'static str {
        match self {
            FilterKind::Blur => "blur",
            FilterKind::Soften => "soften",
            FilterKind::Sharpen => "sharpen",
            FilterKind::Gaussian3 => "gaussian3",
            FilterKind::Gaussian5 => "gaussian5",
            FilterKind::Emboss => "emboss",
            FilterKind::Edge => "edge",
        }
    }

    fn from_cli_name(name: &str) -> Option<Self> {
        Some(match name {
            "blur" => FilterKind::Blur,
            "soften" => FilterKind::Soften,
            "sharpen" => FilterKind::Sharpen,
            "gaussian3" => FilterKind::Gaussian3,
            "gaussian5" => FilterKind::Gaussian5,
            "emboss" => FilterKind::Emboss,
            "edge" => FilterKind::Edge,
            _ => return None,
        })
    }
}

/// Resample kernels offered for [`ImageOp::Resize`], mapped to the `image`
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

    /// The optional third `/resize` component. Appendix B has no filter slot;
    /// this is a documented superset so the text form round-trips losslessly.
    fn cli_name(self) -> &'static str {
        match self {
            ResampleFilter::Nearest => "nearest",
            ResampleFilter::Triangle => "triangle",
            ResampleFilter::CatmullRom => "catmullrom",
            ResampleFilter::Gaussian => "gaussian",
            ResampleFilter::Lanczos3 => "lanczos3",
        }
    }

    fn from_cli_name(name: &str) -> Option<Self> {
        Some(match name {
            "nearest" => ResampleFilter::Nearest,
            "triangle" => ResampleFilter::Triangle,
            "catmullrom" => ResampleFilter::CatmullRom,
            "gaussian" => ResampleFilter::Gaussian,
            "lanczos3" => ResampleFilter::Lanczos3,
            _ => return None,
        })
    }
}

/// One image operation, applied left-to-right in a chain. Shared by the batch
/// engine and the interactive editor; `RotateLeft`/`RotateRight` are directional
/// (90° counter-clockwise / clockwise) to match the registry commands and
/// Appendix B's `/rotate:Left|Right|180`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageOp {
    RotateLeft,
    RotateRight,
    Rotate180,
    FlipHorizontal,
    FlipVertical,
    /// Integer luma conversion (Appendix A): `gray = (77·R + 150·G + 29·B) >> 8`.
    Grayscale,
    /// Inverts R, G and B, preserving alpha.
    Negative,
    /// Resize to an explicit size or, when only one axis is given (a `0` in the
    /// text form), preserving the source aspect ratio. `None` for both axes is
    /// a no-op.
    Resize {
        width: Option<u32>,
        height: Option<u32>,
        filter: ResampleFilter,
    },
    /// Convolution filter (Appendix A), applied via [`convolve`].
    Filter(FilterKind),
    /// Trim `left`/`top`/`right`/`bottom` pixel margins (`/crop:(L,T,R,B)`),
    /// clamped to the image; an empty result is a no-op.
    Crop {
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
    },
}

impl ImageOp {
    /// The Appendix-B switch for this operation. Chain them with
    /// [`recipe_text`].
    pub(crate) fn to_cli(&self) -> String {
        match self {
            ImageOp::RotateLeft => "/rotate:left".to_string(),
            ImageOp::RotateRight => "/rotate:right".to_string(),
            ImageOp::Rotate180 => "/rotate:180".to_string(),
            ImageOp::FlipHorizontal => "/mirror".to_string(),
            ImageOp::FlipVertical => "/flip".to_string(),
            ImageOp::Grayscale => "/grayscale".to_string(),
            ImageOp::Negative => "/negative".to_string(),
            ImageOp::Resize { width, height, filter } => format!(
                "/resize:({},{},{})",
                width.map(|v| v.to_string()).unwrap_or_else(|| "0".to_string()),
                height.map(|v| v.to_string()).unwrap_or_else(|| "0".to_string()),
                filter.cli_name()
            ),
            ImageOp::Filter(kind) => format!("/filter:{}", kind.cli_name()),
            ImageOp::Crop {
                left,
                top,
                right,
                bottom,
            } => format!("/crop:({left},{top},{right},{bottom})"),
        }
    }

    /// Parse one Appendix-B switch (case-insensitive). Free rotation
    /// (`/rotate:<degrees>`) and percent resize (`%`) are not supported and
    /// return an error rather than being ignored.
    pub(crate) fn from_cli(token: &str) -> Result<ImageOp, String> {
        let rest = token
            .strip_prefix('/')
            .ok_or_else(|| format!("{token:?} is not a switch (expected a leading '/')"))?;
        let (name, arg) = match rest.split_once(':') {
            Some((name, arg)) => (name, Some(arg)),
            None => (rest, None),
        };
        let unexpected = |what: &str| format!("/{name} takes no argument (got {what:?})");
        match name.to_ascii_lowercase().as_str() {
            "rotate" => {
                let arg = arg.ok_or_else(|| "/rotate needs left, right or 180".to_string())?;
                match arg.to_ascii_lowercase().as_str() {
                    "left" => Ok(ImageOp::RotateLeft),
                    "right" => Ok(ImageOp::RotateRight),
                    "180" => Ok(ImageOp::Rotate180),
                    other => Err(format!(
                        "unsupported rotation {other:?} (free rotate is not supported; \
                         use left, right or 180)"
                    )),
                }
            }
            // Appendix B lists `/flip` and `/mirror` without axes; Photon's own
            // labelling treats mirroring as the horizontal flip.
            "flip" => arg.map_or(Ok(ImageOp::FlipVertical), |a| Err(unexpected(a))),
            "mirror" => arg.map_or(Ok(ImageOp::FlipHorizontal), |a| Err(unexpected(a))),
            "grayscale" => arg.map_or(Ok(ImageOp::Grayscale), |a| Err(unexpected(a))),
            "negative" => arg.map_or(Ok(ImageOp::Negative), |a| Err(unexpected(a))),
            "resize" => {
                let arg = arg.ok_or_else(|| "/resize needs (W,H)".to_string())?;
                parse_resize(arg)
            }
            "filter" => {
                let arg = arg.ok_or_else(|| "/filter needs a filter name".to_string())?;
                FilterKind::from_cli_name(&arg.to_ascii_lowercase())
                    .map(ImageOp::Filter)
                    .ok_or_else(|| format!("unknown filter {arg:?}"))
            }
            "crop" => {
                let arg = arg.ok_or_else(|| "/crop needs (L,T,R,B)".to_string())?;
                parse_crop(arg)
            }
            other => Err(format!("unknown switch /{other}")),
        }
    }
}

/// Parse the `(W,H[,filter])` argument of `/resize`. `0` means "keep aspect".
fn parse_resize(arg: &str) -> Result<ImageOp, String> {
    let inner = arg
        .strip_prefix('(')
        .and_then(|a| a.strip_suffix(')'))
        .ok_or_else(|| format!("/resize wants parentheses: /resize:(W,H), got {arg:?}"))?;
    if inner.contains('%') {
        return Err("percent resize is not supported; give explicit pixels".to_string());
    }
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    let (w, h, filter) = match parts.as_slice() {
        [w, h] => (*w, *h, ResampleFilter::Lanczos3.cli_name()),
        // Superset of Appendix B's two-argument form: the resample filter
        // travels with the op, so the text form must be able to carry it.
        [w, h, filter] => (*w, *h, *filter),
        _ => return Err(format!("/resize wants (W,H[,filter]), got {arg:?}")),
    };
    let width = parse_axis(w, "width")?;
    let height = parse_axis(h, "height")?;
    if width.is_none() && height.is_none() {
        return Err("/resize with both axes unset does nothing".to_string());
    }
    let filter = ResampleFilter::from_cli_name(&filter.to_ascii_lowercase())
        .ok_or_else(|| format!("unknown resample filter {filter:?}"))?;
    Ok(ImageOp::Resize {
        width,
        height,
        filter,
    })
}

fn parse_axis(text: &str, what: &str) -> Result<Option<u32>, String> {
    match text {
        "0" => Ok(None),
        _ => text
            .parse::<u32>()
            .map(Some)
            .map_err(|_| format!("invalid resize {what} {text:?}")),
    }
}

/// Parse the `(L,T,R,B)` margin argument of `/crop`.
fn parse_crop(arg: &str) -> Result<ImageOp, String> {
    let inner = arg
        .strip_prefix('(')
        .and_then(|a| a.strip_suffix(')'))
        .ok_or_else(|| format!("/crop wants parentheses: /crop:(L,T,R,B), got {arg:?}"))?;
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    let [l, t, r, b] = parts.as_slice() else {
        return Err(format!("/crop wants (L,T,R,B), got {arg:?}"));
    };
    let margin = |text: &str, what: &str| {
        text.parse::<u32>()
            .map_err(|_| format!("invalid crop {what} {text:?}"))
    };
    Ok(ImageOp::Crop {
        left: margin(l, "left")?,
        top: margin(t, "top")?,
        right: margin(r, "right")?,
        bottom: margin(b, "bottom")?,
    })
}

/// Serialise an operation chain to the shared text form: whitespace-separated
/// Appendix-B switches, the format batch recipes and the CLI both use.
pub(crate) fn recipe_text(ops: &[ImageOp]) -> String {
    ops.iter()
        .map(ImageOp::to_cli)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse a chain produced by [`recipe_text`]. Empty text is an empty chain.
pub(crate) fn parse_recipe(text: &str) -> Result<Vec<ImageOp>, String> {
    text.split_whitespace().map(ImageOp::from_cli).collect()
}

/// Apply `ops` in order to an RGBA8 buffer, returning the resulting pixels and
/// dimensions. Pure: no I/O, no GPU.
pub(crate) fn apply(rgba: Vec<u8>, width: u32, height: u32, ops: &[ImageOp]) -> (Vec<u8>, u32, u32) {
    // `load_image` always produces exactly `width * height * 4` bytes, so this
    // is a defensive guard rather than an expected path.
    if rgba.len() != width as usize * height as usize * 4 {
        return (rgba, width, height);
    }
    let mut img = Rgba::new(rgba, width, height);
    for op in ops {
        img = apply_one(img, op);
    }
    (img.pixels, img.width, img.height)
}

fn apply_one(img: Rgba, op: &ImageOp) -> Rgba {
    match op {
        ImageOp::RotateLeft => img.rotate270(),
        ImageOp::RotateRight => img.rotate90(),
        ImageOp::Rotate180 => img.rotate180(),
        ImageOp::FlipHorizontal => img.flip_horizontal(),
        ImageOp::FlipVertical => img.flip_vertical(),
        ImageOp::Grayscale => img.grayscale(),
        ImageOp::Negative => img.negative(),
        ImageOp::Resize {
            width,
            height,
            filter,
        } => match target_dimensions(img.width, img.height, *width, *height) {
            Some((tw, th)) => {
                let src = into_image(img);
                let out = imageops::resize(&src, tw, th, filter.to_filter_type());
                Rgba::new(out.into_raw(), tw, th)
            }
            None => img,
        },
        ImageOp::Filter(kind) => {
            let (kernel, k, divisor, bias) = kind.parameters();
            let src = into_image(img);
            let out = convolve(&src, kernel, k, divisor, bias);
            Rgba::new(out.into_raw(), src.width(), src.height())
        }
        ImageOp::Crop {
            left,
            top,
            right,
            bottom,
        } => {
            let (w, h) = (img.width, img.height);
            let crop_w = w.saturating_sub(*left).saturating_sub(*right);
            let crop_h = h.saturating_sub(*top).saturating_sub(*bottom);
            img.crop(CropRect {
                x: (*left).min(w),
                y: (*top).min(h),
                width: crop_w,
                height: crop_h,
            })
        }
    }
}

fn into_image(img: Rgba) -> RgbaImage {
    RgbaImage::from_raw(img.width, img.height, img.pixels)
        .expect("RGBA buffer length was validated against the dimensions")
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
                Pixel([
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

#[cfg(test)]
mod tests {
    use super::*;
    // The tests construct pixel values, not edit buffers.
    use ::image::Rgba;

    /// Build a row-major RGBA buffer from grayscale values (alpha = 255).
    fn gray_pixels(values: &[u8]) -> Vec<u8> {
        values.iter().flat_map(|&v| [v, v, v, 255u8]).collect()
    }

    /// The source pixel sequence, as gray levels, from an RGBA buffer.
    fn gray_levels(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
    }

    #[test]
    fn rotate_right_swaps_dimensions_and_moves_pixels() {
        // 3x1 row [1, 2, 3] becomes a 1x3 column.
        let (out, w, h) = apply(gray_pixels(&[1, 2, 3]), 3, 1, &[ImageOp::RotateRight]);
        assert_eq!((w, h), (1, 3));
        let mut levels = gray_levels(&out);
        levels.sort_unstable();
        assert_eq!(levels, vec![1, 2, 3]);
    }

    #[test]
    fn rotate180_reverses_pixels() {
        let (out, w, h) = apply(gray_pixels(&[1, 2, 3, 4]), 2, 2, &[ImageOp::Rotate180]);
        assert_eq!((w, h), (2, 2));
        assert_eq!(gray_levels(&out), vec![4, 3, 2, 1]);
    }

    #[test]
    fn rotate_left_swaps_dimensions_and_preserves_pixels() {
        let (out, w, h) = apply(gray_pixels(&[1, 2, 3]), 3, 1, &[ImageOp::RotateLeft]);
        assert_eq!((w, h), (1, 3));
        let mut levels = gray_levels(&out);
        levels.sort_unstable();
        assert_eq!(levels, vec![1, 2, 3]);
    }

    #[test]
    fn flip_horizontal_reverses_row() {
        let (out, w, h) = apply(gray_pixels(&[1, 2, 3]), 3, 1, &[ImageOp::FlipHorizontal]);
        assert_eq!((w, h), (3, 1));
        assert_eq!(gray_levels(&out), vec![3, 2, 1]);
    }

    #[test]
    fn flip_vertical_reverses_rows() {
        // 2x2 [[1, 2], [3, 4]] -> [[3, 4], [1, 2]].
        let (out, w, h) = apply(gray_pixels(&[1, 2, 3, 4]), 2, 2, &[ImageOp::FlipVertical]);
        assert_eq!((w, h), (2, 2));
        assert_eq!(gray_levels(&out), vec![3, 4, 1, 2]);
    }

    #[test]
    fn grayscale_uses_documented_luma_weights() {
        // (77*100 + 150*150 + 29*200) >> 8 = 36000 >> 8 = 140.
        let rgba = vec![100, 150, 200, 255];
        let (out, w, h) = apply(rgba, 1, 1, &[ImageOp::Grayscale]);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![140, 140, 140, 255]);
    }

    #[test]
    fn negative_inverts_rgb_but_preserves_alpha() {
        let rgba = vec![10, 20, 30, 40];
        let (out, w, h) = apply(rgba, 1, 1, &[ImageOp::Negative]);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![245, 235, 225, 40]);
    }

    #[test]
    fn resize_changes_dimensions() {
        let src = gray_pixels(&[128; 16]); // 4x4
        let (out, w, h) = apply(
            src,
            4,
            4,
            &[ImageOp::Resize {
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
        let (out, w, h) = apply(
            src,
            4,
            2,
            &[ImageOp::Resize {
                width: Some(2),
                height: None,
                filter: ResampleFilter::Lanczos3,
            }],
        );
        assert_eq!((w, h), (2, 1));
        assert_eq!(out.len(), 8); // 2x1 RGBA
    }

    #[test]
    fn crop_trims_margins() {
        // 3x3 of 1..=9; trim one column left and one row top -> the 2x2 block
        // [[5, 6], [8, 9]].
        let src: Vec<u8> = (1..=9u8).flat_map(|v| [v, 0, 0, 255]).collect();
        let (out, w, h) = apply(
            src,
            3,
            3,
            &[ImageOp::Crop {
                left: 1,
                top: 1,
                right: 0,
                bottom: 0,
            }],
        );
        assert_eq!((w, h), (2, 2));
        assert_eq!(gray_levels(&out), vec![5, 6, 8, 9]);
    }

    #[test]
    fn crop_larger_than_the_image_is_a_no_op() {
        let src = gray_pixels(&[1, 2, 3, 4]);
        let (out, w, h) = apply(
            src.clone(),
            2,
            2,
            &[ImageOp::Crop {
                left: 10,
                top: 10,
                right: 10,
                bottom: 10,
            }],
        );
        assert_eq!((w, h), (2, 2));
        assert_eq!(out, src);
    }

    #[test]
    fn ops_apply_left_to_right() {
        // Rotate right then crop: the chain order must be observable.
        let (out, w, h) = apply(
            gray_pixels(&[1, 2, 3]),
            3,
            1,
            &[ImageOp::RotateRight, ImageOp::Crop {
                left: 0,
                top: 0,
                right: 0,
                bottom: 1,
            }],
        );
        assert_eq!((w, h), (1, 2));
        assert_eq!(gray_levels(&out), vec![1, 2]);
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
    fn filter_applies_through_apply() {
        let src = gray_pixels(&[100; 9]); // uniform 3x3
        let (out, w, h) = apply(src, 3, 3, &[ImageOp::Filter(FilterKind::Blur)]);
        assert_eq!((w, h), (3, 3));
        assert!(gray_levels(&out).iter().all(|&v| v == 100));
    }

    #[test]
    fn filter_edge_applies_bias_to_a_flat_field() {
        // Edge sums to zero, so a flat field becomes the +127 bias.
        let (out, ..) = apply(gray_pixels(&[0; 4]), 2, 2, &[ImageOp::Filter(FilterKind::Edge)]);
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

    /// Every variant survives `to_cli` -> `from_cli` unchanged.
    #[test]
    fn every_op_round_trips_through_the_text_form() {
        let ops = [
            ImageOp::RotateLeft,
            ImageOp::RotateRight,
            ImageOp::Rotate180,
            ImageOp::FlipHorizontal,
            ImageOp::FlipVertical,
            ImageOp::Grayscale,
            ImageOp::Negative,
            ImageOp::Resize {
                width: Some(800),
                height: None,
                filter: ResampleFilter::CatmullRom,
            },
            ImageOp::Resize {
                width: None,
                height: Some(600),
                filter: ResampleFilter::Lanczos3,
            },
            ImageOp::Filter(FilterKind::Gaussian5),
            ImageOp::Crop {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4,
            },
        ];
        for op in ops {
            assert_eq!(
                ImageOp::from_cli(&op.to_cli()).as_ref(),
                Ok(&op),
                "{op:?} round-trip"
            );
        }
    }

    /// A whole chain round-trips through `recipe_text` -> `parse_recipe`.
    #[test]
    fn recipe_text_round_trips_a_chain() {
        let ops = vec![
            ImageOp::RotateRight,
            ImageOp::Grayscale,
            ImageOp::Resize {
                width: Some(800),
                height: Some(600),
                filter: ResampleFilter::Lanczos3,
            },
        ];
        let text = recipe_text(&ops);
        assert_eq!(text, "/rotate:right /grayscale /resize:(800,600,lanczos3)");
        assert_eq!(parse_recipe(&text), Ok(ops));
    }

    #[test]
    fn from_cli_accepts_the_appendix_b_forms_case_insensitively() {
        assert_eq!(ImageOp::from_cli("/rotate:Left"), Ok(ImageOp::RotateLeft));
        assert_eq!(ImageOp::from_cli("/rotate:RIGHT"), Ok(ImageOp::RotateRight));
        assert_eq!(ImageOp::from_cli("/mirror"), Ok(ImageOp::FlipHorizontal));
        assert_eq!(ImageOp::from_cli("/flip"), Ok(ImageOp::FlipVertical));
        assert_eq!(
            ImageOp::from_cli("/resize:(800,600)"),
            Ok(ImageOp::Resize {
                width: Some(800),
                height: Some(600),
                filter: ResampleFilter::Lanczos3,
            }),
            "the two-argument Appendix-B form defaults the filter"
        );
        assert_eq!(
            ImageOp::from_cli("/filter:Gaussian3"),
            Ok(ImageOp::Filter(FilterKind::Gaussian3))
        );
        assert_eq!(
            ImageOp::from_cli("/crop:(1,2,3,4)"),
            Ok(ImageOp::Crop {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4
            })
        );
    }

    #[test]
    fn from_cli_rejects_unsupported_and_malformed_switches() {
        assert!(ImageOp::from_cli("/rotate:45")
            .err()
            .is_some_and(|e| e.contains("free rotate")));
        assert!(ImageOp::from_cli("/resize:(800,600%)")
            .err()
            .is_some_and(|e| e.contains("percent")));
        assert!(ImageOp::from_cli("/resize:(0,0)").is_err(), "no-op resize");
        assert!(ImageOp::from_cli("/nope").is_err());
        assert!(ImageOp::from_cli("grayscale").is_err(), "missing '/'");
        assert!(ImageOp::from_cli("/filter:tint").is_err());
        assert!(ImageOp::from_cli("/crop:(1,2,3)").is_err());
        assert!(parse_recipe("/rotate:right /bogus").is_err());
        assert_eq!(parse_recipe(""), Ok(vec![]), "an empty recipe is an empty chain");
    }
}
