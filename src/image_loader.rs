use std::path::Path;

/// Upper bound on decoded layers per file. Texture arrays can be arbitrarily
/// deep; decoding/uploading every layer of a huge array would exhaust memory.
/// Cubemaps (6 faces) are nowhere near this.
const MAX_DECODED_LAYERS: u32 = 64;

/// Decoded pixels of an image file: one RGBA8 buffer per layer (cubemap face
/// or texture-array entry), all sharing `width`/`height`. Ordinary images
/// have exactly one layer.
pub struct DecodedImage {
    pub layers: Vec<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// True when the layers are cubemap faces (drives face labels in the UI).
    pub is_cubemap: bool,
}

/// Load an image and return its layers as RGBA pixels with dimensions.
///
/// `.dds` files are routed through `image_dds` (which wraps `ddsfile` for
/// header parsing and decodes BC1–BC7 + uncompressed variants). All other
/// supported formats go through the `image` crate as before and produce a
/// single layer.
pub fn load_image(path: &Path) -> Result<DecodedImage, String> {
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

    Ok(DecodedImage {
        layers: vec![rgba.into_raw()],
        width: w,
        height: h,
        is_cubemap: false,
    })
}

/// Decode a `.dds` file via `image_dds`. Covers DXT1–DXT5, BC4, BC5 (ATI2/BC5U),
/// BC7, and uncompressed variants — a strict superset of `image`'s built-in
/// DDS decoder, which rejects everything past DXT5 / the legacy FourCC set.
///
/// Every layer (cubemap face or array entry) is decoded; the layers are
/// stacked vertically inside the file by `image_dds`, so we split the stack
/// back into per-face buffers here.
fn load_dds(path: &Path) -> Result<DecodedImage, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let dds = image_dds::ddsfile::Dds::read(file)
        .map_err(|e| format!("Failed to decode image: Failed to read DDS header: {}", e))?;

    let layer_count = dds_layer_count(&dds).min(MAX_DECODED_LAYERS);
    let img = image_dds::SurfaceRgba8::decode_layers_mipmaps_dds(&dds, 0..layer_count, 0..1)
        .map_err(|e| format!("Failed to decode image: DDS decode failed: {}", e))?
        .into_image()
        .map_err(|e| format!("Failed to decode image: DDS decode failed: {}", e))?;

    // The decoded image stacks all layers vertically, so its reported height
    // is count × face-height; the true per-layer dimensions are in the header.
    let (w, h) = (dds.get_width(), dds.get_height());
    let layer_bytes = (w as usize) * (h as usize) * 4;
    let raw = img.into_raw();
    if layer_bytes == 0 || raw.len() % layer_bytes != 0 {
        return Err(format!(
            "Failed to decode image: DDS decode produced {} bytes, not a multiple of {}x{}x4",
            raw.len(),
            w,
            h
        ));
    }
    let layers = raw.chunks_exact(layer_bytes).map(<[u8]>::to_vec).collect();

    Ok(DecodedImage {
        layers,
        width: w,
        height: h,
        is_cubemap: dds_is_cubemap(&dds),
    })
}

/// True for legacy (`caps2`) and DXGI (`misc_flag`) cubemaps.
fn dds_is_cubemap(dds: &image_dds::ddsfile::Dds) -> bool {
    dds.header.caps2.contains(image_dds::ddsfile::Caps2::CUBEMAP)
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
                image_dds::ddsfile::Caps2::CUBEMAP
                    | image_dds::ddsfile::Caps2::CUBEMAP_ALLFACES,
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
}