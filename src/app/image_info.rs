//! Headers-only image metadata for the Image Information window (Roadmap §9).
//!
//! [`probe`] opens the file, parses headers and metadata chunks, and never
//! decodes pixels — the window must stay cheap even on a 4×100-megapixel
//! directory. DDS files take their own branch: the `image` crate's DDS decoder
//! cannot read BC4/BC5/BC7, so the header is parsed with `image_dds` instead.

use std::io::BufReader;
use std::path::Path;
use std::time::SystemTime;

use ::image::{ColorType, ExtendedColorType, ImageDecoder};

/// What the Image Information window shows about one file. Raw metadata blocks
/// stay raw (the Exif panel parses [`ImageInfo::exif`] itself); sized metadata
/// is reduced to a byte count.
#[derive(Debug)]
pub(crate) struct ImageInfo {
    /// The container format as the `image` crate names it ("Png", "Jpeg", …).
    pub format: String,
    pub width: u32,
    pub height: u32,
    /// Colour depth of the stored pixels, human-readable.
    pub colour_depth: String,
    /// True when the stored pixels carry an alpha channel.
    pub alpha: bool,
    /// Raw Exif block (TIFF structure), present on PNG/JPEG/WebP.
    pub exif: Option<Vec<u8>>,
    /// ICC profile size in bytes, when present.
    pub icc: Option<usize>,
    /// XMP packet size in bytes, when present.
    pub xmp: Option<usize>,
    /// IPTC block size in bytes, when present.
    pub iptc: Option<usize>,
    pub file_size: u64,
    pub modified: Option<SystemTime>,
    /// Compression label for DDS files ("BC3 (DXT5)", "uncompressed", …);
    /// `None` for every other format.
    pub dds_compression: Option<String>,
}

/// Gather the window's data for one file. Headers and metadata only.
pub(crate) fn probe(path: &Path) -> Result<ImageInfo, String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    let file_size = meta.len();
    let modified = meta.modified().ok();

    let is_dds = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"));
    if is_dds {
        return probe_dds(path, file_size, modified);
    }

    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let reader = ::image::ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|e| format!("cannot guess the format of {}: {e}", path.display()))?;
    let format = reader
        .format()
        .map(|f| format!("{f:?}"))
        .unwrap_or_else(|| "unknown".to_string());
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let (width, height) = decoder.dimensions();
    let original = decoder.original_color_type();
    let colour_depth = colour_depth_label(original);
    let alpha = has_alpha(decoder.color_type());
    // The raw blocks are header parses; keep errors per-field so one odd chunk
    // cannot blank the whole window.
    let exif = decoder.exif_metadata().map_err(|e| e.to_string())?;
    let icc = decoder.icc_profile().map_err(|e| e.to_string())?.map(|b| b.len());
    let xmp = decoder.xmp_metadata().map_err(|e| e.to_string())?.map(|b| b.len());
    let iptc = decoder.iptc_metadata().map_err(|e| e.to_string())?.map(|b| b.len());

    Ok(ImageInfo {
        format,
        width,
        height,
        colour_depth,
        alpha,
        exif,
        icc,
        xmp,
        iptc,
        file_size,
        modified,
        dds_compression: None,
    })
}

/// DDS files carry no Exif/ICC/XMP through `image_dds`; the DXGI header gives
/// the dimensions and the compression family. [`crate::app::image_info`] tests
/// pin the label mapping.
fn probe_dds(path: &Path, file_size: u64, modified: Option<SystemTime>) -> Result<ImageInfo, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let dds =
        image_dds::ddsfile::Dds::read(file).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let dxgi = dds.get_dxgi_format();
    let (width, height) = (dds.get_width(), dds.get_height());
    let compression = dxgi.map(dds_compression_label);
    Ok(ImageInfo {
        format: "Dds".to_string(),
        width,
        height,
        colour_depth: dxgi.map(dds_colour_depth).unwrap_or_else(|| "DDS".to_string()),
        alpha: dxgi.is_some_and(dds_has_alpha),
        exif: None,
        icc: None,
        xmp: None,
        iptc: None,
        file_size,
        modified,
        dds_compression: compression,
    })
}

/// Human label for a DDS DXGI format: the BC family when compressed, otherwise
/// the channel layout.
fn dds_compression_label(format: image_dds::ddsfile::DxgiFormat) -> String {
    use image_dds::ddsfile::DxgiFormat::*;
    match format {
        BC1_UNorm | BC1_UNorm_sRGB => "BC1 (DXT1)".to_string(),
        BC2_UNorm | BC2_UNorm_sRGB => "BC2 (DXT3)".to_string(),
        BC3_UNorm | BC3_UNorm_sRGB => "BC3 (DXT5)".to_string(),
        BC4_UNorm | BC4_SNorm => "BC4".to_string(),
        BC5_UNorm | BC5_SNorm => "BC5".to_string(),
        BC6H_UF16 | BC6H_SF16 => "BC6H (HDR)".to_string(),
        BC7_UNorm | BC7_UNorm_sRGB => "BC7".to_string(),
        _ => "uncompressed".to_string(),
    }
}

