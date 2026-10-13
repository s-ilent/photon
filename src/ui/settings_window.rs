use crate::app;
use crate::commands::{self, Command};
use eframe::egui;

impl app::PhotonApp {
    pub(crate) fn ui_settings_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Settings")
            .open(&mut self.settings_open)
            .show(ctx, |ui| {
                ui.checkbox(
                    &mut self.settings.scale_to_fit,
                    "Scale to fit when switching images",
                );
                ui.horizontal(|ui| {
                    ui.label("Reset zoom:");
                    egui::ComboBox::from_id_salt("zoom_reset")
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
                ui.horizontal(|ui| {
                    ui.label("Fit mode:");
                    egui::ComboBox::from_id_salt("fit_mode")
                        .selected_text(self.settings.fit_mode.label())
                        .show_ui(ui, |ui| {
                            for mode in [
                                app::view::FitMode::DontFit,
                                app::view::FitMode::ReduceOnly,
                                app::view::FitMode::AlwaysFit,
                            ] {
                                ui.selectable_value(
                                    &mut self.settings.fit_mode,
                                    mode,
                                    mode.label(),
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Resize window to image:");
                    egui::ComboBox::from_id_salt("fit_window")
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
                // Lock zoom (Roadmap §3): when enabled, capture the current
                // magnification and hold it across navigation, overriding the
                // reset policy above.
                let mut lock_zoom = self.settings.lock_zoom;
                if ui
                    .checkbox(&mut lock_zoom, "Lock zoom across navigation")
                    .changed()
                {
                    // Field-level writes: the enclosing `.open(&mut self.settings_open)`
                    // borrow forbids calling a `&mut self` method here.
                    self.settings.lock_zoom = lock_zoom;
                    if lock_zoom {
                        self.zoom_lock.lock(self.zoom);
                    } else {
                        self.zoom_lock.unlock();
                    }
                }
                // HDR exposure (shader M4): applied to float sources before the
                // tonemap; ignored for non-HDR images.
                ui.horizontal(|ui| {
                    ui.label("HDR exposure (EV):");
                    ui.add(
                        egui::Slider::new(&mut self.settings.exposure, -6.0..=6.0)
                            .fixed_decimals(2),
                    );
                });
                // Sharp (pixel) scaling is a live toggle: the shader snaps the
                // sample point to the texel centre when magnified.
                ui.checkbox(
                    &mut self.settings.use_sharp_scaling,
                    "Use Sharp (Pixel) Scaling",
                );
                ui.checkbox(
                    &mut self.settings.checkerboard_bg,
                    "Checkerboard background",
                );
                ui.checkbox(
                    &mut self.settings.tile_repeat,
                    "Tile-repeat (seamless texture) view",
                );

                ui.separator();
                ui.label("Font Size");
                ui.add(egui::Slider::new(&mut self.settings.font_size, 10.0..=60.0));
                if self.settings.font_size != self.last_font_size {
                    self.last_font_size = self.settings.font_size;
                    app::PhotonApp::apply_theme(
                        ctx,
                        self.settings.dark_mode,
                        self.settings.font_size,
                    );
                }

                ui.separator();
                if ui.button("Cycle Theme").clicked() {
                    self.settings.dark_mode = !self.settings.dark_mode;
                    app::PhotonApp::apply_theme(
                        ctx,
                        self.settings.dark_mode,
                        self.settings.font_size,
                    );
                }

                ui.separator();
                ui.heading("Shortcuts");
                ui.label(
                    "Copy and Paste always answer Ctrl+C/V; rebinding adds or removes an extra key.",
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
                        self.settings.shortcut_overrides =
                            commands::binding_overrides(&self.bindings);
                        self.binding_capture = None;
                    }
                }

                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    egui::Grid::new("shortcuts").num_columns(4).show(ui, |ui| {
                        for &command in Command::all() {
                            ui.label(command.label());
                            if self.binding_capture == Some(command) {
                                ui.label("press a key… (Esc cancels)");
                            } else {
                                let binding = self
                                    .bindings
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
                });
            });
    }
}
