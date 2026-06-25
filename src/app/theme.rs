use crate::app::PhotonApp;
use eframe::egui;

impl PhotonApp {
    pub(crate) fn setup_dracula_theme(ctx: &egui::Context, font_size: f32) {
        let mut visuals = egui::Visuals::dark();
        let bg_color = egui::Color32::from_rgb(40, 42, 54);
        let fg_color = egui::Color32::from_rgb(248, 248, 242);
        let current_line = egui::Color32::from_rgb(68, 71, 90);
        let purple = egui::Color32::from_rgb(189, 147, 249);

        visuals.widgets.noninteractive.bg_fill = bg_color;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, fg_color);
        visuals.widgets.inactive.bg_fill = current_line;
        visuals.widgets.hovered.bg_fill = purple;
        visuals.window_fill = bg_color;
        visuals.panel_fill = bg_color;

        ctx.set_visuals(visuals);

        let mut style = (*ctx.global_style()).clone();
        for text_style in [
            egui::TextStyle::Body,
            egui::TextStyle::Monospace,
            egui::TextStyle::Button,
            egui::TextStyle::Heading,
        ] {
            if let Some(font_id) = style.text_styles.get_mut(&text_style) {
                font_id.size = font_size;
            }
        }
        ctx.set_global_style(style);
    }

    pub(crate) fn load_custom_font(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "custom_font".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../font.ttf"
            ))),
        );

        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            vec.insert(0, "custom_font".to_owned());
        }

        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            vec.insert(0, "custom_font".to_owned());
        }

        ctx.set_fonts(fonts);
    }
}
