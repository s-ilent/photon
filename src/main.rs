mod app;
mod dir_walker;
mod image_loader;
use crate::app::PhotonApp;
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

impl eframe::App for app::PhotonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx();
        // 0. Initial Setup (First Frame)
        static STARTUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        if STARTUP.swap(false, std::sync::atomic::Ordering::SeqCst) {
            app::PhotonApp::setup_dracula_theme(ctx, self.settings.font_size);
            app::PhotonApp::load_custom_font(ctx);
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
                        println!("DEBUG: Key Pressed {:?} (Modifiers: {:?})", key, modifiers);
                    }
                }
            }
        });

        // Shortcut / Interaction Handling
        let mut do_open = false;
        let mut do_save = false;
        let mut do_copy = false;
        let mut do_paste = false;
        let mut is_mode_switch = false;
        let mut do_next = false;
        let mut do_prev = false;

        ctx.input_mut(|i| {
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::O,
            )) {
                do_open = true;
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            )) {
                do_save = true;
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::C,
            )) {
                do_copy = true;
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::V,
            )) {
                do_paste = true;
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Backslash,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Tab,
            )) {
                is_mode_switch = true;
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Space,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::ArrowRight,
            )) {
                do_next = true;
            }
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Backspace,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::ArrowLeft,
            )) {
                do_prev = true;
            }
        });

        // Execute Actions
        if do_open {
            self.open_file();
        }
        if do_save {
            println!("DEBUG: Save triggered (Not implemented)");
        }
        if do_copy {
            println!("DEBUG: Stateful Copy Triggered");
            self.copy_to_clipboard();
        }
        if do_paste {
            println!("DEBUG: Stateful Paste Triggered");
            self.paste_from_clipboard(ctx);
        }
        if is_mode_switch {
            self.cycle_mode();
        }
        if do_next {
            self.next_image();
        }
        if do_prev {
            self.prev_image();
        }

        // 0. Settings Window
        if self.settings_open {
            egui::Window::new("Settings")
                .open(&mut self.settings_open)
                .show(ctx, |ui| {
                    ui.checkbox(
                        &mut self.settings.scale_to_fit,
                        "Scale to fit when switching images",
                    );
                    ui.checkbox(
                        &mut self.settings.keep_zoom,
                        "Keep zoom level when switching images",
                    );
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
                            app::PhotonApp::setup_dracula_theme(ctx, self.settings.font_size);
                        }
                    }

                    ui.separator();
                    if ui.button("Cycle Theme").clicked() {
                        self.settings.dark_mode = !self.settings.dark_mode;
                        if self.settings.dark_mode {
                            app::PhotonApp::setup_dracula_theme(ctx, self.settings.font_size);
                        } else {
                            ctx.set_visuals(egui::Visuals::light());
                        }
                    }
                });
        }

        // Check for completed background loads
        self.check_load_complete(ctx);
        self.check_dir_load_complete();

        // 1. Top Toolbar (App State & Settings)
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            // Far-right elements are laid out first (right-to-left)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙").clicked() {
                    self.settings_open = !self.settings_open;
                }
                ui.separator();
                if ui.button(format!("Mode: {}", self.mode.name())).clicked() {
                    self.cycle_mode();
                }

                // Left-aligned elements fill the remaining space on the left (left-to-right)
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if ui.button("📁 Open").clicked() {
                        self.open_file();
                    }
                    ui.separator();
                    if ui.button("📋 Copy").clicked() {
                        self.copy_to_clipboard();
                    }
                    if ui.button("📥 Paste").clicked() {
                        self.paste_from_clipboard(ctx);
                    }
                    ui.separator();
                    let sidebar_btn = if self.sidebar_open {
                        "◀ Sidebar"
                    } else {
                        "▶ Sidebar"
                    };
                    if ui.button(sidebar_btn).clicked() {
                        self.sidebar_open = !self.sidebar_open;
                        if self.sidebar_open {
                            self.scroll_to_target = true;
                        }
                    }
                    ui.separator();
                    if ui.button("Prev").clicked() {
                        self.prev_image();
                    }
                    if ui.button("Next").clicked() {
                        self.next_image();
                    }
                });
            });
        });

        // 2. Bottom Info Bar (Image Data)
        egui::TopBottomPanel::bottom("info").show(ctx, |ui| {
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

                if self.mode == app::MouseMode::Select {
                    if let Some(sel) = &self.selection_image_rect {
                        ui.separator();
                        ui.label(format!("Selection: {:.0}x{:.0}", sel.width(), sel.height()));
                    }
                }
            });
        });

        if self.sidebar_open {
            egui::SidePanel::left("sidebar")
                .resizable(true)
                .min_width(200.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Sort:");
                        egui::ComboBox::from_id_source("sort_mode")
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
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.is_loading_dir {
                self.ui_status_overlay(
                    ctx,
                    app::StatusOverlay::Loading("Loading Directory...".to_string()),
                );
            } else {
                self.draw_image_workspace(ui);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    #[cfg(debug_assertions)]
    println!("WARNING: Running in DEBUG mode. Image decoding will be slow!");
    #[cfg(not(debug_assertions))]
    println!("Running in RELEASE mode.");

    let path = std::env::args().nth(1).map(PathBuf::from);

    eframe::run_native(
        "Photon",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1024.0, 768.0])
                .with_resizable(true)
                .with_decorations(true),
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

            app.init_worker();

            if let Some(ref p) = path {
                app.load_directory(p);
            }
            Ok(Box::new(app))
        }),
    )
}
