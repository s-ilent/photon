use crate::app::clipboard::{crop_rgba, current_rgba};
use crate::app::{PhotonApp, SaveStatus};
use eframe::egui;
use image::ExtendedColorType;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

/// How long a save success/failure message stays in the bottom bar.
const SAVE_STATUS_DURATION: Duration = Duration::from_secs(4);

/// One entry in the save-as dialog's format dropdown. The first entry is the
/// default selection. The `image` crate infers the on-disk format from the
/// file extension the OS appends based on the chosen filter.
///
/// Anything the `image` crate can't reliably encode from RGBA8 (gif, dds, exr)
/// is intentionally absent.
const SAVE_FILTERS: &[(&str, &[&str])] = &[
    ("PNG", &["png"]),
    ("JPEG", &["jpg", "jpeg"]),
    ("WebP", &["webp"]),
    ("BMP", &["bmp"]),
    ("TIFF", &["tiff", "tif"]),
];

impl PhotonApp {
    /// Save the full currently displayed image as a new file.
    /// Always uses a save-as dialog; never overwrites the original unless the
    /// user explicitly points at it. Encodes in a background thread so the UI
    /// doesn't stall on a large file write.
    pub(crate) fn save_image_as(&mut self) {
        let (rgba, width, height) = match current_rgba(self) {
            Some(v) => v,
            None => return,
        };

        let suggested = self.suggested_save_name();
        self.save_dialog_and_encode(rgba, width, height, suggested);
    }

    /// Save the current selection crop as a new file. A no-op (returns) when
    /// there is no active selection or no image to crop from.
    pub(crate) fn save_selection_as(&mut self) {
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

    /// The directory the save dialog opens in. Defaults to the displayed
    /// image's parent directory; falls back to the OS default for pasted
    /// images (which have no on-disk location).
    fn suggested_save_dir(&self) -> Option<PathBuf> {
        self.images
            .get(self.current_index)
            .and_then(|p| p.parent())
            .filter(|d| !d.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
    }

    fn save_dialog_and_encode(
        &mut self,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        suggested: String,
    ) {
        let mut dialog = rfd::FileDialog::new().set_file_name(suggested);

        // One dropdown entry per format so the user can pick by type.
        for (name, exts) in SAVE_FILTERS {
            dialog = dialog.add_filter(*name, *exts);
        }

        if let Some(dir) = self.suggested_save_dir() {
            dialog = dialog.set_directory(dir);
        }

        let Some(path) = dialog.save_file() else {
            return;
        };

        // Encoding/IO runs in a background thread so it doesn't freeze the
        // render loop. `image::save_buffer` infers the format from the file
        // extension the user typed (or the OS appended from the chosen filter).
        // The result is reported through `save_status_receiver` and surfaced
        // in the bottom bar; failures also keep the existing println.
        let (tx, rx) = channel();
        self.save_status_receiver = Some(rx);

        std::thread::spawn(move || {
            let result = match encode_to_path(&path, &rgba, width, height) {
                Ok(()) => Ok(
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string()),
                ),
                Err(e) => {
                    println!("Failed to save image to {:?}: {}", path, e);
                    Err(e)
                }
            };
            let _ = tx.send(result);
        });
    }

    /// Poll the background encoder for a finished save and start the
    /// transient status message. Also schedules the repaint that lets the
    /// message clear itself when it expires. Called once per frame.
    pub(crate) fn check_save_status(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = self.save_status_receiver.take() {
            if let Ok(result) = receiver.try_recv() {
                let (message, is_error) = match result {
                    Ok(name) => (format!("Saved {}", name), false),
                    Err(e) => (format!("Save failed: {}", e), true),
                };
                self.save_status = Some(SaveStatus {
                    message,
                    is_error,
                    expires: Instant::now() + SAVE_STATUS_DURATION,
                });
                ctx.request_repaint();
            } else {
                // Still encoding; put the receiver back for next frame.
                self.save_status_receiver = Some(receiver);
            }
        }

        if let Some(status) = &self.save_status {
            let remaining = status.expires.saturating_duration_since(Instant::now());
            if remaining == Duration::ZERO {
                self.save_status = None;
            } else {
                ctx.request_repaint_after(remaining);
            }
        }
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