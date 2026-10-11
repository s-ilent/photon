use crate::app;
use eframe::egui;
use egui_phosphor::regular as icons;

impl app::PhotonApp {
    pub(crate) fn ui_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Far-right elements are laid out first (right-to-left)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(icons::GEAR).clicked() {
                self.settings_open = !self.settings_open;
            }
            ui.separator();
            if ui.button(format!("Mode: {}", self.mode.name())).clicked() {
                self.cycle_mode();
            }

            // Left-aligned elements fill the remaining space on the left (left-to-right)
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if ui
                    .button(format!("{} Open", icons::FOLDER_OPEN))
                    .clicked()
                {
                    self.open_file();
                }
                ui.separator();
                if ui.button(format!("{} Copy", icons::COPY)).clicked() {
                    self.copy_to_clipboard();
                }
                if ui.button(format!("{} Paste", icons::CLIPBOARD)).clicked() {
                    self.paste_from_clipboard(ctx);
                }
                if ui.button(format!("{} Save", icons::FLOPPY_DISK)).clicked() {
                    self.save_image_as();
                }
                if ui
                    .add_enabled(
                        self.mode == app::MouseMode::Select
                            && self.selection_image_rect.is_some(),
                        egui::Button::new(format!(
                            "{} Save Selection As\u{2026}",
                            icons::SCISSORS
                        )),
                    )
                    .clicked()
                {
                    self.save_selection_as();
                }
                ui.separator();
                let sidebar_btn = if self.sidebar_open {
                    format!("{} Sidebar", icons::CARET_LEFT)
                } else {
                    format!("{} Sidebar", icons::CARET_RIGHT)
                };
                if ui.button(sidebar_btn).clicked() {
                    self.sidebar_open = !self.sidebar_open;
                    if self.sidebar_open {
                        self.scroll_to_target = true;
                    }
                }
                ui.separator();
                if ui
                    .button(format!("{} Prev", icons::CARET_LEFT))
                    .clicked()
                {
                    self.prev_image();
                }
                if ui
                    .button(format!("{} Next", icons::CARET_RIGHT))
                    .clicked()
                {
                    self.next_image();
                }
            });
        });
    }
}
