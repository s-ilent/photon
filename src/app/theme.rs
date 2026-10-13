use crate::app::PhotonApp;
use eframe::egui;

// Dracula palette (https://draculatheme.com/contribute). Shared with the UI
// modules so accents stay one source of truth.
pub(crate) const BG: egui::Color32 = egui::Color32::from_rgb(40, 42, 54);
pub(crate) const CURRENT_LINE: egui::Color32 = egui::Color32::from_rgb(68, 71, 90);
/// Dracula surface hover (the button-hover fill the theme applies).
pub(crate) const SURFACE_HOVER: egui::Color32 = egui::Color32::from_rgb(98, 114, 164);
pub(crate) const COMMENT: egui::Color32 = egui::Color32::from_rgb(98, 114, 164);
pub(crate) const FG: egui::Color32 = egui::Color32::from_rgb(248, 248, 242);
pub(crate) const PURPLE: egui::Color32 = egui::Color32::from_rgb(189, 147, 249);
pub(crate) const RED: egui::Color32 = egui::Color32::from_rgb(255, 85, 85);
pub(crate) const ORANGE: egui::Color32 = egui::Color32::from_rgb(255, 184, 108);
pub(crate) const SUBDUED: egui::Color32 = egui::Color32::from_rgb(139, 143, 163);

impl PhotonApp {
    /// Apply the theme for `dark_mode`, so every call site (startup, font-size
    /// change, theme toggle) takes the same path.
    pub(crate) fn apply_theme(ctx: &egui::Context, dark_mode: bool, font_size: f32) {
        if dark_mode {
            Self::setup_dracula_theme(ctx, font_size);
        } else {
            Self::setup_light_theme(ctx, font_size);
        }
    }

    pub(crate) fn setup_dracula_theme(ctx: &egui::Context, font_size: f32) {
        let mut visuals = egui::Visuals::dark();

        visuals.panel_fill = BG;
        visuals.window_fill = BG;
        visuals.extreme_bg_color = egui::Color32::from_rgb(30, 31, 41);
        visuals.hyperlink_color = PURPLE;
        visuals.selection.bg_fill = PURPLE;
        // Dark text over the light purple selection fill.
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, BG);

        // Buttons and checkboxes read `weak_bg_fill`/`bg_fill` per state, so
        // both must be set: a Dracula surface at rest, a raised hover, and the
        // accent when pressed.
        visuals.widgets.noninteractive.bg_fill = BG;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, FG);

        visuals.widgets.inactive.weak_bg_fill = CURRENT_LINE;
        visuals.widgets.inactive.bg_fill = CURRENT_LINE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, FG);

        visuals.widgets.hovered.weak_bg_fill = COMMENT;
        visuals.widgets.hovered.bg_fill = COMMENT;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5_f32, FG);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, FG);

        visuals.widgets.active.weak_bg_fill = PURPLE;
        visuals.widgets.active.bg_fill = PURPLE;
        // Dark text over the purple pressed fill. egui derives
        // `strong_text_color()` from this colour, so `ui.strong` text and an
        // uncoloured `Spinner` would paint in BG; both set a colour explicitly
        // (see `ui::batch_window::header_label`, `ui::overlay`).
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, BG);

        // Open menu/combo background.
        visuals.widgets.open.weak_bg_fill = COMMENT;
        visuals.widgets.open.bg_fill = COMMENT;
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, FG);

        ctx.set_visuals(visuals);
        apply_text_style(ctx, font_size);
    }

    /// egui default light visuals, with the same no-selectable-text rule and
    /// font size as the dark theme.
    pub(crate) fn setup_light_theme(ctx: &egui::Context, font_size: f32) {
        ctx.set_visuals(egui::Visuals::light());
        apply_text_style(ctx, font_size);
    }

    pub(crate) fn load_custom_font(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        // Phosphor lands at family slot 1 (icon glyphs); the custom font
        // takes slot 0 below, UI text fonts after that.
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

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

/// Apply the shared text style: no selectable text anywhere (Photon is a
/// viewer — egui's selectable labels engage a text-selection cursor with its
/// own scroll behavior), and the user's font size on every text style.
fn apply_text_style(ctx: &egui::Context, font_size: f32) {
    let mut style = (*ctx.global_style()).clone();
    style.interaction.selectable_labels = false;
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
