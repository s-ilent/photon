pub(crate) mod batch_window;
pub(crate) mod info_bar;
pub(crate) mod info_window;
pub(crate) mod overlay;
#[cfg(test)]
mod render_checks;
pub(crate) mod settings_window;
pub(crate) mod sidebar;
pub(crate) mod toolbar;

use eframe::egui;

/// Horizontal spacing between a form's label and field columns; shared by
/// every form so their field columns land on the same x for a given label
/// width.
pub(crate) const FORM_SPACING: f32 = 12.0;

/// A form cell of a fixed width: the content is laid out inside a child of
/// exactly `width`, which is the rect the Grid measures as the column width.
/// `set_min_width` on the cell itself is not enough — the Grid sizes a column
/// from the placed widget, and a right-aligned label only places its own text.
pub(crate) fn form_cell(
    ui: &mut egui::Ui,
    width: f32,
    layout: egui::Layout,
    add: impl FnOnce(&mut egui::Ui),
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        layout,
        |ui| {
            ui.set_min_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            add(ui);
        },
    );
}

/// A right-aligned form label with its colon (UI-DESIGN §4), on a fixed label
/// column of `width` so every form's field column starts at the same x.
pub(crate) fn form_label(ui: &mut egui::Ui, width: f32, text: &str) {
    form_cell(ui, width, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(text);
    });
}
