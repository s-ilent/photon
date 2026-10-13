use crate::app;
use crate::commands::{Command, MenuGroup};
use eframe::egui;
use egui_phosphor::regular as icons;

impl app::PhotonApp {
    /// Top menu bar (File / Edit / Image / View), driven by the command
    /// registry: the groups come from [`MenuGroup::ALL`], the items and their
    /// enable state from [`Command::all`], so a new command joins a menu just
    /// by declaring its [`MenuGroup`] (Roadmap §1).
    pub(crate) fn ui_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            for group in MenuGroup::ALL {
                ui.menu_button(group.title(), |ui| {
                    for &command in Command::all() {
                        if command.menu_group() != group {
                            continue;
                        }
                        if command.starts_menu_section() {
                            ui.separator();
                        }
                        if ui
                            .add_enabled(
                                self.command_enabled(command),
                                egui::Button::new(command.label()),
                            )
                            .clicked()
                        {
                            ui.close();
                            self.run_command(command, ctx);
                        }
                    }
                    // Lock-zoom and the window-fit/zoom actions are runtime
                    // state, not registry commands, so they are appended to the
                    // View menu directly.
                    if group == MenuGroup::View {
                        ui.separator();
                        let locked = self.zoom_lock.is_locked();
                        if ui.selectable_label(locked, "Lock Zoom").clicked() {
                            ui.close();
                            self.set_zoom_lock(!locked);
                        }
                        ui.separator();
                        let has_image = self.image_dimensions.is_some();
                        let can_window_fit =
                            has_image && self.settings.fit_window != app::view::FitWindow::DontFit;
                        if ui
                            .add_enabled(can_window_fit, egui::Button::new("Fit Window to Image"))
                            .clicked()
                        {
                            ui.close();
                            self.fit_window_to_image(ctx);
                        }
                        if ui
                            .add_enabled(has_image, egui::Button::new("Centre Window"))
                            .clicked()
                        {
                            ui.close();
                            self.center_window(ctx);
                        }
                        if ui
                            .add_enabled(has_image, egui::Button::new("Zoom to Desktop"))
                            .clicked()
                        {
                            ui.close();
                            self.zoom_to_desktop(ctx);
                        }
                    }
                });
            }
        });
    }

    pub(crate) fn ui_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Drain any finished background batch run every frame, regardless of
        // whether the Batch Convert window is currently open.
        self.check_batch_status(ctx);

        // Far-right elements are laid out first (right-to-left)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(icons::GEAR).on_hover_text("Settings").clicked() {
                self.settings_open = !self.settings_open;
            }
            if ui
                .button(icons::INFO)
                .on_hover_text("Image Information (Ctrl+I)")
                .clicked()
            {
                self.info_open = !self.info_open;
            }
            ui.separator();
            if ui
                .button(format!("{} {}", icons::MOUSE_SIMPLE, self.mode.name()))
                .on_hover_text("Mouse mode — click to cycle (Tab)")
                .clicked()
            {
                self.cycle_mode();
            }

            // Left-aligned elements fill the remaining space on the left (left-to-right)
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if ui.button(format!("{} Open", icons::FOLDER_OPEN)).clicked() {
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
                    .button(format!("{} Convert\u{2026}", icons::ARROWS_CLOCKWISE))
                    .on_hover_text("Batch convert files")
                    .clicked()
                {
                    self.batch.open = true;
                }
                if ui
                    .add_enabled(
                        self.mode == app::MouseMode::Select && self.selection_image_rect.is_some(),
                        egui::Button::new(format!("{} Save Selection As\u{2026}", icons::SCISSORS)),
                    )
                    .clicked()
                {
                    self.save_selection_as();
                }
                ui.separator();
                let sidebar_btn = format!("{} Sidebar", icons::SIDEBAR_SIMPLE);
                if ui
                    .button(sidebar_btn)
                    .on_hover_text("Toggle the file list")
                    .clicked()
                {
                    self.sidebar_open = !self.sidebar_open;
                    if self.sidebar_open {
                        self.scroll_to_target = true;
                    }
                }
                ui.separator();
                if ui
                    .button(format!("{} Prev", icons::CARET_LEFT))
                    .on_hover_text("Previous image (Left arrow)")
                    .clicked()
                {
                    self.prev_image();
                }
                if ui
                    .button(format!("{} Next", icons::CARET_RIGHT))
                    .on_hover_text("Next image (Right arrow)")
                    .clicked()
                {
                    self.next_image();
                }
                ui.separator();
                if ui
                    .button(format!("{} Fit", icons::ARROWS_OUT_SIMPLE))
                    .on_hover_text("Fit to window")
                    .clicked()
                {
                    self.needs_fit = true;
                }
                if ui.button("100%").on_hover_text("Actual size").clicked() {
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                }
            });
        });

        // The Batch Convert window is an `egui::Window`; drive it from here so
        // the toolbar is its single entry point.
        if self.batch.open {
            self.ui_batch_window(ctx);
        }
    }
}