/// Colour-depth label for a DDS's DXGI format.
fn dds_colour_depth(format: image_dds::ddsfile::DxgiFormat) -> String {
    use image_dds::ddsfile::DxgiFormat::*;
    match format {
        BC1_UNorm | BC1_UNorm_sRGB => "RGB, 5:6:5 + 1-bit alpha (block-compressed)".to_string(),
        BC2_UNorm | BC2_UNorm_sRGB | BC3_UNorm | BC3_UNorm_sRGB | BC7_UNorm | BC7_UNorm_sRGB => {
            "RGBA, 8-bit/channel (block-compressed)".to_string()
        }
        BC4_UNorm | BC4_SNorm => "Single channel, 8-bit (block-compressed)".to_string(),
        BC5_UNorm | BC5_SNorm => "Two channels, 8-bit (block-compressed)".to_string(),
        BC6H_UF16 | BC6H_SF16 => "RGB half-float (HDR, block-compressed)".to_string(),
        R32G32B32A32_Float => "RGBA, 32-bit float".to_string(),
        R16G16B16A16_Float => "RGBA, 16-bit float".to_string(),
        R8G8B8A8_UNorm => "RGBA, 8-bit/channel".to_string(),
        _ => "DDS".to_string(),
    }
}

/// True for the DXGI formats whose stored pixels carry alpha.
fn dds_has_alpha(format: image_dds::ddsfile::DxgiFormat) -> bool {
    use image_dds::ddsfile::DxgiFormat::*;
    matches!(
        format,
        BC1_UNorm
            | BC1_UNorm_sRGB
            | BC2_UNorm
            | BC2_UNorm_sRGB
            | BC3_UNorm
            | BC3_UNorm_sRGB
            | BC7_UNorm
            | BC7_UNorm_sRGB
            | R32G32B32A32_Float
            | R16G16B16A16_Float
            | R8G8B8A8_UNorm
    )
}

/// Human label for the stored colour layout, including the sub-8-bit and
/// indexed forms `original_color_type` preserves.
fn colour_depth_label(original: ExtendedColorType) -> String {
    use ExtendedColorType::*;
    match original {
        L1 | L2 | L4 => "Grayscale, fewer than 8 bits".to_string(),
        La1 | La2 | La4 => "Grayscale + alpha, fewer than 8 bits".to_string(),
        L8 => "Grayscale, 8-bit".to_string(),
        L16 => "Grayscale, 16-bit".to_string(),
        La8 => "Grayscale + alpha, 8-bit".to_string(),
        La16 => "Grayscale + alpha, 16-bit".to_string(),
        Rgb8 => "RGB, 8-bit/channel".to_string(),
        Rgba8 => "RGBA, 8-bit/channel".to_string(),
        Rgb16 => "RGB, 16-bit/channel".to_string(),
        Rgba16 => "RGBA, 16-bit/channel".to_string(),
        Rgb32F => "RGB, 32-bit float".to_string(),
        Rgba32F => "RGBA, 32-bit float".to_string(),
        Bgr8 => "BGR, 8-bit/channel".to_string(),
        Bgra8 => "BGRA, 8-bit/channel".to_string(),
        Cmyk8 => "CMYK, 8-bit/channel".to_string(),
        Cmyk16 => "CMYK, 16-bit/channel".to_string(),
        Rgb1 | Rgb2 | Rgb4 | Rgba1 | Rgba2 | Rgba4 | Rgb5x1 => {
            "RGB(A), fewer than 8 bits".to_string()
        }
        Unknown(bits) => format!("{bits}-bit (indexed palette or unknown layout)"),
        A8 => "Alpha only, 8-bit".to_string(),
        // `#[non_exhaustive]` upstream: label whatever appears rather than
        // failing the probe.
        _ => "unsupported layout".to_string(),
    }
}

/// True when the stored pixels carry an alpha channel.
fn has_alpha(color: ColorType) -> bool {
    matches!(
        color,
        ColorType::La8 | ColorType::Rgba8 | ColorType::La16 | ColorType::Rgba16 | ColorType::Rgba32F
    )
}

/// "1.2 MB"-style file size.
pub(crate) fn format_size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::DECIMAL)
}

