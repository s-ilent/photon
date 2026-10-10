use crate::app::{PhotonApp, WorkerMessage};
use crate::image_loader;
use arboard::Clipboard;
use eframe::egui;

/// Crop an RGBA buffer to the given image-space rect, clamped to image bounds.
/// Returns `(cropped_rgba, width, height)`. If the (clamped) rect is empty or
/// the rect is non-positive, returns the full buffer unchanged.
///
/// Pure function; safe to unit-test.
pub(crate) fn crop_rgba(
    rgba: &[u8],
    width: usize,
    height: usize,
    rect: egui::Rect,
) -> (Vec<u8>, usize, usize) {
    let crop_x = rect.min.x.max(0.0) as usize;
    let crop_y = rect.min.y.max(0.0) as usize;
    let crop_w = rect.width() as usize;
    let crop_h = rect.height() as usize;

    let x = crop_x.min(width);
    let y = crop_y.min(height);
    let w = crop_w.min(width.saturating_sub(x));
    let h = crop_h.min(height.saturating_sub(y));

    if w == 0 || h == 0 {
        return (rgba.to_vec(), width, height);
    }

    let mut cropped = Vec::with_capacity(w * h * 4);
    for row in y..(y + h) {
        for col in x..(x + w) {
            let idx = (row * width + col) * 4;
            cropped.push(rgba[idx]);
            cropped.push(rgba[idx + 1]);
            cropped.push(rgba[idx + 2]);
            cropped.push(rgba[idx + 3]);
        }
    }
    (cropped, w, h)
}

/// The displayed layer of a decoded image: `layer` if present, else the last
/// one (defensive — layers always share dimensions so indices line up).
fn displayed_layer(layers: &[Vec<u8>], layer: usize) -> Option<&Vec<u8>> {
    layers.get(layer).or_else(|| layers.last())
}

/// Resolve the "current" RGBA bytes + dimensions for the displayed image —
/// specifically the displayed layer (cubemap face / array entry). Uses
/// `pasted_rgba` for pasted images, `active_rgba` for the loaded image the
/// worker already decoded, and only re-reads from disk as a fallback
/// (e.g. a cache hit that never went through the worker).
/// Returns `None` if there is nothing to save.
pub(crate) fn current_rgba(app: &PhotonApp) -> Option<(Vec<u8>, u32, u32)> {
    if app.is_pasted {
        let rgba = app.pasted_rgba.clone()?;
        let (w, h) = app.image_dimensions?;
        Some((rgba, w, h))
    } else {
        let path = app.images.get(app.current_index)?;

        if app.target_path.as_ref() == Some(path) {
            if let Some(layers) = &app.active_rgba {
                if let Some(rgba) = displayed_layer(layers, app.current_layer) {
                    let (w, h) = app.image_dimensions?;
                    return Some((rgba.clone(), w, h));
                }
            }
        }

        match image_loader::load_image(path) {
            Ok(decoded) => {
                let rgba = displayed_layer(&decoded.layers, app.current_layer)?.clone();
                Some((rgba, decoded.width, decoded.height))
            }
            Err(e) => {
                println!("Failed to load image: {}", e);
                None
            }
        }
    }
}

impl PhotonApp {
    pub(crate) fn copy_to_clipboard(&self) {
        let (rgba, width, height) = match current_rgba(self) {
            Some(v) => v,
            None => return,
        };
        let (img_width, img_height) = (width as usize, height as usize);

        // If there's a selection in image-space, crop to it
        let (final_rgba, final_width, final_height) = if let Some(sel) = &self.selection_image_rect
        {
            crop_rgba(&rgba, img_width, img_height, *sel)
        } else {
            (rgba, img_width, img_height)
        };

        // Copy to clipboard
        std::thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    println!("Clipboard error: {}", e);
                    return;
                }
            };

            let img_data = arboard::ImageData {
                width: final_width,
                height: final_height,
                bytes: final_rgba.into(),
            };

            if let Err(e) = clipboard.set_image(img_data) {
                println!("Failed to set clipboard: {}", e);
            }
        });
    }

    pub(crate) fn paste_from_clipboard(&mut self, ctx: &eframe::egui::Context) {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                println!("Clipboard error: {}", e);
                return;
            }
        };

        match clipboard.get_image() {
            Ok(img) => {
                let width = img.width as u32;
                let height = img.height as u32;
                let rgba = img.bytes.to_vec();

                if let Some(sender) = &self.request_sender {
                    self.is_pasted = true;
                    self.is_loading = true;
                    self.target_path = None; // Important so check_load_complete knows it's the paste
                    self.error_overlay = None;
                    let _ = sender.send(WorkerMessage::Paste {
                        rgba,
                        width,
                        height,
                    });
                    ctx.request_repaint();
                }
            }
            Err(e) => {
                println!("No image in clipboard or failed to get image: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn current_rgba_uses_displayed_layer() {
        let mut app = PhotonApp::new();
        app.images = vec![PathBuf::from("/cube.dds")];
        app.target_path = Some(PathBuf::from("/cube.dds"));
        app.active_rgba = Some(vec![vec![1, 1, 1, 1], vec![2, 2, 2, 2], vec![3, 3, 3, 3]]);
        app.image_dimensions = Some((1, 1));

        app.current_layer = 1;
        assert_eq!(current_rgba(&app).unwrap().0, vec![2, 2, 2, 2]);

        app.current_layer = 2;
        assert_eq!(current_rgba(&app).unwrap().0, vec![3, 3, 3, 3]);
    }

    #[test]
    fn test_crop_rgba_full_when_rect_empty() {
        // 2x2 image, all red
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let (out, w, h) = crop_rgba(&rgba, 2, 2, egui::Rect::NOTHING);
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(out, rgba);
    }

    #[test]
    fn test_crop_rgba_single_pixel() {
        // 2x2: top-left red, top-right green, bottom-left blue, bottom-right white
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, // (0,0) red
            0, 255, 0, 255, // (1,0) green
            0, 0, 255, 255, // (0,1) blue
            255, 255, 255, 255, // (1,1) white
        ];
        let rect = egui::Rect::from_min_size(egui::pos2(1.0, 0.0), egui::vec2(1.0, 1.0));
        let (out, w, h) = crop_rgba(&rgba, 2, 2, rect);
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(out, vec![0, 255, 0, 255]); // the green pixel
    }

    #[test]
    fn test_crop_rgba_clamps_to_bounds() {
        let rgba: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 255];
        // rect extends past the image; should clamp to width 1, height 1
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0));
        let (out, w, h) = crop_rgba(&rgba, 2, 1, rect);
        assert_eq!(w, 2);
        assert_eq!(h, 1);
        assert_eq!(out, rgba);
    }
}