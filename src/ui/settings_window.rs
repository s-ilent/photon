use crate::app;
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
            });
    }
}
