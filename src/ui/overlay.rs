use crate::app::{theme, PhotonApp, StatusOverlay};
use eframe::egui;
use egui_phosphor::regular as icons;

impl PhotonApp {
    pub(crate) fn ui_status_overlay(&self, ctx: &egui::Context, state: StatusOverlay) {
        egui::Area::new(egui::Id::new("status_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            // Background order: above the canvas, below tool windows — the
            // overlay must never sit on top of an open window.
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| match state {
                    StatusOverlay::NoImages => {
                        ui.add(overlay_label("No images loaded.", theme::SUBDUED));
                    }
                    StatusOverlay::Loading(msg) => {
                        ui.add(egui::widgets::Spinner::new().size(32.0).color(theme::FG));
                        ui.add_space(8.0);
                        ui.add(overlay_label(&msg, theme::FG));
                    }
                    StatusOverlay::Error(err) => {
                        ui.label(egui::RichText::new(icons::X_CIRCLE).size(32.0));
                        ui.add_space(8.0);
                        ui.add(overlay_label(&format!("Error: {}", err), theme::RED));
                    }
                    StatusOverlay::UnsupportedFormat(filename) => {
                        ui.label(egui::RichText::new(icons::QUESTION).size(32.0));
                        ui.add_space(8.0);
                        ui.add(overlay_label(
                            &format!("Unsupported format: {}", filename),
                            theme::ORANGE,
                        ));
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

/// The large centred line under an overlay icon (or spinner).
fn overlay_label(text: &str, color: egui::Color32) -> egui::Label {
    egui::Label::new(egui::RichText::new(text).strong().size(18.0).color(color))
        .wrap_mode(egui::TextWrapMode::Extend)
}
