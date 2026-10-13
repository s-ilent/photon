use crate::app;
use eframe::egui;
use egui_phosphor::regular as icons;

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
                if sidebar_row(ui, &self.images[row], row == self.current_index, row_height) {
                    self.current_index = row;
                    self.request_current();
                }
            }
        });
    }
}

/// One sidebar entry: a full-width hit area with the filetype icon and file
/// name right-aligned, so a truncated long name still shows its tail. Returns
/// true when the entry was clicked.
fn sidebar_row(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    is_selected: bool,
    row_height: f32,
) -> bool {
    let response = ui
        .allocate_response(egui::vec2(ui.available_width(), row_height), egui::Sense::click())
        .on_hover_text(path.display().to_string());

    let fill = if is_selected {
        Some(app::theme::PURPLE)
    } else if response.hovered() {
        Some(app::theme::SURFACE_HOVER)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(response.rect, 0.0, fill);
    }

    // Dark text reads on the accent selection fill.
    let text_color = if is_selected {
        app::theme::BG
    } else {
        app::theme::FG
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let text = format!("{}  {name}", file_type_icon(path));

    let format = egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), text_color);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (response.rect.width() - 16.0).max(0.0);
    // One row, ellipsised: a long name loses its head, not its extension.
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('\u{2026}');
    job.append(&text, 0.0, format);
    let galley = ui.painter().layout_job(job);
    let pos = egui::pos2(
        response.rect.right() - 8.0 - galley.size().x,
        response.rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(pos, galley, egui::Color32::TRANSPARENT);

    response.clicked()
}

/// Phosphor's filetype glyphs where they exist, a generic file otherwise.
fn file_type_icon(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => icons::FILE_PNG,
        Some("jpg") | Some("jpeg") => icons::FILE_JPG,
        _ => icons::FILE,
    }
}
