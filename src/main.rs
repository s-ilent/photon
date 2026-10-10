mod app;
mod dir_walker;
mod image_loader;
use crate::app::PhotonApp;
use eframe::egui;
use eframe::egui_wgpu::WgpuConfiguration;
use egui_phosphor::regular as icons;
use std::path::PathBuf;
use std::sync::Arc;

impl eframe::App for app::PhotonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Owned Context (an Arc internally): panel closures below get `&mut ui`
        // via show_inside, so a borrow of `ui` must not be held across them.
        let ctx = ui.ctx().clone();
        // 0. Initial Setup (First Frame)
        static STARTUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        if STARTUP.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if self.settings.dark_mode {
                app::PhotonApp::setup_dracula_theme(&ctx, self.settings.font_size);
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }
            app::PhotonApp::load_custom_font(&ctx);
        }

        // 0. Terminal Trace Logging
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = event
                {
                    if *pressed {
                        log::debug!("Key Pressed {:?} (Modifiers: {:?})", key, modifiers);
                    }
                }
            }
        });

        // Shortcut / Interaction Handling
        let actions = take_shortcut_actions(&ctx);

        // Execute Actions
        if actions.open {
            self.open_file();
        }
        if actions.save {
            log::debug!("Save triggered");
            self.save_image_as();
        }
        if actions.copy {
            log::debug!("Stateful Copy Triggered");
            self.copy_to_clipboard();
        }
        if actions.paste {
            log::debug!("Stateful Paste Triggered");
            self.paste_from_clipboard(&ctx);
        }
        if actions.mode_switch {
            self.cycle_mode();
        }
        if actions.next {
            self.next_image();
        }
        if actions.prev {
            self.prev_image();
        }

        // 0. Settings Window
        if self.settings_open {
            egui::Window::new("Settings")
                .open(&mut self.settings_open)
                .show(&ctx, |ui| {
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
                    ui.checkbox(
                        &mut self.settings.use_sharp_scaling,
                        "Use Sharp (Pixel) Scaling",
                    );
                    ui.checkbox(
                        &mut self.settings.checkerboard_bg,
                        "Checkerboard background",
                    );

                    ui.separator();
                    ui.label("Font Size");
                    ui.add(egui::Slider::new(&mut self.settings.font_size, 10.0..=60.0));
                    if self.settings.font_size != self.last_font_size {
                        self.last_font_size = self.settings.font_size;
                        if self.settings.dark_mode {
                            app::PhotonApp::setup_dracula_theme(&ctx, self.settings.font_size);
                        }
                    }

                    ui.separator();
                    if ui.button("Cycle Theme").clicked() {
                        self.settings.dark_mode = !self.settings.dark_mode;
                        if self.settings.dark_mode {
                            app::PhotonApp::setup_dracula_theme(&ctx, self.settings.font_size);
                        } else {
                            ctx.set_visuals(egui::Visuals::light());
                        }
                    }
                });
        }

        // Check for completed background loads
        self.check_load_complete(&ctx);
        self.check_dir_load_complete();
        self.check_save_status(&ctx);

        // Persist settings when they changed this frame
        self.save_config_if_changed();

        // 1. Top Toolbar (App State & Settings)
        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            // Far-right elements are laid out first (right-to-left)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(icons::GEAR).clicked() {
                    self.settings_open = !self.settings_open;
                }
                ui.separator();
                if ui.button(format!("Mode: {}", self.mode.name())).clicked() {
                    self.cycle_mode();
                }

                // Left-aligned elements fill the remaining space on the left (left-to-right)
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if ui
                        .button(format!("{} Open", icons::FOLDER_OPEN))
                        .clicked()
                    {
                        self.open_file();
                    }
                    ui.separator();
                    if ui.button(format!("{} Copy", icons::COPY)).clicked() {
                        self.copy_to_clipboard();
                    }
                    if ui.button(format!("{} Paste", icons::CLIPBOARD)).clicked() {
                        self.paste_from_clipboard(&ctx);
                    }
                    if ui.button(format!("{} Save", icons::FLOPPY_DISK)).clicked() {
                        self.save_image_as();
                    }
                    if ui
                        .add_enabled(
                            self.mode == app::MouseMode::Select
                                && self.selection_image_rect.is_some(),
                            egui::Button::new(format!(
                                "{} Save Selection As\u{2026}",
                                icons::SCISSORS
                            )),
                        )
                        .clicked()
                    {
                        self.save_selection_as();
                    }
                    ui.separator();
                    let sidebar_btn = if self.sidebar_open {
                        format!("{} Sidebar", icons::CARET_LEFT)
                    } else {
                        format!("{} Sidebar", icons::CARET_RIGHT)
                    };
                    if ui.button(sidebar_btn).clicked() {
                        self.sidebar_open = !self.sidebar_open;
                        if self.sidebar_open {
                            self.scroll_to_target = true;
                        }
                    }
                    ui.separator();
                    if ui
                        .button(format!("{} Prev", icons::CARET_LEFT))
                        .clicked()
                    {
                        self.prev_image();
                    }
                    if ui
                        .button(format!("{} Next", icons::CARET_RIGHT))
                        .clicked()
                    {
                        self.next_image();
                    }
                });
            });
        });

        // 2. Bottom Info Bar (Image Data)
        egui::Panel::bottom("info").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if self.is_pasted {
                    ui.label("Pasted Image");
                } else if !self.images.is_empty() {
                    ui.label(format!("{}/{}", self.current_index + 1, self.images.len()));
                    ui.separator();
                    if let Some(path) = self.images.get(self.current_index) {
                        ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    }
                }

                ui.separator();
                if let Some((w, h)) = self.image_dimensions {
                    ui.label(format!("{}x{}", w, h));
                    ui.separator();
                }
                ui.label(format!("Zoom: {:.0}%", self.zoom * 100.0));

                if let Some(status) = &self.save_status {
                    if std::time::Instant::now() < status.expires {
                        ui.separator();
                        ui.label(&status.message);
                    }
                }

                if self.mode == app::MouseMode::Select {
                    if let Some(sel) = &self.selection_image_rect {
                        ui.separator();
                        ui.label(format!("Selection: {:.0}x{:.0}", sel.width(), sel.height()));
                    }
                }

                // Cubemap face / array layer selector
                if self.layer_texture_ids.len() > 1 {
                    ui.separator();
                    ui.label(if self.layers_are_cubemap { "Face:" } else { "Layer:" });
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for i in 0..self.layer_texture_ids.len() {
                                let label = self.layer_label(i);
                                if ui
                                    .add(
                                        egui::Button::new(label)
                                            .selected(i == self.current_layer),
                                    )
                                    .clicked()
                                {
                                    self.select_layer(i);
                                }
                            }
                        });
                    });
                }
            });
        });

        if self.sidebar_open {
            egui::Panel::left("sidebar")
                .resizable(true)
                .min_size(200.0)
                .show_inside(ui, |ui| {
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
                                        .selectable_value(
                                            &mut self.settings.sort_mode,
                                            mode,
                                            mode.label(),
                                        )
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
                });
        }

        // 3. Central Workspace
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.is_loading_dir {
                self.ui_status_overlay(
                    &ctx,
                    app::StatusOverlay::Loading("Loading Directory...".to_string()),
                );
            } else {
                self.draw_image_workspace(ui);
            }
        });
    }
}

