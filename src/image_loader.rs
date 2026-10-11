use std::path::Path;

use ::image::metadata::Orientation;
use ::image::ImageDecoder;

/// Upper bound on decoded layers per file. Texture arrays can be arbitrarily
/// deep; decoding/uploading every layer of a huge array would exhaust memory.
/// Cubemaps (6 faces) are nowhere near this.
const MAX_DECODED_LAYERS: u32 = 64;

/// The transfer function of an image's stored bytes.
///
/// A property of the *image*, not of any one codec, so EXR/HDR/RAW (linear-light)
/// and future ICC work read the same field. Photon's canvas is a display/gamma
/// pass (matching egui's non-sRGB target), so a `Srgb` image is drawn as-is and a
/// `Linear` one is encoded to display space in the fragment shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    /// Bytes are already display-encoded (sRGB); drawn as-is.
    Srgb,
    /// Linear-light data; the shader encodes it for display.
    Linear,
}

/// Block-compressed layouts kept compressed for direct GPU upload (mip 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BcKind {
    Bc1,
    Bc2,
    Bc3,
    Bc4Unorm,
    Bc4Snorm,
    Bc5Unorm,
    Bc5Snorm,
    Bc7,
}

/// How the bytes in [`DecodedImage::layers`] are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedFormat {
    /// 8-bit RGBA, `width * height * 4` bytes per layer.
    Rgba8,
    /// Block-compressed BC1–BC7, uploaded to a matching wgpu BC texture.
    Bc(BcKind),
    /// Half-float RGBA (little-endian `f16`, 8 bytes per texel), uploaded to an
    /// `Rgba16Float` texture. Used for HDR sources (EXR/HDR, float/BC6H DDS) so
    /// values above 1.0 survive to the tonemap.
    Float16,
}

/// Decode-time capabilities. `allow_bc` requests keeping BC-compressed DDS
/// surfaces compressed; `max_dim` is the GPU's per-texture limit, above which a
/// compressed image falls back to the CPU path so it can be chunked.
#[derive(Debug, Clone, Copy)]
pub struct DecodeOptions {
    pub allow_bc: bool,
    pub max_dim: u32,
}

impl DecodeOptions {
    /// Always decode to RGBA8 — the batch/clipboard/save callers need pixels.
    pub fn cpu() -> Self {
        Self {
            allow_bc: false,
            max_dim: u32::MAX,
        }
    }
}

/// Decoded pixels of an image file: one buffer per layer (cubemap face or
/// texture-array entry), all sharing `width`/`height`. Ordinary images have
/// exactly one layer. The buffer layout is [`DecodedImage::format`] — RGBA8, or
/// still-block-compressed BC bytes for a DDS loaded with [`DecodeOptions::allow_bc`].
pub struct DecodedImage {
    pub layers: Vec<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// True when the layers are cubemap faces (drives face labels in the UI).
    pub is_cubemap: bool,
    /// How the bytes in `layers` are stored (RGBA8 or a BC layout).
    pub format: DecodedFormat,
    /// The transfer function of the stored bytes; see [`ColorSpace`].
    pub color_space: ColorSpace,
}

/// Typed decode failure, replacing the old `Result<_, String>` + string-prefix
/// classification (Roadmap §8). The variants form the fixed vocabulary the
/// roadmap calls for; the payload carries the underlying error text so the
/// user-visible overlay message stays as detailed as before.
///
/// Some variants are not constructed yet — they are the agreed vocabulary for
/// the broader codec work in §8 — so the whole enum is allowed to be dead.
#[allow(dead_code)]
#[derive(Debug)]
pub enum DecodeError {
    /// Recognized format that this build cannot decode, or malformed pixel
    /// data for a format it can. Shown as the "unsupported format" overlay.
    UnsupportedFormat(String),
    /// The file's format could not be determined.
    UnknownFormat(String),
    OutOfMemory,
    ColorNotSupported,
    ReadError,
    WriteError,
    /// Malformed or inconsistent file data (bad header, truncated stream,
    /// layer byte count that doesn't match the reported dimensions).
    InvalidData(String),
    FileNotFound(String),
    Aborted,
    GrayscaleOnly,
    OtherComponentsRequired,
    SizeNotSupported,
    CantGetInformation,
}

