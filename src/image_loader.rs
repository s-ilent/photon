use std::path::Path;

/// Detailed timing for image loading stages
pub struct LoadStats {}

/// Load an image and return as RGBA pixels with dimensions, plus timing stats
pub fn load_image(path: &Path) -> Result<(Vec<u8>, u32, u32, LoadStats), String> {
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
