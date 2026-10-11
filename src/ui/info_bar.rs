use crate::app;
use eframe::egui;

impl app::PhotonApp {
    pub(crate) fn ui_info_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.is_pasted {
                ui.label("Pasted Image");
            } else if !self.images.is_empty() {
                ui.label(format!("{}/{}", self.current_index + 1, self.images.len()));
                ui.separator();
                if let Some(path) = self.images.get(self.current_index) {
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                }
            }

            ui.separator();
            if let Some((w, h)) = self.image_dimensions {
                ui.label(format!("{}x{}", w, h));
                ui.separator();
            }
            ui.label(format!(
                "Zoom: {}",
                app::view::zoom_percent_label(self.zoom)
            ));

            if let Some(status) = &self.save_status {
                if std::time::Instant::now() < status.expires {
                    ui.separator();
                    ui.label(&status.message);
                }
            }

            if self.mode == app::MouseMode::Select {
                if let Some(sel) = &self.selection_image_rect {
                    ui.separator();
                    ui.label(format!("Selection: {:.0}x{:.0}", sel.width(), sel.height()));
                }
            }

            // Cubemap face / array layer selector
            let layer_count = self
                .displayed_image
                .as_ref()
                .map_or(0, |image| image.layers.len());
            if layer_count > 1 {
                ui.separator();
                ui.label(if self.layers_are_cubemap { "Face:" } else { "Layer:" });
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for i in 0..layer_count {
                            let label = self.layer_label(i);
                            if ui
                                .add(egui::Button::new(label).selected(i == self.current_layer))
                                .clicked()
                            {
                                self.select_layer(i);
                            }
                        }
                    });
                });
            }
        });
    }
}