impl DecodeError {
    /// Whether this failure should surface as the "unsupported format" overlay
    /// rather than the generic error overlay. Reproduces the old
    /// `e.starts_with("Failed to decode")` classification exactly: decode-time
    /// failures (image decode, DDS header/decode/size checks) are format
    /// errors; open and format-guess failures are not.
    pub fn is_unsupported_format(&self) -> bool {
        matches!(
            self,
            DecodeError::UnsupportedFormat(_) | DecodeError::InvalidData(_)
        )
    }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnsupportedFormat(e) => write!(f, "Failed to decode image: {}", e),
            DecodeError::UnknownFormat(e) => write!(f, "Failed to guess format: {}", e),
            DecodeError::OutOfMemory => write!(f, "Out of memory while decoding image"),
            DecodeError::ColorNotSupported => write!(f, "Image colour type is not supported"),
            DecodeError::ReadError => write!(f, "Failed to read image data"),
            DecodeError::WriteError => write!(f, "Failed to write image data"),
            DecodeError::InvalidData(e) => write!(f, "Failed to decode image: {}", e),
            DecodeError::FileNotFound(e) => write!(f, "Failed to open file: {}", e),
            DecodeError::Aborted => write!(f, "Image decoding was aborted"),
            DecodeError::GrayscaleOnly => write!(f, "Only grayscale images are supported"),
            DecodeError::OtherComponentsRequired => {
                write!(f, "Image requires other colour components")
            }
            DecodeError::SizeNotSupported => write!(f, "Image dimensions are not supported"),
            DecodeError::CantGetInformation => write!(f, "Could not read image information"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Load an image and return its layers as RGBA pixels with dimensions.
///
/// `.dds` files are routed through `image_dds` (which wraps `ddsfile` for
/// header parsing and decodes BC1–BC7 + uncompressed variants). All other
/// supported formats go through the `image` crate as before and produce a
/// single layer.
///
/// The `image`-crate path applies the image's Exif orientation before handing
/// back pixels, so phone photos with a rotated sensor load upright and the
/// reported dimensions reflect the rotation. Formats without Exif (or with a
/// broken Exif block) decode unrotated.
pub fn load_image(path: &Path) -> Result<DecodedImage, DecodeError> {
    load_image_opts(path, DecodeOptions::cpu())
}

/// [`load_image`] with decode capabilities: when `opts.allow_bc` is set and the
/// file is a BC-compressed DDS within `opts.max_dim`, the layers come back still
/// compressed ([`DecodedFormat::Bc`]) for a direct GPU upload. Every other path
/// (and all non-DDS formats) yields RGBA8 exactly as [`load_image`] does.
pub fn load_image_opts(path: &Path, opts: DecodeOptions) -> Result<DecodedImage, DecodeError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    if extension.as_deref() == Some("dds") {
        return load_dds(path, opts);
    }

    let file = std::fs::File::open(path).map_err(|e| DecodeError::FileNotFound(e.to_string()))?;
    let reader = std::io::BufReader::new(file);

    let img_reader = ::image::ImageReader::new(reader)
        .with_guessed_format()
        .map_err(|e| DecodeError::UnknownFormat(e.to_string()))?;

    let mut decoder = img_reader
        .into_decoder()
        .map_err(|e| DecodeError::UnsupportedFormat(e.to_string()))?;

    // Exif orientation (roadmap §3). A missing or malformed EXIF block must
    // never fail an otherwise-decodable image, so an orientation error falls
    // back to `NoTransforms` and we decode the pixels as-is.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);

    let mut img = ::image::DynamicImage::from_decoder(decoder)
        .map_err(|e| DecodeError::UnsupportedFormat(e.to_string()))?;
    img.apply_orientation(orientation);

    let (w, h) = (img.width(), img.height());
    let color_space = color_space_for_extension(extension.as_deref());

    // Linear-light sources (EXR/HDR) decode to f32; keep the HDR range as
    // half-float rather than clamping it through 8-bit.
    let (layers, format) = if color_space == ColorSpace::Linear {
        let rgba = img.into_rgba32f();
        (
            vec![pack_f16_rgba(rgba.as_raw())],
            DecodedFormat::Float16,
        )
    } else {
        (vec![img.to_rgba8().into_raw()], DecodedFormat::Rgba8)
    };

    Ok(DecodedImage {
        layers,
        width: w,
        height: h,
        is_cubemap: false,
        format,
        color_space,
    })
}

/// Pack interleaved f32 RGBA into little-endian `f16` bytes — the layout
/// [`DecodedFormat::Float16`] carries and an `Rgba16Float` texture uploads.
fn pack_f16_rgba(pixels: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 2);
    for &component in pixels {
        out.extend_from_slice(&half::f16::from_f32(component).to_le_bytes());
    }
    out
}

/// The colour space an `image`-crate format carries. Radiance HDR and OpenEXR
/// are linear-light; every other format Photon reads is display-encoded sRGB.
fn color_space_for_extension(extension: Option<&str>) -> ColorSpace {
    match extension {
        Some("exr") | Some("hdr") => ColorSpace::Linear,
        _ => ColorSpace::Srgb,
    }
}

