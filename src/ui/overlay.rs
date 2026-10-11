use crate::app::{PhotonApp, StatusOverlay};
use eframe::egui;
use egui_phosphor::regular as icons;

impl PhotonApp {
    pub(crate) fn ui_status_overlay(&self, ctx: &egui::Context, state: StatusOverlay) {
        egui::Area::new(egui::Id::new("status_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    match state {
                        StatusOverlay::NoImages => {
                            // Subdued grey text for empty state
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("No images loaded.")
                                        .strong()
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(139, 143, 163)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        StatusOverlay::Loading(msg) => {
                            // Spinner + bright Dracula purple/pink text
                            ui.add(egui::widgets::Spinner::new().size(32.0));
                            ui.add_space(10.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(msg)
                                        .strong()
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(248, 248, 242)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        StatusOverlay::Error(err) => {
                            // Dracula Red Alert Cross + Error Text
                            ui.label(egui::RichText::new(icons::X_CIRCLE).size(32.0));
                            ui.add_space(8.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("Error: {}", err))
                                        .strong()
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(255, 85, 85)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        StatusOverlay::UnsupportedFormat(filename) => {
                            // Dracula Orange Alert Question Mark + Warning Text
                            ui.label(egui::RichText::new(icons::QUESTION).size(32.0));
                            ui.add_space(8.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "Unsupported format: {}",
                                        filename
                                    ))
                                    .strong()
                                    .size(18.0)
                                    .color(egui::Color32::from_rgb(255, 184, 108)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                    }
                });
            });
    }

    pub(crate) fn loading_text(&self) -> String {
        if let Some(p) = &self.target_path {
            format!(
                "Loading {}...",
                p.file_name().unwrap_or_default().to_string_lossy()
            )
        } else {
            "Loading...".to_string()
        }
    }
}
