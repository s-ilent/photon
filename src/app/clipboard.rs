use crate::app::{PhotonApp, WorkerMessage};
use crate::image_loader;
use arboard::Clipboard;

impl PhotonApp {
    pub(crate) fn copy_to_clipboard(&self) {
        let (rgba, width, height) = if self.is_pasted {
            let Some(rgba) = &self.pasted_rgba else {
                return;
            };
            let Some((w, h)) = self.image_dimensions else {
                return;
            };
            (rgba.clone(), w, h)
        } else {
            let path = match self.images.get(self.current_index) {
                Some(p) => p,
                None => return,
            };

            match image_loader::load_image(path) {
                Ok(data) => (data.0, data.1, data.2),
                Err(e) => {
                    println!("Failed to load image: {}", e);
                    return;
                }
            }
        };

        let (img_width, img_height) = (width as usize, height as usize);

        // If there's a selection in image-space, crop to it
        let (final_rgba, final_width, final_height) = if let Some(sel) = &self.selection_image_rect
        {
            let crop_x = sel.min.x.max(0.0) as usize;
            let crop_y = sel.min.y.max(0.0) as usize;
            let crop_w = sel.width() as usize;
            let crop_h = sel.height() as usize;

            let x = crop_x.min(img_width);
            let y = crop_y.min(img_height);
            let w = crop_w.min(img_width.saturating_sub(x));
            let h = crop_h.min(img_height.saturating_sub(y));

            if w == 0 || h == 0 {
                (rgba, img_width, img_height)
            } else {
                let mut cropped = Vec::with_capacity(w * h * 4);
                for row in y..(y + h) {
                    for col in x..(x + w) {
                        let idx = (row * img_width + col) * 4;
                        cropped.push(rgba[idx]);
                        cropped.push(rgba[idx + 1]);
                        cropped.push(rgba[idx + 2]);
                        cropped.push(rgba[idx + 3]);
                    }
                }
                (cropped, w, h)
            }
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
                width: final_width as usize,
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
