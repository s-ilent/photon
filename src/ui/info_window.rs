//! Image Information window (Roadmap.md §9): file, image, and metadata rows
//! gathered by the headers-only probe in `app/image_info.rs`. Mirrors the
//! Settings window's drive pattern (`main` opens it each frame; a background
//! thread probes so the UI never blocks on disk).

use crate::app::image_info;
use crate::app::PhotonApp;
use eframe::egui;
use std::sync::mpsc::{channel, TryRecvError};

impl PhotonApp {
    /// Show the Image Information window when open. Called from `main` each
    /// frame (mirroring `ui_settings_window`).
    pub(crate) fn ui_info_window(&mut self, ctx: &egui::Context) {
        // Local copy of the open flag: the body borrows `self` immutably, which
        // would otherwise conflict with `Window::open`.
        let mut open = self.info_open;
        let mut refresh = false;
        egui::Window::new("Image Information")
            .default_width(420.0)
            .open(&mut open)
            .show(ctx, |ui| refresh = info_window_body(ui, self));
        self.info_open = open;
        if refresh {
            self.request_image_info();
        }
    }

    /// Re-probe the displayed target on a background thread. The latest probe
    /// wins; safe to call repeatedly.
    pub(crate) fn request_image_info(&mut self) {
        let Some(path) = self.target_path.clone() else {
            return;
        };
        let probe_path = path.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(image_info::probe(&probe_path));
        });
        self.info_probe = Some((path, rx));
    }

    /// Poll the in-flight probe and re-probe automatically when the displayed
    /// target changes while the window is open. Called every frame (mirrors
    /// `check_save_status`).
    pub(crate) fn check_image_info(&mut self) {
        let stale = self.target_path.is_some()
            && self.info_probe.is_none()
            && self
                .image_info
                .as_ref()
                .is_none_or(|(path, _)| Some(path) != self.target_path.as_ref());
        if self.info_open && stale {
            self.request_image_info();
        }

        if let Some((path, receiver)) = self.info_probe.take() {
            match receiver.try_recv() {
                Ok(result) => self.image_info = Some((path, result)),
                Err(TryRecvError::Empty) => self.info_probe = Some((path, receiver)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
    }
}

/// The window body. Read-only over `PhotonApp`; reports a Refresh click so the
/// caller can re-probe after the borrow ends.
fn info_window_body(ui: &mut egui::Ui, app: &PhotonApp) -> bool {
    if app.target_path.is_none() {
        if app.is_pasted {
            pasted_section(ui, app);
            return false;
        }
        ui.label("No image open.");
        return false;
    }
    let Some(target) = app.target_path.clone() else {
        return false;
    };

    let Some((path, result)) = &app.image_info else {
        ui.label("Reading\u{2026}");
        return false;
    };
    if path != &target {
        // The target changed; the re-probe is in flight.
        ui.label("Reading\u{2026}");
        return false;
    }

    let info = match result {
        Ok(info) => info,
        Err(error) => {
            ui.label(error);
            return refresh_button(ui);
        }
    };

    egui::Grid::new("info_file")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("File:");
            ui.label(
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string()),
            );
            ui.label("Path:");
            ui.label(path.display().to_string());
            ui.label("Format:");
            ui.label(&info.format);
            ui.label("Size:");
            ui.label(image_info::format_size(info.file_size));
            ui.label("Modified:");
            ui.label(
                info.modified
                    .map(image_info::format_modified)
                    .unwrap_or_else(|| "-".to_string()),
            );
        });
    ui.separator();
    egui::Grid::new("info_image")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Dimensions:");
            ui.label(format!("{} × {} px", info.width, info.height));
            ui.label("Colour depth:");
            ui.label(&info.colour_depth);
            ui.label("Alpha:");
            ui.label(if info.alpha { "yes" } else { "no" });
            ui.label("Colour space:");
            let space = match app.current_color_space {
                Some(crate::image_loader::ColorSpace::Srgb) => "sRGB",
                Some(crate::image_loader::ColorSpace::Linear) => "Linear",
                None => "-",
            };
            ui.label(if app.current_hdr {
                format!("{space} (HDR, tonemapped)")
            } else {
                space.to_string()
            });
            if let Some(compression) = &info.dds_compression {
                ui.label("Compression:");
                ui.label(compression);
            }
            ui.label("Frames:");
            ui.label(frames_label(app));
        });
    ui.separator();
    egui::Grid::new("info_metadata")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("ICC profile:");
            ui.label(present(info.icc));
            ui.label("XMP:");
            ui.label(present(info.xmp));
            ui.label("IPTC:");
            ui.label(present(info.iptc));
            ui.label("Exif:");
            ui.label(present(info.exif.as_ref().map(|b| b.len())));
        });

    if let Some(block) = info.exif.as_ref() {
        let rows = crate::app::exif::parse(block);
        if rows.is_empty() {
            ui.label("Exif block unreadable.");
        } else {
            egui::CollapsingHeader::new(format!("Exif tags ({})", rows.len())).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        egui::Grid::new("info_exif")
                            .num_columns(3)
                            .striped(true)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                for row in &rows {
                                    ui.label(row.ifd);
                                    ui.label(&row.tag);
                                    ui.label(&row.value);
                                    ui.end_row();
                                }
                            });
                    });
            });
        }
    }

    refresh_button(ui)
}

fn pasted_section(ui: &mut egui::Ui, app: &PhotonApp) {
    egui::Grid::new("info_pasted")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Source:");
            ui.label("Clipboard paste (not on disk)");
            if let Some((w, h)) = app.image_dimensions {
                ui.label("Dimensions:");
                ui.label(format!("{w} × {h} px"));
            }
            ui.label("Colour space:");
            ui.label(match app.current_color_space {
                Some(crate::image_loader::ColorSpace::Srgb) => "sRGB",
                Some(crate::image_loader::ColorSpace::Linear) => "Linear",
                None => "-",
            });
        });
}

/// How many layers the displayed image has: a flat image, a cubemap, or a
/// texture array.
fn frames_label(app: &PhotonApp) -> String {
    match app.displayed_image.as_ref() {
        None => "-".to_string(),
        Some(image) if image.is_cubemap => {
            format!("6 faces (cubemap), {} shown", app.current_layer + 1)
        }
        Some(image) if image.layers.len() > 1 => format!(
            "{} layers, {} shown",
            image.layers.len(),
            app.current_layer + 1
        ),
        Some(_) => "1 (flat image)".to_string(),
    }
}

fn present(bytes: Option<usize>) -> String {
    match bytes {
        Some(n) => format!("present ({n} bytes)"),
        None => "none".to_string(),
    }
}

fn refresh_button(ui: &mut egui::Ui) -> bool {
    ui.add_space(4.0);
    ui.button("Refresh").clicked()
}