/// Decode a `.dds` file via `image_dds`. Covers DXT1–DXT5, BC4, BC5 (ATI2/BC5U),
/// BC7, and uncompressed variants — a strict superset of `image`'s built-in
/// DDS decoder, which rejects everything past DXT5 / the legacy FourCC set.
///
/// Every layer (cubemap face or array entry) is decoded; the layers are
/// stacked vertically inside the file by `image_dds`, so we split the stack
/// back into per-face buffers here.
fn load_dds(path: &Path, opts: DecodeOptions) -> Result<DecodedImage, DecodeError> {
    let file = std::fs::File::open(path).map_err(|e| DecodeError::FileNotFound(e.to_string()))?;
    let dds = image_dds::ddsfile::Dds::read(file)
        .map_err(|e| DecodeError::InvalidData(format!("Failed to read DDS header: {}", e)))?;

    let layer_count = dds_layer_count(&dds).min(MAX_DECODED_LAYERS);
    let (w, h) = (dds.get_width(), dds.get_height());
    let is_cubemap = dds_is_cubemap(&dds);
    let color_space = dds_color_space(&dds);

    // Native path: keep the compressed mip-0 surfaces for direct GPU upload when
    // the format is a supported BC layout, the caller allows it, and the image
    // fits one texture. Chunking can't slice a compressed stream on a block
    // boundary, so anything over the limit falls through to the CPU path.
    if opts.allow_bc && w > 0 && h > 0 && w <= opts.max_dim && h <= opts.max_dim {
        if let Some(kind) = dds.get_dxgi_format().and_then(dxgi_bc_kind) {
            let surface = image_dds::Surface::from_dds(&dds)
                .map_err(|e| DecodeError::InvalidData(format!("DDS surface error: {}", e)))?;
            let mut layers = Vec::with_capacity(layer_count as usize);
            for layer in 0..layer_count {
                let data = surface.get(layer, 0, 0).ok_or_else(|| {
                    DecodeError::InvalidData(format!("DDS layer {} mip 0 missing", layer))
                })?;
                layers.push(data.to_vec());
            }
            return Ok(DecodedImage {
                layers,
                width: w,
                height: h,
                is_cubemap,
                format: DecodedFormat::Bc(kind),
                color_space,
            });
        }
    }

    // Float path: BC6H and uncompressed float formats decode to f32 and are kept
    // as half-float so the HDR range survives (mip 0, all layers).
    if dds.get_dxgi_format().is_some_and(dxgi_is_float) {
        let img =
            image_dds::SurfaceRgba32Float::decode_layers_mipmaps_dds(&dds, 0..layer_count, 0..1)
                .map_err(|e| DecodeError::InvalidData(format!("DDS float decode failed: {}", e)))?
                .into_image()
                .map_err(|e| DecodeError::InvalidData(format!("DDS float decode failed: {}", e)))?;

        let layer_floats = (w as usize) * (h as usize) * 4;
        let raw = img.into_raw();
        if layer_floats == 0 || raw.len() % layer_floats != 0 {
            return Err(DecodeError::InvalidData(format!(
                "DDS float decode produced {} components, not a multiple of {}x{}x4",
                raw.len(),
                w,
                h
            )));
        }
        let layers = raw.chunks_exact(layer_floats).map(pack_f16_rgba).collect();
        return Ok(DecodedImage {
            layers,
            width: w,
            height: h,
            is_cubemap,
            format: DecodedFormat::Float16,
            color_space,
        });
    }

    // CPU fallback: decode every layer's mip 0 to RGBA8.
    let img = image_dds::SurfaceRgba8::decode_layers_mipmaps_dds(&dds, 0..layer_count, 0..1)
        .map_err(|e| DecodeError::InvalidData(format!("DDS decode failed: {}", e)))?
        .into_image()
        .map_err(|e| DecodeError::InvalidData(format!("DDS decode failed: {}", e)))?;

    // The decoded image stacks all layers vertically, so its reported height
    // is count × face-height; the true per-layer dimensions are in the header.
    let layer_bytes = (w as usize) * (h as usize) * 4;
    let raw = img.into_raw();
    if layer_bytes == 0 || raw.len() % layer_bytes != 0 {
        return Err(DecodeError::InvalidData(format!(
            "DDS decode produced {} bytes, not a multiple of {}x{}x4",
            raw.len(),
            w,
            h
        )));
    }
    let layers = raw.chunks_exact(layer_bytes).map(<[u8]>::to_vec).collect();

    Ok(DecodedImage {
        layers,
        width: w,
        height: h,
        is_cubemap,
        format: DecodedFormat::Rgba8,
        color_space,
    })
}

/// The colour space of a DDS's stored bytes: an `*_sRGB` DXGI format (which the
/// legacy DXT FourCCs map to) is display-encoded; everything else — non-sRGB BC,
/// uncompressed UNORM, float — is treated as linear-light.
fn dds_color_space(dds: &image_dds::ddsfile::Dds) -> ColorSpace {
    match dds.get_dxgi_format() {
        Some(f) if is_srgb_dxgi(f) => ColorSpace::Srgb,
        _ => ColorSpace::Linear,
    }
}

fn is_srgb_dxgi(format: image_dds::ddsfile::DxgiFormat) -> bool {
    use image_dds::ddsfile::DxgiFormat::*;
    matches!(
        format,
        BC1_UNorm_sRGB | BC2_UNorm_sRGB | BC3_UNorm_sRGB | BC7_UNorm_sRGB
    )
}

/// True for DXGI formats that carry float (or BC6H HDR) samples: these take the
/// `Rgba16Float` path rather than the 8-bit one. Depth/stencil float formats are
/// excluded — they are not colour images.
fn dxgi_is_float(format: image_dds::ddsfile::DxgiFormat) -> bool {
    use image_dds::ddsfile::DxgiFormat::*;
    matches!(
        format,
        R32G32B32A32_Float
            | R32G32B32_Float
            | R16G16B16A16_Float
            | R32G32_Float
            | R11G11B10_Float
            | R16G16_Float
            | R32_Float
            | R16_Float
            | BC6H_UF16
            | BC6H_SF16
    )
}

