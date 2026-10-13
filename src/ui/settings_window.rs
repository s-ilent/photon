//! The Preferences window (Roadmap §10): Sophia's master-detail schema — a
//! category column on the left, the selected page in a scrolling detail pane.
//! Settings apply live; the config writer persists them when they change.

use crate::app;
use crate::app::settings::Side;
use crate::commands::{self, Command};
use eframe::egui;

const PAGES: [&str; 3] = ["General", "View", "Shortcuts"];

impl app::PhotonApp {
    /// Render the Preferences surface while it is open: a native OS child
    /// window (viewport), like the Batch Convert surface — its title bar,
    /// resizing and close button belong to the window manager. Called from
    /// `main` each frame, after the panels, so the live-applied settings are
    /// observed as complete edits.
    pub(crate) fn show_settings_viewport(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Preferences")
            .with_inner_size([560.0, 430.0])
            .with_min_inner_size([480.0, 320.0])
            .with_active(true);
        if ctx.embed_viewports() {
            builder.min_inner_size = None;
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("preferences"),
            builder,
            |ui, class| {
                if class == egui::ViewportClass::Immediate {
                    if ui.input(|i| i.viewport().close_requested()) {
                        self.settings_open = false;
                        self.binding_capture = None;
                        return;
                    }
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        self.preferences_body(ui);
                    });
                } else {
                    // Embedded fallback: the surface is a child Ui of the host.
                    self.preferences_body(ui);
                }
            },
        );
    }

    /// The master-detail body: category column on the left, the selected
    /// page in the detail pane.
    fn preferences_body(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.preferences_page = self.preferences_page.min(PAGES.len() - 1);
        // A plain `horizontal` reserves one interact-row of height; the
        // master-detail row needs the full window height.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            // Master: the category column.
            ui.allocate_ui(egui::vec2(120.0, ui.available_height()), |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    for (index, label) in PAGES.iter().enumerate() {
                        if ui
                            .selectable_label(self.preferences_page == index, *label)
                            .clicked()
                        {
                            self.preferences_page = index;
                        }
                    }
                });
            });
            ui.separator();
            // Detail: the selected page. Fills the window height (the
            // shrink-to-content default collapses the pane to a sliver while
            // the grid inside is still measuring itself).
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // The scroll content inherits the row's horizontal
                    // layout; the pages are vertical forms.
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        match self.preferences_page {
                            0 => self.general_page(ui, &ctx),
                            1 => self.view_page(ui),
                            _ => self.shortcuts_page(ui, &ctx),
                        }
                    });
                });
        });
    }

    /// Theme, fonts, and the sidebar's placement.
    fn general_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Grid::new("prefs_general")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                grid_label(ui, "Theme:");
                if ui.button("Cycle Theme").clicked() {
                    self.settings.dark_mode = !self.settings.dark_mode;
                    app::PhotonApp::apply_theme(
                        ctx,
                        self.settings.dark_mode,
                        self.settings.font_size,
                    );
                }
                ui.end_row();

                grid_label(ui, "Font size:");
                let font_size =
                    ui.add(egui::Slider::new(&mut self.settings.font_size, 10.0..=60.0));
                if font_size.changed() && self.settings.font_size != self.last_font_size {
                    self.last_font_size = self.settings.font_size;
                    app::PhotonApp::apply_theme(
                        ctx,
                        self.settings.dark_mode,
                        self.settings.font_size,
                    );
                }
                ui.end_row();

                grid_label(ui, "Sidebar:");
                egui::ComboBox::from_id_salt("sidebar_side")
                    .selected_text(self.settings.sidebar_side.label())
                    .show_ui(ui, |ui| {
                        for side in [Side::Left, Side::Right] {
                            ui.selectable_value(
                                &mut self.settings.sidebar_side,
                                side,
                                side.label(),
                            );
                        }
                    });
                ui.end_row();

                grid_label(ui, "Text alignment:");
                egui::ComboBox::from_id_salt("sidebar_align")
                    .selected_text(self.settings.sidebar_align.label())
                    .show_ui(ui, |ui| {
                        for side in [Side::Left, Side::Right] {
                            ui.selectable_value(
                                &mut self.settings.sidebar_align,
                                side,
                                side.label(),
                            );
                        }
                    });
                ui.end_row();
            });
    }

    /// How images meet the viewport.
    fn view_page(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("prefs_view")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                grid_label(ui, "On open:");
                ui.checkbox(
                    &mut self.settings.scale_to_fit,
                    "Scale to fit when switching images",
                );
                ui.end_row();

                grid_label(ui, "Fit mode:");
                egui::ComboBox::from_id_salt("fit_mode")
                    .selected_text(self.settings.fit_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            app::view::FitMode::DontFit,
                            app::view::FitMode::ReduceOnly,
                            app::view::FitMode::AlwaysFit,
                        ] {
                            ui.selectable_value(&mut self.settings.fit_mode, mode, mode.label());
                        }
                    });
                ui.end_row();

                grid_label(ui, "Resize window to image:");
                egui::ComboBox::from_id_salt("fit_window")
                    .selected_text(self.settings.fit_window.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            app::view::FitWindow::DontFit,
                            app::view::FitWindow::EnlargeIfSmaller,
                            app::view::FitWindow::AlwaysResize,
                        ] {
                            ui.selectable_value(&mut self.settings.fit_window, mode, mode.label());
                        }
                    });
                ui.end_row();

                grid_label(ui, "Reset zoom:");
                egui::ComboBox::from_id_salt("zoom_reset")
                    .selected_text(self.settings.zoom_reset.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            app::ZoomReset::Always,
                            app::ZoomReset::DimensionChange,
                            app::ZoomReset::Never,
                        ] {
                            ui.selectable_value(&mut self.settings.zoom_reset, mode, mode.label());
                        }
                    });
                ui.end_row();

                grid_label(ui, "Zoom:");
                // Lock zoom (Roadmap §3): when enabled, capture the current
                // magnification and hold it across navigation, overriding the
                // reset policy above.
                let mut lock_zoom = self.settings.lock_zoom;
                if ui
                    .checkbox(&mut lock_zoom, "Lock across navigation")
                    .changed()
                {
                    self.settings.lock_zoom = lock_zoom;
                    if lock_zoom {
                        self.zoom_lock.lock(self.zoom);
                    } else {
                        self.zoom_lock.unlock();
                    }
                }
                ui.end_row();

                grid_label(ui, "Scaling:");
                ui.checkbox(
                    &mut self.settings.use_sharp_scaling,
                    "Sharp (pixel) scaling when magnified",
                );
                ui.end_row();

                grid_label(ui, "Background:");
                ui.checkbox(
                    &mut self.settings.checkerboard_bg,
                    "Checkerboard behind transparent images",
                );
                ui.end_row();

                grid_label(ui, "Repeat:");
                ui.checkbox(
                    &mut self.settings.tile_repeat,
                    "Tile-repeat (seamless) view",
                );
                ui.end_row();

                grid_label(ui, "HDR exposure (EV):");
                // HDR exposure (shader M4): applied to float sources before
                // the tonemap; ignored for non-HDR images.
                ui.add(
                    egui::Slider::new(&mut self.settings.exposure, -6.0..=6.0).fixed_decimals(2),
                );
                ui.end_row();
            });
    }

    /// The per-command shortcut table with its capture flow.
    fn shortcuts_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Wrap explicitly: the hint must never widen the scroll content and
        // clip at the window edge.
        ui.add(
            egui::Label::new(
                "Copy and Paste always answer Ctrl+C/V; rebinding adds or removes an extra key.",
            )
            .wrap(),
        );

        // While a capture is armed, it owns the input: main.rs pauses
        // command dispatch until the capture resolves. egui-winit
        // rewrites Ctrl+C/X/V into clipboard command events, so those
        // keys are only observable as Copy/Cut/Paste here.
        if let Some(capture) = self.binding_capture {
            let mut captured = None;
            let mut cancelled = false;
            ctx.input(|i| {
                for event in &i.events {
                    match *event {
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            ..
                        } => {
                            cancelled = true;
                            break;
                        }
                        egui::Event::Key {
                            key,
                            modifiers,
                            pressed: true,
                            repeat: false,
                            ..
                        } => {
                            captured = Some(egui::KeyboardShortcut {
                                modifiers,
                                logical_key: key,
                            });
                            break;
                        }
                        egui::Event::Copy => {
                            captured = Some(egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::C,
                            ));
                            break;
                        }
                        egui::Event::Cut => {
                            captured = Some(egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::X,
                            ));
                            break;
                        }
                        egui::Event::Paste(_) => {
                            captured = Some(egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::V,
                            ));
                            break;
                        }
                        _ => {}
                    }
                }
            });
            if cancelled {
                self.binding_capture = None;
            } else if let Some(shortcut) = captured {
                commands::apply_binding_change(&mut self.bindings, capture, Some(shortcut));
                self.settings.shortcut_overrides = commands::binding_overrides(&self.bindings);
                self.binding_capture = None;
            }
        }

        egui::Grid::new("shortcuts").num_columns(4).show(ui, |ui| {
            for &command in Command::all() {
                ui.label(command.label());
                if self.binding_capture == Some(command) {
                    ui.label("press a key… (Esc cancels)");
                } else {
                    let binding =
                        self.bindings
                            .iter()
                            .find_map(|(c, s)| if *c == command { *s } else { None });
                    ui.label(
                        binding
                            .map(commands::shortcut_to_text)
                            .unwrap_or_else(|| "unbound".into()),
                    );
                    if ui.small_button("Change").clicked() {
                        self.binding_capture = Some(command);
                    }
                    if ui.small_button("Clear").clicked() {
                        commands::apply_binding_change(&mut self.bindings, command, None);
                        self.settings.shortcut_overrides =
                            commands::binding_overrides(&self.bindings);
                    }
                }
                ui.end_row();
            }
        });
        if ui.button("Reset All").clicked() {
            self.bindings = commands::effective_bindings(&[]);
            self.settings.shortcut_overrides = Vec::new();
        }
    }
}

/// A right-aligned form label with its colon (UI-DESIGN §4).
fn grid_label(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(text);
    });
}

/// Render-check entry (see `ui::render_checks`), wrapped the same way the
/// native viewport wraps the body.
#[cfg(test)]
pub(crate) fn render_check_preferences_body(app: &mut app::PhotonApp, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        app.preferences_body(ui);
    });
}