/// One frame's worth of shortcut actions, scanned off the egui input state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ShortcutActions {
    open: bool,
    save: bool,
    copy: bool,
    paste: bool,
    mode_switch: bool,
    next: bool,
    prev: bool,
}

/// Scan this frame's key presses and clipboard commands.
///
/// GOTCHA: egui-winit rewrites Ctrl+C / Ctrl+V / Ctrl+X at the input layer
/// into `Event::Copy` / `Event::Paste` / `Event::Cut` (its TextEdit clipboard
/// path) — on winit platforms the corresponding `Event::Key`s never arrive,
/// so copy/paste cannot be matched with `consume_shortcut` alone. We listen
/// for the command events AND keep the key-shortcut checks as a fallback for
/// backends that don't rewrite; on any one platform only one path can fire.
/// App-level handling deliberately takes these events before any widget:
/// Photon has no TextEdits today, but if one is ever added, it will not see
/// Ctrl+C/V and needs its own handling.
fn take_shortcut_actions(ctx: &egui::Context) -> ShortcutActions {
    let mut actions = ShortcutActions::default();

    ctx.input_mut(|i| {
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::O,
        )) {
            actions.open = true;
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::S,
        )) {
            actions.save = true;
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::C,
        )) {
            actions.copy = true;
        }

        // The real winit path: egui-winit turned Ctrl+C/V into command events.
        // Consume them so no widget sees them later in the pass.
        if i
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy))
        {
            actions.copy = true;
        }
        if i
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Paste(_)))
        {
            actions.paste = true;
        }
        i.events
            .retain(|event| !matches!(event, egui::Event::Copy | egui::Event::Paste(_)));

        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::V,
        )) {
            actions.paste = true;
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Backslash,
        )) || i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Tab,
        )) {
            actions.mode_switch = true;
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Space,
        )) || i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::ArrowRight,
        )) {
            actions.next = true;
        }
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::Backspace,
        )) || i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::ArrowLeft,
        )) {
            actions.prev = true;
        }
    });

    actions
}