/// Local-time "2026-10-13 14:02:11" for the file's mtime, when it converts.
pub(crate) fn format_modified(time: SystemTime) -> String {
    // Dates before 1970 or beyond jiff's range format as the raw timestamp.
    match jiff::Timestamp::try_from(time) {
        Ok(timestamp) => timestamp
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => format!("{time:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode RGBA pixels to a PNG in memory.
    fn png_bytes(pixels: &[u8], w: u32, h: u32, exif: Option<Vec<u8>>) -> Vec<u8> {
        use ::image::codecs::png::PngEncoder;
        use ::image::{ExtendedColorType, ImageEncoder};
        let mut out = Vec::new();
        let mut encoder = PngEncoder::new(&mut out);
        if let Some(exif) = exif {
            encoder.set_exif_metadata(exif).expect("set exif");
        }
        encoder
            .write_image(pixels, w, h, ExtendedColorType::Rgba8)
            .expect("encode png");
        out
    }

    /// The synthetic little-endian TIFF Exif block shape `image_loader.rs`'
    /// orientation tests use: a header plus a handful of payload bytes.
    fn exif_block() -> Vec<u8> {
        let mut block = Vec::new();
        block.extend_from_slice(b"II\x2a\x00"); // little-endian TIFF magic
        block.extend_from_slice(&8u32.to_le_bytes()); // IFD offset
        block.extend_from_slice(&1u16.to_le_bytes()); // entry count
        block.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation tag
        block.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        block.extend_from_slice(&1u32.to_le_bytes()); // count
        block.extend_from_slice(&6u16.to_le_bytes()); // value: rotate 90°
        block.extend_from_slice(&0u16.to_le_bytes()); // next IFD = none
        block
    }

    #[test]
    fn probe_reads_a_png_headers_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("photo.png");
        let pixels: Vec<u8> = [10u8, 20, 30, 255].repeat(6);
        std::fs::write(&path, png_bytes(&pixels, 3, 2, Some(exif_block()))).expect("write");

        let info = probe(&path).expect("probe ok");
        assert_eq!(info.format, "Png");
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.colour_depth, "RGBA, 8-bit/channel");
        assert!(info.alpha);
        assert!(info.exif.is_some(), "the eXIf chunk survives the probe");
        assert_eq!(info.icc, None);
        assert_eq!(info.xmp, None);
        assert_eq!(info.iptc, None);
        assert!(info.file_size > 0);
        assert!(info.modified.is_some());
        assert_eq!(info.dds_compression, None);
    }

    #[test]
    fn probe_reports_a_png_without_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("plain.png");
        std::fs::write(&path, png_bytes(&[0; 8], 1, 2, None)).expect("write");

        let info = probe(&path).expect("probe ok");
        assert!(info.exif.is_none());
        assert_eq!(info.icc, None);
    }

    #[test]
    fn probe_fails_cleanly_on_a_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = probe(&dir.path().join("gone.png")).expect_err("must fail");
        assert!(err.contains("cannot stat") || err.contains("cannot open"), "{err}");
    }

    #[test]
    fn probe_reads_a_dds_header() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tex.dds");

        let mut dds = image_dds::ddsfile::Dds::new_dxgi(image_dds::ddsfile::NewDxgiParams {
            height: 8,
            width: 8,
            depth: None,
            format: image_dds::ddsfile::DxgiFormat::BC3_UNorm,
            mipmap_levels: Some(1),
            array_layers: None,
            caps2: None,
            is_cubemap: false,
            resource_dimension: image_dds::ddsfile::D3D10ResourceDimension::Texture2D,
            alpha_mode: image_dds::ddsfile::AlphaMode::Unknown,
        })
        .expect("build dds");
        dds.data = vec![0; 8 * 2 * 16]; // 8x8 = 2x2 BC3 blocks, 16 bytes each
        let mut file = std::fs::File::create(&path).expect("create");
        dds.write(&mut file).expect("write dds");
        drop(file);

        let info = probe(&path).expect("probe ok");
        assert_eq!(info.format, "Dds");
        assert_eq!((info.width, info.height), (8, 8));
        assert_eq!(info.dds_compression.as_deref(), Some("BC3 (DXT5)"));
        assert_eq!(info.colour_depth, "RGBA, 8-bit/channel (block-compressed)");
        assert!(info.alpha);
        assert!(info.exif.is_none());
    }

    #[test]
    fn colour_depth_labels_cover_the_common_layouts() {
        use ExtendedColorType as E;
        assert_eq!(colour_depth_label(E::Rgb8), "RGB, 8-bit/channel");
        assert_eq!(colour_depth_label(E::Rgba16), "RGBA, 16-bit/channel");
        assert_eq!(colour_depth_label(E::Rgba32F), "RGBA, 32-bit float");
        assert_eq!(colour_depth_label(E::L8), "Grayscale, 8-bit");
        assert_eq!(colour_depth_label(E::Unknown(8)), "8-bit (indexed palette or unknown layout)");
    }

    #[test]
    fn alpha_detection_follows_the_channel_layout() {
        use ::image::ColorType as C;
        assert!(has_alpha(C::Rgba8));
        assert!(has_alpha(C::La16));
        assert!(has_alpha(C::Rgba32F));
        assert!(!has_alpha(C::Rgb8));
        assert!(!has_alpha(C::L8));
    }

    #[test]
    fn size_and_date_format_helpers() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        // 1.5 MB in decimal units.
        assert_eq!(format_size(1_500_000), "1.50 MB");

        let epoch = SystemTime::UNIX_EPOCH;
        let formatted = format_modified(epoch);
        // Timezone-dependent clock reading, but always this shape.
        assert_eq!(formatted.len(), 19, "{formatted}");
        assert_eq!(&formatted[4..5], "-");
        assert_eq!(&formatted[10..11], " ");
        assert_eq!(&formatted[13..14], ":");
    }
}
