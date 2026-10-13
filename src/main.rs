mod app;
mod commands;
mod dir_walker;
mod image_loader;
mod input;
mod render;
mod ui;
use crate::app::PhotonApp;
use eframe::egui;
use eframe::egui_wgpu::WgpuConfiguration;
use std::sync::Arc;

impl eframe::App for app::PhotonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Owned Context (an Arc internally): panel closures below get `&mut ui`
        // via show_inside, so a borrow of `ui` must not be held across them.
        let ctx = ui.ctx().clone();
        // 0. Initial Setup (First Frame)
        static STARTUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        if STARTUP.swap(false, std::sync::atomic::Ordering::SeqCst) {
            app::PhotonApp::apply_theme(&ctx, self.settings.dark_mode, self.settings.font_size);
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

        // Shortcut / Interaction Handling: scan the command registry's key
        // bindings, then dispatch each through the single command spine. While
        // the Settings rebinding UI is capturing a key, it owns the input.
        if self.binding_capture.is_none() {
            for command in input::take_commands(&ctx, &self.bindings) {
                self.run_command(command, &ctx);
            }
        }

        // Drag-and-drop: open a dropped folder, or a dropped file together
        // with its parent folder (same as the Open dialog).
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.iter().find_map(|f| f.path.clone()) {
            self.open_path(path);
        }

        // 0. Settings Window
        if self.settings_open {
            self.ui_settings_window(&ctx);
        } else {
            // A key capture must not survive the Settings window closing.
            self.binding_capture = None;
        }

        // 0. Image Information Window
        if self.info_open {
            self.ui_info_window(&ctx);
        }

        // Check for completed background loads
        self.check_load_complete(&ctx);
        self.check_dir_load_complete();
        self.check_dir_watch();
        self.check_save_status(&ctx);
        self.check_image_info();
        self.check_cli_launch(&ctx);

        // Persist settings when they changed this frame
        self.save_config_if_changed();

        // 1. Top Menu Bar (registry-driven)
        egui::Panel::top("menubar").show_inside(ui, |ui| self.ui_menu_bar(ui, &ctx));

        // 1. Top Toolbar (App State & Settings)
        egui::Panel::top("toolbar").show_inside(ui, |ui| self.ui_toolbar(ui, &ctx));

        // 2. Bottom Info Bar (Image Data)
        egui::Panel::bottom("info").show_inside(ui, |ui| self.ui_info_bar(ui));

        if self.sidebar_open {
            egui::Panel::left("sidebar")
                .resizable(true)
                .min_size(200.0)
                .show_inside(ui, |ui| self.ui_sidebar(ui));
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

    // Command line (Roadmap §10): parse before the GUI opens so a bad
    // invocation exits with a message instead of an empty window.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let launch = match app::cli::parse_args(&args) {
        Ok(app::cli::Parsed::Help) => {
            println!("{}", app::cli::usage());
            return Ok(());
        }
        Ok(app::cli::Parsed::Launch(l)) => l,
        Err(e) => {
            eprintln!("photon: {e} (try --help)");
            std::process::exit(2);
        }
    };

    eframe::run_native(
        "Photon",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1024.0, 768.0])
                .with_min_inner_size([640.0, 480.0])
                .with_resizable(true)
                .with_decorations(true),
            wgpu_options: WgpuConfiguration {
                present_mode: wgpu::PresentMode::Mailbox,
                // Raise the texture-size ceiling where the adapter supports
                // it (most desktop GPUs do 16384; the worker's oversized
                // guard reads the live limit, so it adapts automatically).
                wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                    device_descriptor: Arc::new(|adapter| wgpu::DeviceDescriptor {
                        // Block-compressed (BC1–BC7) DDS upload needs this; request
                        // it only where the adapter supports it (the loader falls
                        // back to a CPU decode otherwise).
                        required_features: adapter.features()
                            & wgpu::Features::TEXTURE_COMPRESSION_BC,
                        required_limits: wgpu::Limits {
                            max_texture_dimension_2d: 16384
                                .min(adapter.limits().max_texture_dimension_2d),
                            ..wgpu::Limits::default()
                        },
                        ..Default::default()
                    }),
                    ..egui_wgpu::WgpuSetupCreateNew::without_display_handle()
                }),
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

                // Image canvas pipeline: built against the surface's format
                // and shared with callbacks (and, via the layouts, the
                // loader worker).
                let renderer_state = Arc::new(render::ImageRendererState::new(
                    &wgpu_state.device,
                    wgpu_state.target_format,
                ));
                render::ImageRendererState::insert_into(
                    &mut wgpu_state.renderer.write().callback_resources,
                    renderer_state.clone(),
                );
                app.image_renderer = Some(renderer_state);
            }

            app.load_config();
            app.bindings = commands::effective_bindings(&app.settings.shortcut_overrides);
            app.init_worker();

            // Apply the command-line launch: one path opens with its folder,
            // several file paths form a playlist in argument order (Roadmap
            // §10). The op chain and --save target are consumed per frame
            // once the first image is displayed (cli::check_cli_launch).
            let app::cli::CliLaunch {
                paths,
                ops,
                save_to,
                open_settings,
            } = launch;
            if paths.len() > 1 {
                app.load_playlist(paths);
            } else if let Some(p) = paths.into_iter().next() {
                app.target_path = Some(p.clone());
                app.load_directory(&p);
            }
            if open_settings {
                app.settings_open = true;
            }
            app.cli_ops = (!ops.is_empty()).then_some(ops);
            app.cli_save_to = save_to;
            Ok(Box::new(app))
        }),
    )
}