fn main() -> eframe::Result<()> {
    // Log to the terminal. Default: photon at info (loader timings, load
    // failures), everything else at warn — dependency crates like zbus/wgpu
    // are chatty at info. RUST_LOG overrides the whole filter.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("photon=info,warn"))
        .init();

    #[cfg(debug_assertions)]
    log::warn!("Running in DEBUG mode. Image decoding will be slow!");
    #[cfg(not(debug_assertions))]
    log::info!("Running in RELEASE mode.");

    let path = std::env::args().nth(1).map(PathBuf::from);

    eframe::run_native(
        "Photon",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1024.0, 768.0])
                .with_resizable(true)
                .with_decorations(true),
            wgpu_options: WgpuConfiguration {
                present_mode: wgpu::PresentMode::Mailbox,
                ..Default::default()
            },
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = PhotonApp::new();

            // Initialize wgpu state from CreationContext
            if let Some(wgpu_state) = cc.wgpu_render_state.as_ref() {
                app.device = Some(wgpu_state.device.clone());
                app.queue = Some(Arc::new(wgpu_state.queue.clone()));
                app.render_state = Some(Arc::new(wgpu_state.clone()));
            }

            app.load_config();
            app.init_worker();

            if let Some(ref p) = path {
                app.target_path = Some(p.clone());
                app.load_directory(p);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_events(events: Vec<egui::Event>) -> ShortcutActions {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        ctx.begin_pass(raw);
        let actions = take_shortcut_actions(&ctx);
        let _ = ctx.end_pass();
        actions
    }

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// The real winit path: egui-winit rewrites Ctrl+C/V into command events.
    #[test]
    fn clipboard_command_events_trigger_copy_and_paste() {
        let actions = with_events(vec![egui::Event::Copy]);
        assert!(actions.copy);
        assert!(!actions.paste);

        let actions = with_events(vec![egui::Event::Paste("pixels".into())]);
        assert!(actions.paste);
        assert!(!actions.copy);
    }

    /// Fallback for backends that deliver the raw key instead of rewriting it.
    #[test]
    fn ctrl_c_and_ctrl_v_key_events_trigger_copy_and_paste() {
        let actions = with_events(vec![key_event(egui::Key::C, egui::Modifiers::COMMAND)]);
        assert!(actions.copy);

        let actions = with_events(vec![key_event(egui::Key::V, egui::Modifiers::COMMAND)]);
        assert!(actions.paste);
    }

    #[test]
    fn navigation_and_mode_shortcuts_still_work() {
        let actions = with_events(vec![key_event(egui::Key::Space, egui::Modifiers::NONE)]);
        assert!(actions.next);

        let actions = with_events(vec![key_event(egui::Key::ArrowLeft, egui::Modifiers::NONE)]);
        assert!(actions.prev);

        let actions = with_events(vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert!(actions.mode_switch);
    }

    #[test]
    fn plain_key_press_without_shortcut_is_ignored() {
        let actions = with_events(vec![key_event(egui::Key::C, egui::Modifiers::NONE)]);
        assert!(!actions.copy);
        assert!(!actions.open);
        assert!(!actions.save);
    }
}