/// Map a DXGI format to the BC layout we can upload directly. `Bc6h`/float and
/// typeless formats return `None` (they fall back to the CPU decode).
fn dxgi_bc_kind(format: image_dds::ddsfile::DxgiFormat) -> Option<BcKind> {
    use image_dds::ddsfile::DxgiFormat::*;
    Some(match format {
        BC1_UNorm | BC1_UNorm_sRGB => BcKind::Bc1,
        BC2_UNorm | BC2_UNorm_sRGB => BcKind::Bc2,
        BC3_UNorm | BC3_UNorm_sRGB => BcKind::Bc3,
        BC4_UNorm => BcKind::Bc4Unorm,
        BC4_SNorm => BcKind::Bc4Snorm,
        BC5_UNorm => BcKind::Bc5Unorm,
        BC5_SNorm => BcKind::Bc5Snorm,
        BC7_UNorm | BC7_UNorm_sRGB => BcKind::Bc7,
        _ => return None,
    })
}

/// True for legacy (`caps2`) and DXGI (`misc_flag`) cubemaps.
fn dds_is_cubemap(dds: &image_dds::ddsfile::Dds) -> bool {
    dds.header
        .caps2
        .contains(image_dds::ddsfile::Caps2::CUBEMAP)
        || dds
            .header10
            .as_ref()
            .is_some_and(|h| h.misc_flag == image_dds::ddsfile::MiscFlag::TEXTURECUBE)
}

