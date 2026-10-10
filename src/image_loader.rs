use std::path::Path;

/// Detailed timing for image loading stages
pub struct LoadStats {}

/// Load an image and return as RGBA pixels with dimensions, plus timing stats.
///
/// `.dds` files are routed through `image_dds` (which wraps `ddsfile` for header
/// parsing and decodes BC1–BC7 + uncompressed variants). All other supported
/// formats go through the `image` crate as before.
pub fn load_image(path: &Path) -> Result<(Vec<u8>, u32, u32, LoadStats), String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    if extension.as_deref() == Some("dds") {
        return load_dds(path);
    }

    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let reader = std::io::BufReader::new(file);

    let img_reader = ::image::ImageReader::new(reader)
        .with_guessed_format()
        .map_err(|e| format!("Failed to guess format: {}", e))?;

    let img = img_reader
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;

    let (w, h) = (img.width(), img.height());
    let rgba = img.to_rgba8();

    Ok((rgba.into_raw(), w, h, LoadStats {}))
}

/// Decode a `.dds` file via `image_dds`. Covers DXT1–DXT5, BC4, BC5 (ATI2/BC5U),
/// BC7, and uncompressed variants — a strict superset of `image`'s built-in
/// DDS decoder, which rejects everything past DXT5 / the legacy FourCC set.
///
/// Multi-layer textures (cubemaps, texture arrays) decode as layer 0 only:
/// `image_from_dds` stacks every layer vertically (a 2048² 6-face cubemap
/// becomes 2048×12288), which is never the wanted view and overflows the
/// GPU's max texture dimension.
fn load_dds(path: &Path) -> Result<(Vec<u8>, u32, u32, LoadStats), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let dds = image_dds::ddsfile::Dds::read(file)
        .map_err(|e| format!("Failed to decode image: Failed to read DDS header: {}", e))?;

    let img = if dds_has_multiple_layers(&dds) {
        image_dds::SurfaceRgba8::decode_layers_mipmaps_dds(&dds, 0..1, 0..1)
            .map_err(|e| format!("Failed to decode image: DDS decode failed: {}", e))?
            .into_image()
            .map_err(|e| format!("Failed to decode image: DDS decode failed: {}", e))?
    } else {
        image_dds::image_from_dds(&dds, 0)
            .map_err(|e| format!("Failed to decode image: DDS decode failed: {}", e))?
    };

    let (w, h) = (img.width(), img.height());
    Ok((img.into_raw(), w, h, LoadStats {}))
}

/// True when the DDS holds more than one array layer: legacy cubemaps set
/// `caps2::CUBEMAP`, DXGI cubemaps set `misc_flag::TEXTURECUBE`, and texture
/// arrays report a layer count above one.
fn dds_has_multiple_layers(dds: &image_dds::ddsfile::Dds) -> bool {
    let dxgi_cube = dds
        .header10
        .as_ref()
        .is_some_and(|h| h.misc_flag == image_dds::ddsfile::MiscFlag::TEXTURECUBE);
    dxgi_cube
        || dds.header.caps2.contains(image_dds::ddsfile::Caps2::CUBEMAP)
        || dds.get_num_array_layers() > 1
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

        let (rgba, decoded_w, decoded_h, _) = load_image(&path).expect("load_image ok");
        assert_eq!((decoded_w, decoded_h), (w, h));
        assert_eq!(rgba, pixels);
        assert!(!rgba.is_empty());
    }

    /// Build a 6-face cubemap DDS in memory and verify `load_image` returns
    /// only face 0 (not the six faces stacked vertically).
    #[test]
    fn test_load_dds_cubemap_returns_single_face() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cubemap.dds");

        let (w, h) = (4u32, 4u32);
        let face: Vec<u8> = (0..(w * h * 4)).map(|i| (i % 251) as u8).collect();

        let mut dds = image_dds::ddsfile::Dds::new_d3d(image_dds::ddsfile::NewD3dParams {
            height: h,
            width: w,
            depth: None,
            format: image_dds::ddsfile::D3DFormat::A8B8G8R8,
            mipmap_levels: Some(1),
            caps2: Some(
                image_dds::ddsfile::Caps2::CUBEMAP
                    | image_dds::ddsfile::Caps2::CUBEMAP_ALLFACES,
            ),
        })
        .expect("build dds");
        dds.data = face.repeat(6);
        let mut f = std::fs::File::create(&path).expect("create file");
        dds.write(&mut f).expect("write dds");

        let (rgba, decoded_w, decoded_h, _) = load_image(&path).expect("load_image ok");
        assert_eq!((decoded_w, decoded_h), (w, h));
        assert_eq!(rgba, face);
    }
}