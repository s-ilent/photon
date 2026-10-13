//! The Preferences window (Roadmap §10): Sophia's master-detail schema — a
//! category column on the left, the selected page in a scrolling detail pane.
//! Settings apply live; the config writer persists them when they change.

use crate::app;
use crate::app::settings::Side;
use crate::commands::{self, Command};
use crate::ui::{form_cell, form_label, FORM_SPACING};
use eframe::egui;

const PAGES: [&str; 3] = ["General", "View", "Shortcuts"];

/// The detail pane carries one content column of this width: centred when the
/// pane is wider, clamped to the pane at narrow sizes. One column for every
/// page keeps the form (and the Shortcuts table) at the same x across pages.
const CONTENT_WIDTH: f32 = 460.0;

/// The label column: one width for every page, so the field column never
/// moves when the page changes. 180 pt holds the longest label ("Resize window
/// to image:", 171.6 pt at the default font).
const LABEL_WIDTH: f32 = 180.0;

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
            // Detail: the selected page on one centred content column, so a
            // page switch never moves the form. The pane fills the window
            // height (the shrink-to-content default collapses it to a sliver
            // while the grid inside is still measuring itself).
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let avail = ui.available_width();
                    let column = CONTENT_WIDTH.min(avail);
                    // The field column takes what the label column and the
                    // spacing leave. A slider fills it: the bar plus the value
                    // box after it (fixed at interact_size.x) equal the field.
                    let field = (column - LABEL_WIDTH - FORM_SPACING).max(0.0);
                    let value_box = ui.spacing().interact_size.x + ui.spacing().item_spacing.x;
                    ui.spacing_mut().slider_width =
                        (field - value_box).max(ui.spacing().interact_size.x);
                    // The scroll content inherits the master-detail row's
                    // horizontal layout; give the page a vertical one, and
                    // centre the column in it. The column must be a widget-like
                    // child (allocate_ui_with_layout): a `with_layout` scope
                    // anchors at the cursor's left, so Center would not move it.
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(column, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(column);
                                match self.preferences_page {
                                    0 => self.general_page(ui, &ctx, field),
                                    1 => self.view_page(ui, field),
                                    _ => self.shortcuts_page(ui, &ctx),
                                }
                            },
                        );
                    });
                });
        });
    }

    /// Theme, fonts, and the sidebar's placement.
    fn general_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, field: f32) {
        egui::Grid::new("prefs_general")
            .num_columns(2)
            .spacing([FORM_SPACING, 6.0])
            .show(ui, |ui| {
                form_label(ui, LABEL_WIDTH, "Theme:");
                form_field(ui, field, |ui| {
                    if ui.button("Cycle Theme").clicked() {
                        self.settings.dark_mode = !self.settings.dark_mode;
                        app::PhotonApp::apply_theme(
                            ctx,
                            self.settings.dark_mode,
                            self.settings.font_size,
                        );
                    }
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Font size:");
                form_field(ui, field, |ui| {
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
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Sidebar:");
                form_field(ui, field, |ui| {
                    egui::ComboBox::from_id_salt("sidebar_side")
                        .wrap_mode(egui::TextWrapMode::Truncate)
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
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Text alignment:");
                form_field(ui, field, |ui| {
                    egui::ComboBox::from_id_salt("sidebar_align")
                        .wrap_mode(egui::TextWrapMode::Truncate)
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
                });
                ui.end_row();
            });
    }

    /// How images meet the viewport.
    fn view_page(&mut self, ui: &mut egui::Ui, field: f32) {
        egui::Grid::new("prefs_view")
            .num_columns(2)
            .spacing([FORM_SPACING, 6.0])
            .show(ui, |ui| {
                form_label(ui, LABEL_WIDTH, "On open:");
                form_field(ui, field, |ui| {
                    ui.checkbox(
                        &mut self.settings.scale_to_fit,
                        "Scale to fit when switching images",
                    );
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Fit mode:");
                form_field(ui, field, |ui| {
                    egui::ComboBox::from_id_salt("fit_mode")
                        .wrap_mode(egui::TextWrapMode::Truncate)
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
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Resize window to image:");
                form_field(ui, field, |ui| {
                    egui::ComboBox::from_id_salt("fit_window")
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .selected_text(self.settings.fit_window.label())
                        .show_ui(ui, |ui| {
                            for mode in [
                                app::view::FitWindow::DontFit,
                                app::view::FitWindow::EnlargeIfSmaller,
                                app::view::FitWindow::AlwaysResize,
                            ] {
                                ui.selectable_value(
                                    &mut self.settings.fit_window,
                                    mode,
                                    mode.label(),
                                );
                            }
                        });
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Reset zoom:");
                form_field(ui, field, |ui| {
                    egui::ComboBox::from_id_salt("zoom_reset")
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .selected_text(self.settings.zoom_reset.label())
                        .show_ui(ui, |ui| {
                            for mode in [
                                app::ZoomReset::Always,
                                app::ZoomReset::DimensionChange,
                                app::ZoomReset::Never,
                            ] {
                                ui.selectable_value(
                                    &mut self.settings.zoom_reset,
                                    mode,
                                    mode.label(),
                                );
                            }
                        });
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Zoom:");
                // Lock zoom (Roadmap §3): when enabled, capture the current
                // magnification and hold it across navigation, overriding the
                // reset policy above.
                form_field(ui, field, |ui| {
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
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Scaling:");
                form_field(ui, field, |ui| {
                    ui.checkbox(
                        &mut self.settings.use_sharp_scaling,
                        "Sharp (pixel) scaling when magnified",
                    );
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Background:");
                form_field(ui, field, |ui| {
                    ui.checkbox(
                        &mut self.settings.checkerboard_bg,
                        "Checkerboard behind transparent images",
                    );
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "Repeat:");
                form_field(ui, field, |ui| {
                    ui.checkbox(&mut self.settings.tile_repeat, "Tile-repeat (seamless) view");
                });
                ui.end_row();

                form_label(ui, LABEL_WIDTH, "HDR exposure (EV):");
                // HDR exposure (shader M4): applied to float sources before
                // the tonemap; ignored for non-HDR images.
                form_field(ui, field, |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.settings.exposure, -6.0..=6.0)
                            .fixed_decimals(2),
                    );
                });
                ui.end_row();
            });
    }

    /// The per-command shortcut table with its capture flow.
    fn shortcuts_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Wrap explicitly: the hint must never widen the scroll content and
        // clip at the window edge. Left-aligned: it is a paragraph, so its
        // wrapped lines flush left even though the column itself is centred.
        ui.add(
            egui::Label::new(
                "Copy and Paste always answer Ctrl+C/V; rebinding adds or removes an extra key.",
            )
            .halign(egui::Align::Min)
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

/// One form row's field cell: the fixed field column, control left-aligned in
/// it. A long control label wraps at the column edge instead of widening the
/// grid past the pane at narrow sizes.
fn form_field(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    form_cell(
        ui,
        width,
        egui::Layout::left_to_right(egui::Align::Center),
        add,
    );
}

/// Render-check entry (see `ui::render_checks`), wrapped the same way the
/// native viewport wraps the body.
#[cfg(test)]
pub(crate) fn render_check_preferences_body(app: &mut app::PhotonApp, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        app.preferences_body(ui);
    });
}