/// Number of array layers in the DDS, matching `image_dds`'s interpretation:
/// DXGI cubemaps report `array_size * 6`, legacy cubemaps 6, everything else
/// its array size.
fn dds_layer_count(dds: &image_dds::ddsfile::Dds) -> u32 {
    let dxgi_cube = dds
        .header10
        .as_ref()
        .is_some_and(|h| h.misc_flag == image_dds::ddsfile::MiscFlag::TEXTURECUBE);
    let count = dds.get_num_array_layers().max(1);
    if dxgi_cube {
        count * 6
    } else {
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small uncompressed RGBA8 DDS in memory, write it to a tempdir,
    /// and verify `load_image` round-trips it back to the same pixels.
    #[test]
    fn test_load_dds_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rgba8.dds");

        let (w, h) = (4u32, 2u32);
        let pixels: Vec<u8> = (0..(w * h * 4)).map(|i| i as u8).collect();

        let mut dds = image_dds::ddsfile::Dds::new_d3d(image_dds::ddsfile::NewD3dParams {
            height: h,
            width: w,
            depth: None,
            format: image_dds::ddsfile::D3DFormat::A8B8G8R8,
            mipmap_levels: Some(1),
            caps2: None,
        })
        .expect("build dds");
        dds.data = pixels.clone();
        let mut f = std::fs::File::create(&path).expect("create file");
        dds.write(&mut f).expect("write dds");

        let decoded = load_image(&path).expect("load_image ok");
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0], pixels);
        assert!(!decoded.layers[0].is_empty());
    }

    /// Build a 6-face cubemap DDS with distinct faces and verify `load_image`
    /// returns all six, in file order, each as its own buffer.
    #[test]
    fn test_load_dds_cubemap_returns_all_faces() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cubemap.dds");

        let (w, h) = (4u32, 4u32);
        let faces: Vec<Vec<u8>> = (0..6)
            .map(|face| {
                (0..(w * h * 4))
                    .map(|i| ((i % 251) as u8).wrapping_add(face as u8 * 7))
                    .collect()
            })
            .collect();

        let mut dds = image_dds::ddsfile::Dds::new_d3d(image_dds::ddsfile::NewD3dParams {
            height: h,
            width: w,
            depth: None,
            format: image_dds::ddsfile::D3DFormat::A8B8G8R8,
            mipmap_levels: Some(1),
            caps2: Some(
                image_dds::ddsfile::Caps2::CUBEMAP | image_dds::ddsfile::Caps2::CUBEMAP_ALLFACES,
            ),
        })
        .expect("build dds");
        dds.data = faces.iter().flatten().copied().collect();
        let mut f = std::fs::File::create(&path).expect("create file");
        dds.write(&mut f).expect("write dds");

        let decoded = load_image(&path).expect("load_image ok");
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers, faces);
        assert!(decoded.is_cubemap);
    }

    /// Build a 3-entry DX10 texture array and verify all three layers decode,
    /// labeled as an array (not a cubemap).
    #[test]
    fn test_load_dds_dx10_array_returns_all_layers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("array.dds");

        let (w, h) = (4u32, 4u32);
        let layers_px: Vec<Vec<u8>> = (0..3)
            .map(|layer| vec![layer as u8 + 1; (w * h * 4) as usize])
            .collect();

        let mut dds = image_dds::ddsfile::Dds::new_dxgi(image_dds::ddsfile::NewDxgiParams {
            height: h,
            width: w,
            depth: None,
            format: image_dds::ddsfile::DxgiFormat::R8G8B8A8_UNorm,
            mipmap_levels: Some(1),
            array_layers: Some(3),
            caps2: None,
            is_cubemap: false,
            resource_dimension: image_dds::ddsfile::D3D10ResourceDimension::Texture2D,
            alpha_mode: image_dds::ddsfile::AlphaMode::Unknown,
        })
        .expect("build dds");
        dds.data = layers_px.iter().flatten().copied().collect();
        let mut f = std::fs::File::create(&path).expect("create file");
        dds.write(&mut f).expect("write dds");

        let decoded = load_image(&path).expect("load_image ok");
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers, layers_px);
        assert!(!decoded.is_cubemap);
    }

    /// A missing file is an open failure, not a format error.
    #[test]
    fn test_missing_file_is_not_format_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.png");

        let err = match load_image(&path) {
            Ok(_) => panic!("missing file must fail"),
            Err(e) => e,
        };
        assert!(matches!(err, DecodeError::FileNotFound(_)));
        assert!(
            !err.is_unsupported_format(),
            "open failure is not a format error"
        );
        assert!(err.to_string().starts_with("Failed to open file:"));
    }

    /// Malformed DDS bytes reach the header/decode path and classify as a
    /// format error (matching the old "Failed to decode" prefix behaviour).
    #[test]
    fn test_corrupt_dds_is_format_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrupt.dds");
        std::fs::write(&path, b"not a real dds file").expect("write");

        let err = match load_image(&path) {
            Ok(_) => panic!("corrupt dds must fail"),
            Err(e) => e,
        };
        assert!(matches!(err, DecodeError::InvalidData(_)));
        assert!(
            err.is_unsupported_format(),
            "decode failure is a format error"
        );
    }

    /// Encode a small RGBA8 PNG in memory, optionally tagged with a raw Exif
    /// chunk (little-endian TIFF header) via the PNG `eXIf` chunk.
    fn encode_rgba_png(pixels: &[u8], w: u32, h: u32, exif: Option<Vec<u8>>) -> Vec<u8> {
        use ::image::codecs::png::PngEncoder;
        use ::image::{ExtendedColorType, ImageEncoder};

        let mut out = Vec::new();
        {
            let mut encoder = PngEncoder::new(&mut out);
            if let Some(exif) = exif {
                encoder.set_exif_metadata(exif).expect("set exif");
            }
            encoder
                .write_image(pixels, w, h, ExtendedColorType::Rgba8)
                .expect("encode png");
        }
        out
    }

    /// A minimal big-endian-free (little-endian) TIFF Exif chunk carrying only
    /// an orientation entry (tag 0x112). Layout: "II*\0", IFD offset 8, one
    /// entry, the orientation, then a null next-IFD pointer.
    fn exif_chunk_with_orientation(orientation: u8) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(b"II\x2a\x00"); // little-endian TIFF magic
        chunk.extend_from_slice(&8u32.to_le_bytes()); // IFD offset
        chunk.extend_from_slice(&1u16.to_le_bytes()); // entry count
        chunk.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation tag
        chunk.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        chunk.extend_from_slice(&1u32.to_le_bytes()); // count
        chunk.extend_from_slice(&(orientation as u16).to_le_bytes()); // value
        chunk.extend_from_slice(&0u16.to_le_bytes()); // padding
        chunk.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none
        chunk
    }

    /// Pin the Exif byte <-> `Orientation` mapping the wiring relies on.
    #[test]
    fn test_orientation_exif_mapping() {
        assert_eq!(Orientation::from_exif(1), Some(Orientation::NoTransforms));
        assert_eq!(Orientation::from_exif(6), Some(Orientation::Rotate90));
        assert_eq!(Orientation::from_exif(8), Some(Orientation::Rotate270));
        assert_eq!(Orientation::from_exif(9), None);

        for orient in [
            Orientation::NoTransforms,
            Orientation::Rotate90,
            Orientation::Rotate180,
            Orientation::Rotate270,
            Orientation::FlipHorizontal,
            Orientation::FlipVertical,
            Orientation::Rotate90FlipH,
            Orientation::Rotate270FlipH,
        ] {
            assert_eq!(
                Orientation::from_exif(orient.to_exif()),
                Some(orient),
                "to_exif/from_exif round-trip for {orient:?}"
            );
        }
    }

    /// An ordinary image with no Exif decodes with unchanged dimensions and
    /// pixels (orientation falls back to `NoTransforms`).
    #[test]
    fn test_no_exif_png_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("plain.png");

        let (w, h) = (3u32, 2u32);
        let pixels: Vec<u8> = (0..(w * h)).flat_map(|i| [i as u8, 0, 0, 255]).collect();
        let png = encode_rgba_png(&pixels, w, h, None);
        std::fs::write(&path, png).expect("write");

        let decoded = load_image(&path).expect("load_image ok");
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(
            decoded.layers[0], pixels,
            "no-Exif pixels must be untouched"
        );
        assert!(!decoded.is_cubemap);
    }

    /// An image tagged Exif orientation 6 (rotate 90° CW) decodes upright: the
    /// reported dimensions swap and the pixels are rotated, not dropped.
    #[test]
    fn test_exif_orientation_rotates_and_swaps_dimensions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rotated.png");

        let (w, h) = (3u32, 2u32);
        // Distinct pixel values so a rotation is detectable as a permutation.
        let pixels: Vec<u8> = (0..(w * h)).flat_map(|i| [i as u8, 0, 0, 255]).collect();
        let exif = exif_chunk_with_orientation(6);
        let png = encode_rgba_png(&pixels, w, h, Some(exif));
        std::fs::write(&path, png).expect("write");

        let decoded = load_image(&path).expect("load_image ok");
        // Rotate90 swaps width and height.
        assert_eq!((decoded.width, decoded.height), (h, w));

        // Every source pixel survives, just relocated.
        let mut got: Vec<[u8; 4]> = decoded.layers[0]
            .chunks(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect();
        let mut want: Vec<[u8; 4]> = pixels.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(
            got, want,
            "rotation must preserve pixel data as a permutation"
        );
    }

    /// The variant -> classification mapping for the whole vocabulary.
    #[test]
    fn test_unsupported_format_classification() {
        let format_errors = [
            DecodeError::UnsupportedFormat("x".into()),
            DecodeError::InvalidData("x".into()),
        ];
        for e in &format_errors {
            assert!(e.is_unsupported_format(), "{e:?} should be a format error");
        }

        let other_errors = [
            DecodeError::UnknownFormat("x".into()),
            DecodeError::FileNotFound("x".into()),
            DecodeError::OutOfMemory,
            DecodeError::ColorNotSupported,
            DecodeError::ReadError,
            DecodeError::WriteError,
            DecodeError::Aborted,
            DecodeError::GrayscaleOnly,
            DecodeError::OtherComponentsRequired,
            DecodeError::SizeNotSupported,
            DecodeError::CantGetInformation,
        ];
        for e in &other_errors {
            assert!(!e.is_unsupported_format(), "{e:?} is not a format error");
        }
    }

    #[test]
    fn color_space_is_srgb_except_hdr_formats() {
        assert_eq!(color_space_for_extension(Some("png")), ColorSpace::Srgb);
        assert_eq!(color_space_for_extension(Some("jpg")), ColorSpace::Srgb);
        assert_eq!(color_space_for_extension(Some("jpeg")), ColorSpace::Srgb);
        assert_eq!(color_space_for_extension(Some("dds")), ColorSpace::Srgb);
        assert_eq!(color_space_for_extension(Some("exr")), ColorSpace::Linear);
        assert_eq!(color_space_for_extension(Some("hdr")), ColorSpace::Linear);
        assert_eq!(color_space_for_extension(None), ColorSpace::Srgb);
    }

    #[test]
    fn dxgi_bc_kind_maps_the_uploadable_layouts() {
        use image_dds::ddsfile::DxgiFormat::*;
        assert_eq!(dxgi_bc_kind(BC1_UNorm), Some(BcKind::Bc1));
        assert_eq!(dxgi_bc_kind(BC1_UNorm_sRGB), Some(BcKind::Bc1));
        assert_eq!(dxgi_bc_kind(BC2_UNorm_sRGB), Some(BcKind::Bc2));
        assert_eq!(dxgi_bc_kind(BC3_UNorm), Some(BcKind::Bc3));
        assert_eq!(dxgi_bc_kind(BC4_UNorm), Some(BcKind::Bc4Unorm));
        assert_eq!(dxgi_bc_kind(BC4_SNorm), Some(BcKind::Bc4Snorm));
        assert_eq!(dxgi_bc_kind(BC5_UNorm), Some(BcKind::Bc5Unorm));
        assert_eq!(dxgi_bc_kind(BC5_SNorm), Some(BcKind::Bc5Snorm));
        assert_eq!(dxgi_bc_kind(BC7_UNorm_sRGB), Some(BcKind::Bc7));
        // Float and typeless layouts are not uploadable; they fall back to CPU.
        assert_eq!(dxgi_bc_kind(BC6H_UF16), None);
        assert_eq!(dxgi_bc_kind(BC6H_SF16), None);
        assert_eq!(dxgi_bc_kind(R8G8B8A8_UNorm), None);
    }

    #[test]
    fn is_srgb_dxgi_flags_only_the_srgb_variants() {
        use image_dds::ddsfile::DxgiFormat::*;
        assert!(is_srgb_dxgi(BC1_UNorm_sRGB));
        assert!(is_srgb_dxgi(BC3_UNorm_sRGB));
        assert!(is_srgb_dxgi(BC7_UNorm_sRGB));
        assert!(!is_srgb_dxgi(BC1_UNorm));
        assert!(!is_srgb_dxgi(BC5_UNorm));
        assert!(!is_srgb_dxgi(R8G8B8A8_UNorm));
    }

    /// Write an in-memory DDS to `path`, overwriting its `data` with `data`.
    fn write_dds_with_data(path: &std::path::Path, mut dds: image_dds::ddsfile::Dds, data: Vec<u8>) {
        dds.data = data;
        let mut f = std::fs::File::create(path).expect("create file");
        dds.write(&mut f).expect("write dds");
    }

    /// Build a DXGI DDS (single array layer, mip 0 only) for a BC format.
    fn new_dxgi_dds(
        w: u32,
        h: u32,
        format: image_dds::ddsfile::DxgiFormat,
    ) -> image_dds::ddsfile::Dds {
        image_dds::ddsfile::Dds::new_dxgi(image_dds::ddsfile::NewDxgiParams {
            height: h,
            width: w,
            depth: None,
            format,
            mipmap_levels: Some(1),
            array_layers: Some(1),
            caps2: None,
            is_cubemap: false,
            resource_dimension: image_dds::ddsfile::D3D10ResourceDimension::Texture2D,
            alpha_mode: image_dds::ddsfile::AlphaMode::Unknown,
        })
        .expect("build dds")
    }

    /// A BC1 DDS loaded with `allow_bc` keeps its raw block bytes compressed
    /// instead of decoding to RGBA8. 8x8 BC1 = 2x2 blocks of 8 bytes = 32 bytes.
    #[test]
    fn dds_bc1_allowed_keeps_compressed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bc1.dds");

        let (w, h) = (8u32, 8u32);
        let blocks: Vec<u8> = (0..32u8).collect();
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::BC1_UNorm);
        write_dds_with_data(&path, dds, blocks.clone());

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: true,
                max_dim: 16384,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Bc(BcKind::Bc1));
        assert_eq!(
            decoded.color_space,
            ColorSpace::Linear,
            "BC1_UNorm is non-sRGB"
        );
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len(), 32);
        assert_eq!(decoded.layers[0], blocks, "raw block bytes must survive");
    }

    /// The sRGB variant of BC1 keeps the same BC1 layout but reports sRGB bytes.
    #[test]
    fn dds_bc1_srgb_reads_as_srgb() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bc1_srgb.dds");

        let (w, h) = (8u32, 8u32);
        let blocks: Vec<u8> = (0..32u8).collect();
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::BC1_UNorm_sRGB);
        write_dds_with_data(&path, dds, blocks.clone());

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: true,
                max_dim: 16384,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Bc(BcKind::Bc1));
        assert_eq!(decoded.color_space, ColorSpace::Srgb);
        assert_eq!(decoded.layers, vec![blocks]);
    }

    /// Without `allow_bc` the same BC1 DDS falls back to a CPU RGBA8 decode.
    #[test]
    fn dds_bc_disallowed_decodes_to_rgba8() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bc1_cpu.dds");

        let (w, h) = (8u32, 8u32);
        let blocks: Vec<u8> = (0..32u8).collect();
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::BC1_UNorm);
        write_dds_with_data(&path, dds, blocks);

        let decoded = load_image_opts(&path, DecodeOptions::cpu()).expect("load_image_opts ok");

        assert!(matches!(decoded.format, DecodedFormat::Rgba8));
        assert!(
            !matches!(decoded.format, DecodedFormat::Bc(_)),
            "CPU path must not return compressed bytes"
        );
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len() as u32, w * h * 4);
    }

    /// A BC image larger than `max_dim` cannot be kept compressed (chunking
    /// can't slice a block stream), so it falls back to the CPU decode.
    #[test]
    fn dds_bc_over_max_dim_falls_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bc1_big.dds");

        let (w, h) = (8u32, 8u32);
        let blocks: Vec<u8> = (0..32u8).collect();
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::BC1_UNorm);
        write_dds_with_data(&path, dds, blocks);

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: true,
                max_dim: 4,
            },
        )
        .expect("load_image_opts ok");

        assert!(matches!(decoded.format, DecodedFormat::Rgba8));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len() as u32, w * h * 4);
    }

    /// A legacy DXT1 FourCC DDS maps through `get_dxgi_format` to
    /// `BC1_UNorm_sRGB` (ddsfile's `try_from_pixel_format`), so it comes back as
    /// a BC1 layout tagged sRGB. 4x4 = a single 8-byte block.
    #[test]
    fn dds_legacy_dxt1_maps_to_bc1() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dxt1.dds");

        let (w, h) = (4u32, 4u32);
        let blocks: Vec<u8> = (0..8u8).collect();
        let dds = image_dds::ddsfile::Dds::new_d3d(image_dds::ddsfile::NewD3dParams {
            height: h,
            width: w,
            depth: None,
            format: image_dds::ddsfile::D3DFormat::DXT1,
            mipmap_levels: Some(1),
            caps2: None,
        })
        .expect("build dds");
        write_dds_with_data(&path, dds, blocks.clone());

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: true,
                max_dim: 16384,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Bc(BcKind::Bc1));
        assert_eq!(
            decoded.color_space,
            ColorSpace::Srgb,
            "ddsfile maps the DXT1 FourCC to BC1_UNorm_sRGB"
        );
        assert_eq!(decoded.layers, vec![blocks]);
    }

    /// BC5_UNorm (ATI2) is a two-channel 16-byte block layout; it maps to
    /// `BcKind::Bc5Unorm` with linear bytes. 8x8 = 2x2 blocks of 16 = 64 bytes.
    #[test]
    fn dds_bc5_unorm_maps_to_bc5_and_keeps_blocks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bc5.dds");

        let (w, h) = (8u32, 8u32);
        let blocks: Vec<u8> = (0..64u8).collect();
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::BC5_UNorm);
        write_dds_with_data(&path, dds, blocks.clone());

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: true,
                max_dim: 16384,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Bc(BcKind::Bc5Unorm));
        assert_eq!(decoded.color_space, ColorSpace::Linear);
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len(), 16 * 2 * 2);
        assert_eq!(decoded.layers[0], blocks);
    }

    #[test]
    fn dxgi_is_float_detects_float_and_bc6h_only() {
        use image_dds::ddsfile::DxgiFormat::*;
        assert!(dxgi_is_float(R32G32B32A32_Float));
        assert!(dxgi_is_float(R16G16B16A16_Float));
        assert!(dxgi_is_float(R32_Float));
        assert!(dxgi_is_float(BC6H_UF16));
        assert!(dxgi_is_float(BC6H_SF16));
        assert!(!dxgi_is_float(BC1_UNorm));
        assert!(!dxgi_is_float(BC7_UNorm_sRGB));
        assert!(!dxgi_is_float(R8G8B8A8_UNorm));
    }

    /// f32 -> little-endian f16 bytes, half precision preserved for the values
    /// HDR tonemapping cares about (including > 1.0).
    #[test]
    fn pack_f16_rgba_packs_little_endian_half_floats() {
        let packed = pack_f16_rgba(&[1.0, 0.5, 2.0, -1.0]);
        assert_eq!(packed.len(), 8, "4 components * 2 bytes");

        let decoded: Vec<f32> = packed
            .chunks_exact(2)
            .map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32())
            .collect();
        assert_eq!(decoded, vec![1.0, 0.5, 2.0, -1.0]);

        // Exact bit patterns, catching endianness flips.
        assert_eq!(&packed[0..2], &half::f16::from_f32(1.0).to_le_bytes());
        assert_eq!(&packed[4..6], &half::f16::from_f32(2.0).to_le_bytes());
    }

    /// Build an `R32G32B32A32_Float` DDS whose `data` is `w*h*4` little-endian
    /// f32 values cycling `pattern`, and write it to `path`.
    fn write_float32_dds(path: &std::path::Path, w: u32, h: u32, pattern: &[f32]) {
        let data: Vec<u8> = (0..(w * h * 4) as usize)
            .flat_map(|i| pattern[i % pattern.len()].to_le_bytes())
            .collect();
        assert_eq!(data.len(), (w * h * 4 * 4) as usize);
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::R32G32B32A32_Float);
        write_dds_with_data(path, dds, data);
    }

    /// An uncompressed float DDS decodes to `Float16` linear pixels: 8 bytes per
    /// texel, and the HDR values (> 1.0) survive the f32 -> f16 round-trip.
    #[test]
    fn dds_float32_decodes_to_float16() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hdr_f32.dds");

        let (w, h) = (8u32, 8u32);
        // Round values (exact in f16) including one above 1.0.
        let pattern = [2.0f32, 0.5, 4.0, 1.0];
        let floats: Vec<f32> = (0..(w * h * 4) as usize)
            .map(|i| pattern[i % pattern.len()])
            .collect();
        write_float32_dds(&path, w, h, &pattern);

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: false,
                max_dim: u32::MAX,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Float16);
        assert_eq!(
            decoded.color_space,
            ColorSpace::Linear,
            "float DDS is linear-light regardless of allow_bc"
        );
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len(), (w * h * 8) as usize, "8 bytes/texel");

        let got: Vec<f32> = decoded.layers[0]
            .chunks_exact(2)
            .map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32())
            .collect();
        assert_eq!(got, floats, "f16 round-trip must reproduce the f32 input");
    }

    /// A BC6H (HDR) DDS takes the float path: shape is `Float16`/linear with 8
    /// bytes per texel. Only the shape is asserted — the block bytes are opaque.
    #[test]
    fn dds_bc6h_hdr_decodes_to_float16() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bc6h.dds");

        let (w, h) = (4u32, 4u32);
        // One 16-byte BC6H block covers the whole 4x4 image.
        let block: Vec<u8> = (0..16u8).collect();
        let dds = new_dxgi_dds(w, h, image_dds::ddsfile::DxgiFormat::BC6H_UF16);
        write_dds_with_data(&path, dds, block);

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: false,
                max_dim: u32::MAX,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Float16);
        assert_eq!(decoded.color_space, ColorSpace::Linear);
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len(), (w * h * 8) as usize);
    }

    /// `max_dim`/`allow_bc` gate only the BC native path; a float DDS still
    /// returns `Float16` when both are set so as to exclude a compressed upload.
    #[test]
    fn dds_float_max_dim_does_not_affect_float_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hdr_f32_small_max.dds");

        let (w, h) = (8u32, 8u32);
        let pattern = [2.0f32, 0.5, 4.0, 1.0];
        write_float32_dds(&path, w, h, &pattern);

        let decoded = load_image_opts(
            &path,
            DecodeOptions {
                allow_bc: true,
                max_dim: 4,
            },
        )
        .expect("load_image_opts ok");

        assert_eq!(decoded.format, DecodedFormat::Float16);
        assert_eq!(decoded.color_space, ColorSpace::Linear);
        assert_eq!(decoded.layers.len(), 1);
        assert_eq!(decoded.layers[0].len(), (w * h * 8) as usize);
    }
}
