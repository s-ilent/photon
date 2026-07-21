use crate::app::clipboard::{crop_rgba, current_rgba};
use crate::app::PhotonApp;
use image::ExtendedColorType;
use std::path::PathBuf;

/// Save buffers are always written as RGBA8; the `image` crate infers the
/// on-disk format from the file extension the user picks in the save dialog.
/// These are the only extensions offered; anything the `image` crate can't
/// reliably encode from RGBA8 (gif, dds, exr) is intentionally absent.
const SAVE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif"];

impl PhotonApp {
    /// Save the full currently displayed image as a new file.
    /// Always uses a save-as dialog; never overwrites the original unless the
    /// user explicitly points at it. Encodes in a background thread so the UI
    /// doesn't stall on a large file write.
    pub(crate) fn save_image_as(&self) {
        let (rgba, width, height) = match current_rgba(self) {
            Some(v) => v,
            None => return,
        };

        let suggested = self.suggested_save_name();
        self.save_dialog_and_encode(rgba, width, height, suggested);
    }

    /// Save the current selection crop as a new file. A no-op (returns) when
    /// there is no active selection or no image to crop from.
    pub(crate) fn save_selection_as(&self) {
        let Some(selection_image_rect) = self.selection_image_rect else {
            return;
        };

        let (rgba, width, height) = match current_rgba(self) {
            Some(v) => v,
            None => return,
        };
        let (cropped, crop_w, crop_h) =
            crop_rgba(&rgba, width as usize, height as usize, selection_image_rect);

        if cropped.is_empty() || crop_w == 0 || crop_h == 0 {
            return;
        }

        let suggested = self.suggested_save_name();
        self.save_dialog_and_encode(cropped, crop_w as u32, crop_h as u32, suggested);
    }

    /// A reasonable default filename for the save dialog: the displayed
    /// image's file stem (or "pasted" if the image came from the clipboard).
    fn suggested_save_name(&self) -> String {
        if self.is_pasted {
            return "pasted".to_string();
        }
        self.images
            .get(self.current_index)
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string())
    }

    fn save_dialog_and_encode(
        &self,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        suggested: String,
    ) {
        let path = rfd::FileDialog::new()
            .add_filter("Images", SAVE_EXTENSIONS)
            .set_file_name(suggested)
            .save_file();

        let Some(path) = path else {
            return;
        };

        // Encoding/IO runs in a background thread so it doesn't freeze the
        // render loop. On failure we follow the existing convention: print to
        // stdout. A "surface save errors in the UI" cleanup is tracked in Todo.
        std::thread::spawn(move || {
            if let Err(e) = encode_to_path(&path, &rgba, width, height) {
                println!("Failed to save image to {:?}: {}", path, e);
            }
        });
    }
}

/// Encode RGBA8 bytes to a file at `path`. The format is chosen by the file
/// extension. For JPEG (which has no alpha channel) the alpha is dropped and
/// the buffer is re-packed as RGB8; every other supported format keeps alpha.
fn encode_to_path(path: &PathBuf, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // JPEG can't represent alpha; flatten RGBA → RGB (alpha discarded).
    if ext == "jpg" || ext == "jpeg" {
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        for chunk in rgba.chunks_exact(4) {
            rgb.push(chunk[0]);
            rgb.push(chunk[1]);
            rgb.push(chunk[2]);
        }
        image::save_buffer(path, &rgb, width, height, ExtendedColorType::Rgb8)
    } else {
        image::save_buffer(path, rgba, width, height, ExtendedColorType::Rgba8)
    }
    .map_err(|e| format!("Failed to encode: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_to_path_png_writes_file() {
        // 2x2 solid-red RGBA
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let dir = std::env::temp_dir();
        let path = dir.join("photon_saver_test.png");
        encode_to_path(&path, &rgba, 2, 2).expect("png encode should succeed");
        let loaded = image::open(&path).expect("round-trip load should succeed");
        assert_eq!(loaded.width(), 2);
        assert_eq!(loaded.height(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_encode_to_path_jpeg_drops_alpha() {
        // 1x1 RGBA with alpha 128; jpeg should still encode (alpha discarded)
        let rgba: Vec<u8> = vec![255, 0, 0, 128];
        let dir = std::env::temp_dir();
        let path = dir.join("photon_saver_test_jpeg.jpg");
        encode_to_path(&path, &rgba, 1, 1).expect("jpeg encode should succeed");
        let loaded = image::open(&path).expect("round-trip load should succeed");
        assert_eq!(loaded.width(), 1);
        assert_eq!(loaded.height(), 1);
        let _ = std::fs::remove_file(&path);
    }
}