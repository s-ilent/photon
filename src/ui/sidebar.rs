use crate::app;
use eframe::egui;

impl app::PhotonApp {
    pub(crate) fn ui_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Sort:");
            egui::ComboBox::from_id_salt("sort_mode")
                .selected_text(self.settings.sort_mode.label())
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    for mode in [
                        app::SortMode::Alphabetical,
                        app::SortMode::AlphabeticalDesc,
                        app::SortMode::DateModified,
                        app::SortMode::DateModifiedOldest,
                        app::SortMode::FileSize,
                        app::SortMode::FileSizeSmallest,
                    ] {
                        if ui
                            .selectable_value(&mut self.settings.sort_mode, mode, mode.label())
                            .clicked()
                        {
                            changed = true;
                        }
                    }
                    if changed {
                        self.sort_images(self.settings.sort_mode);
                    }
                });
        });

        ui.separator();

        let num_images = self.images.len();
        let row_height = 28.0;

        let mut scroll_area = egui::ScrollArea::vertical();
        if self.scroll_to_target {
            let view_height = ui.available_height();
            let target_top = self.current_index as f32 * row_height;
            let scroll_offset = target_top - (view_height / 2.0) + (row_height / 2.0);
            scroll_area = scroll_area.vertical_scroll_offset(scroll_offset.max(0.0));
            self.scroll_to_target = false;
        }

        // Force zero spacing for perfect predictability
        ui.spacing_mut().item_spacing.y = 0.0;

        scroll_area.show_rows(ui, row_height, num_images, |ui, row_range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in row_range {
                let path = &self.images[row];
                let name = path.file_name().unwrap_or_default().to_string_lossy();

                let is_selected = row == self.current_index;
                let text = if is_selected {
                    egui::RichText::new(name)
                        .strong()
                        .color(egui::Color32::from_rgb(189, 147, 249))
                } else {
                    egui::RichText::new(name)
                };

                let response = ui.add_sized(
                    [ui.available_width(), row_height],
                    egui::Button::new(text).selected(is_selected).truncate(),
                );

                if response.clicked() {
                    self.current_index = row;
                    self.request_current();
                }
            }
        });
    }
}
