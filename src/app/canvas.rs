use crate::app::{MouseMode, PhotonApp, StatusOverlay};
use eframe::egui;

impl PhotonApp {
    pub(crate) fn ui_status_overlay(&self, ctx: &egui::Context, state: StatusOverlay) {
        egui::Area::new(egui::Id::new("status_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    match state {
                        StatusOverlay::NoImages => {
                            // Subdued grey text for empty state
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("No images loaded.")
                                        .strong()
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(139, 143, 163)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        StatusOverlay::Loading(msg) => {
                            // Spinner + bright Dracula purple/pink text
                            ui.add(egui::widgets::Spinner::new().size(32.0));
                            ui.add_space(10.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(msg)
                                        .strong()
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(248, 248, 242)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        StatusOverlay::Error(err) => {
                            // Dracula Red Alert Cross + Error Text
                            ui.label(egui::RichText::new("❌").size(32.0));
                            ui.add_space(8.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("Error: {}", err))
                                        .strong()
                                        .size(18.0)
                                        .color(egui::Color32::from_rgb(255, 85, 85)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        StatusOverlay::UnsupportedFormat(filename) => {
                            // Dracula Orange Alert Question Mark + Warning Text
                            ui.label(egui::RichText::new("❓").size(32.0));
                            ui.add_space(8.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "Unsupported format: {}",
                                        filename
                                    ))
                                    .strong()
                                    .size(18.0)
                                    .color(egui::Color32::from_rgb(255, 184, 108)),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                    }
                });
            });
    }

    pub(crate) fn draw_image_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(tex_id) = self.image_texture_id else {
            self.ui_status_overlay(ui.ctx(), StatusOverlay::NoImages);
            return;
        };

        let Some((img_w, img_h)) = self.image_dimensions else {
            self.ui_status_overlay(ui.ctx(), StatusOverlay::NoImages);
            return;
        };

        // Allocate an interactive canvas area
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());

        // Handle auto-fit logic before calculating screen rect
        let texture_size = egui::vec2(img_w as f32, img_h as f32);

        if self.needs_fit {
            let available = response.rect.size();
            let scale_x = available.x / texture_size.x;
            let scale_y = available.y / texture_size.y;
            self.zoom = f32::min(scale_x, scale_y).min(1.0); // Don't scale up past 100%
            self.pan = egui::Vec2::ZERO;
            self.needs_fit = false;
        }

        let image_size = texture_size * self.zoom;
        let center_offset = (response.rect.size() - image_size) / 2.0;
        let image_screen_rect =
            egui::Rect::from_min_size(response.rect.min + center_offset + self.pan, image_size);

        if self.settings.checkerboard_bg {
            let clip_rect = ui.clip_rect().intersect(image_screen_rect);
            if clip_rect.is_positive() {
                let size = 20.0;
                let start_x = (clip_rect.min.x / size).floor() as isize;
                let end_x = (clip_rect.max.x / size).ceil() as isize;
                let start_y = (clip_rect.min.y / size).floor() as isize;
                let end_y = (clip_rect.max.y / size).ceil() as isize;

                let color1 = egui::Color32::from_rgb(180, 180, 180);
                let color2 = egui::Color32::from_rgb(130, 130, 130);

                let mut mesh = egui::Mesh::default();
                for y in start_y..end_y {
                    for x in start_x..end_x {
                        let is_dark = (x + y) % 2 == 0;
                        let color = if is_dark { color1 } else { color2 };
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(x as f32 * size, y as f32 * size),
                            egui::pos2((x + 1) as f32 * size, (y + 1) as f32 * size),
                        );
                        let clipped = rect.intersect(image_screen_rect);
                        if clipped.is_positive() {
                            mesh.add_colored_rect(clipped, color);
                        }
                    }
                }
                painter.add(egui::Shape::mesh(mesh));
            }
        }

        // Draw the image
        painter.image(
            tex_id,
            image_screen_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            if self.is_loading {
                egui::Color32::from_gray(128)
            } else {
                egui::Color32::WHITE
            },
        );

        if self.is_loading {
            let text = if let Some(p) = &self.target_path {
                format!(
                    "Loading {}...",
                    p.file_name().unwrap_or_default().to_string_lossy()
                )
            } else {
                "Loading...".to_string()
            };
            self.ui_status_overlay(ui.ctx(), StatusOverlay::Loading(text));
        }

        // Calculate scale factors for image <-> screen conversion
        let scale_x = image_size.x / texture_size.x;
        let scale_y = image_size.y / texture_size.y;

        // Handle input based on mode
        let mouse_pos = response.interact_pointer_pos();

        // Scroll wheel - zoom or navigate
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 && response.hovered() {
            if self.mode == MouseMode::Navigate {
                self.scroll_accumulator += scroll_delta;
                let threshold = 30.0;
                if self.scroll_accumulator >= threshold {
                    self.prev_image();
                    self.scroll_accumulator = 0.0;
                } else if self.scroll_accumulator <= -threshold {
                    self.next_image();
                    self.scroll_accumulator = 0.0;
                }
            } else {
                let old_zoom = self.zoom;
                // Use smoother log-scale zoom
                let zoom_delta = scroll_delta * 0.005;
                let new_zoom = (self.zoom * (1.0 + zoom_delta)).clamp(0.1, 50.0);

                // Adjust pan so the image scales around the mouse cursor or center
                let zoom_center = ui
                    .input(|i| i.pointer.hover_pos())
                    .filter(|&pos| response.rect.contains(pos))
                    .unwrap_or_else(|| image_screen_rect.center());

                // The math: To keep zoom_center fixed, we adjust pan:
                let center_dist = zoom_center - image_screen_rect.center();
                self.pan -= center_dist * (new_zoom / old_zoom - 1.0);
                self.zoom = new_zoom;

                // Update selection in image-space when zooming
                if let Some(sel) = self.selection {
                    let new_sel = egui::Rect::from_min_size(
                        egui::pos2(
                            (sel.min.x - response.rect.min.x - center_offset.x - self.pan.x)
                                / scale_x,
                            (sel.min.y - response.rect.min.y - center_offset.y - self.pan.y)
                                / scale_y,
                        ),
                        egui::vec2(sel.width() / scale_x, sel.height() / scale_y),
                    );
                    self.selection_image_rect = Some(new_sel);
                }
            }
        }

        // Handle mouse interaction
        if let Some(mouse_pos) = mouse_pos {
            self.last_mouse_pos = mouse_pos;

            match self.mode {
                MouseMode::Browse => {
                    // Pan with left or middle mouse drag
                    if response.dragged() {
                        self.pan += response.drag_delta();

                        // Update screen-space selection to follow pan
                        if let Some(image_sel) = self.selection_image_rect {
                            let screen_min = egui::pos2(
                                response.rect.min.x
                                    + image_sel.min.x * scale_x
                                    + center_offset.x
                                    + self.pan.x,
                                response.rect.min.y
                                    + image_sel.min.y * scale_y
                                    + center_offset.y
                                    + self.pan.y,
                            );
                            let screen_max = egui::pos2(
                                response.rect.min.x
                                    + image_sel.max.x * scale_x
                                    + center_offset.x
                                    + self.pan.x,
                                response.rect.min.y
                                    + image_sel.max.y * scale_y
                                    + center_offset.y
                                    + self.pan.y,
                            );
                            self.selection = Some(egui::Rect::from_min_max(screen_min, screen_max));
                        }
                    }
                }
                MouseMode::Select => {
                    // Left click drag for selection
                    if response.dragged_by(egui::PointerButton::Primary) {
                        if response.drag_started() {
                            self.selection_start = Some(mouse_pos);
                        }

                        if let Some(start) = self.selection_start {
                            let min =
                                egui::pos2(start.x.min(mouse_pos.x), start.y.min(mouse_pos.y));
                            let max =
                                egui::pos2(start.x.max(mouse_pos.x), start.y.max(mouse_pos.y));
                            let screen_rect = egui::Rect::from_min_max(min, max);
                            self.selection = Some(screen_rect);

                            // Convert to image-space
                            let image_min = egui::pos2(
                                (min.x - response.rect.min.x - center_offset.x - self.pan.x)
                                    / scale_x,
                                (min.y - response.rect.min.y - center_offset.y - self.pan.y)
                                    / scale_y,
                            );
                            let image_max = egui::pos2(
                                (max.x - response.rect.min.x - center_offset.x - self.pan.x)
                                    / scale_x,
                                (max.y - response.rect.min.y - center_offset.y - self.pan.y)
                                    / scale_y,
                            );
                            self.selection_image_rect =
                                Some(egui::Rect::from_min_max(image_min, image_max));
                        }
                    } else if response.clicked() {
                        // Reset selection on click
                        self.selection = None;
                        self.selection_image_rect = None;
                        self.selection_start = None;
                    } else if !response.dragged() {
                        // Finalize selection
                        self.selection_start = None;
                    }
                }
                MouseMode::Navigate => {
                    // Middle mouse pan only
                    if response.dragged_by(egui::PointerButton::Middle) {
                        self.pan += response.drag_delta();

                        // Update screen-space selection to follow pan
                        if let Some(image_sel) = self.selection_image_rect {
                            let screen_min = egui::pos2(
                                response.rect.min.x
                                    + image_sel.min.x * scale_x
                                    + center_offset.x
                                    + self.pan.x,
                                response.rect.min.y
                                    + image_sel.min.y * scale_y
                                    + center_offset.y
                                    + self.pan.y,
                            );
                            let screen_max = egui::pos2(
                                response.rect.min.x
                                    + image_sel.max.x * scale_x
                                    + center_offset.x
                                    + self.pan.x,
                                response.rect.min.y
                                    + image_sel.max.y * scale_y
                                    + center_offset.y
                                    + self.pan.y,
                            );
                            self.selection = Some(egui::Rect::from_min_max(screen_min, screen_max));
                        }
                    }
                }
            }
        }

        // Draw selection box (screen-space)
        if let Some(sel) = self.selection {
            painter.rect_stroke(
                sel,
                0.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(50, 150, 255)),
                egui::StrokeKind::Outside,
            );
            painter.rect_filled(
                sel,
                0.0,
                egui::Color32::from_rgba_unmultiplied(50, 150, 255, 50),
            );
        }
    }
}
