use crate::app::edit;
use crate::app::image_op::{self, ImageOp};
use crate::app::view;
use crate::app::{HoveredPixel, MouseMode, PhotonApp, StatusOverlay};
use eframe::egui;
use std::sync::Arc;

/// Upper bound on the number of copies tile-repeat will draw, so a tiny image
/// zoomed far out cannot spawn an unbounded grid of paint callbacks.
const MAX_TILE_COPIES: usize = 256;

impl PhotonApp {
    pub(crate) fn draw_image_workspace(&mut self, ui: &mut egui::Ui) {
        // A loader error for the active target takes precedence over the
        // loading/empty overlays: show it and stop (a failed load can't be
        // viewed). The overlay is cleared on navigation / paste / dir load.
        if let Some(overlay) = self.error_overlay.clone() {
            self.ui_status_overlay(ui.ctx(), overlay);
            return;
        }

        let Some(image) = self.displayed_image.clone() else {
            if self.is_loading {
                self.ui_status_overlay(ui.ctx(), StatusOverlay::Loading(self.loading_text()));
            } else {
                self.ui_status_overlay(ui.ctx(), StatusOverlay::NoImages);
            }
            return;
        };

        let Some((img_w, img_h)) = self.image_dimensions else {
            if self.is_loading {
                self.ui_status_overlay(ui.ctx(), StatusOverlay::Loading(self.loading_text()));
            } else {
                self.ui_status_overlay(ui.ctx(), StatusOverlay::NoImages);
            }
            return;
        };

        // Allocate an interactive canvas area
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());

        // Handle auto-fit logic before calculating screen rect
        let texture_size = egui::vec2(img_w as f32, img_h as f32);

        // A freshly displayed image resizes the OS window to it per the
        // configured fit-window mode (Roadmap §3); consumes the load trigger.
        if std::mem::take(&mut self.pending_window_fit) {
            self.fit_window_to_image(ui.ctx());
        }

        if self.needs_fit {
            // Fit using the user-selected mode (default reduce-only: shrink a
            // too-large image to the window, never upscale past 100%). A locked
            // zoom overrides the computed fit (Roadmap §3).
            let fit = view::fit_zoom(response.rect.size(), texture_size, self.settings.fit_mode);
            self.zoom = self.zoom_lock.apply(fit);
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

        // Draw the image through the custom render pass (shader-based sharp
        // scaling; oversized images arrive pre-chunked into tiles). Dimmed
        // while the active load is in flight; the spinner draws on top.
        let tint: [f32; 4] = if self.is_loading {
            [0.5, 0.5, 0.5, 1.0]
        } else {
            [1.0, 1.0, 1.0, 1.0]
        };
        // Sharp only when magnified: snapping while minifying aliases.
        let sharp = self.settings.use_sharp_scaling && self.zoom >= 1.0;
        // Tile-repeat ("seamless texture") view: draw the image once at the
        // centre rect plus any neighbouring copies needed to cover the canvas.
        // A canvas-drawing concern only — the shader pass is untouched.
        let tile_offsets = if self.settings.tile_repeat {
            view::tile_offsets(response.rect, image_screen_rect, MAX_TILE_COPIES)
        } else {
            vec![egui::Vec2::ZERO]
        };
        for offset in tile_offsets {
            let rect = image_screen_rect.translate(offset);
            let callback = crate::render::DrawImageCallback::new(
                image.clone(),
                self.current_layer,
                sharp,
                tint,
                rect,
                self.settings.exposure,
            );
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        }

        // Status-bar pixel readout: sample the decoded bytes under the cursor.
        // `active_rgba` is only set by an `Active` worker load, so a cache-hit
        // load has no sample — the readout simply stays hidden.
        self.hovered_pixel = response.hover_pos().and_then(|pos| {
            let (px, py) = view::image_pixel_at(pos, image_screen_rect, self.zoom, img_w, img_h)?;
            let layers = self.active_rgba.as_ref()?;
            let rgba = layers.get(self.current_layer).or_else(|| layers.last())?;
            let idx = (py as usize * img_w as usize + px as usize) * 4;
            let sample = rgba.get(idx..idx + 4)?;
            Some(HoveredPixel {
                x: px,
                y: py,
                rgba: [sample[0], sample[1], sample[2], sample[3]],
            })
        });

        if self.is_loading {
            self.ui_status_overlay(ui.ctx(), StatusOverlay::Loading(self.loading_text()));
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
                // Use smoother log-scale zoom. Limits are derived from the
                // image/viewport, not fixed: a giant image can zoom out to
                // fit, a tiny one up to fill the screen (see `view`).
                let zoom_delta = scroll_delta * 0.005;
                let new_zoom = view::clamp_to_limits(
                    self.zoom * (1.0 + zoom_delta),
                    view::zoom_limits(response.rect.size(), texture_size),
                );

                // Adjust pan so the image scales around the mouse cursor or center
                let zoom_center = ui
                    .input(|i| i.pointer.hover_pos())
                    .filter(|&pos| response.rect.contains(pos))
                    .unwrap_or_else(|| image_screen_rect.center());

                // The math: To keep zoom_center fixed, we adjust pan:
                let center_dist = zoom_center - image_screen_rect.center();
                self.pan -= center_dist * (new_zoom / old_zoom - 1.0);
                self.zoom = new_zoom;
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

                            // The selection lives in image space, so it stays
                            // pinned to the pixels across zoom/pan.
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
                    }
                }
            }
        }

        // Draw the selection box, mapped through the current transform so it
        // stays pinned to the image pixels across zoom and pan.
        if let Some(sel) = self.selection_image_rect {
            let to_screen = |p: egui::Pos2| {
                egui::pos2(
                    response.rect.min.x + p.x * scale_x + center_offset.x + self.pan.x,
                    response.rect.min.y + p.y * scale_y + center_offset.y + self.pan.y,
                )
            };
            let screen = egui::Rect::from_min_max(to_screen(sel.min), to_screen(sel.max));
            painter.rect_stroke(
                screen,
                0.0,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(50, 150, 255)),
                egui::StrokeKind::Outside,
            );
            painter.rect_filled(
                screen,
                0.0,
                egui::Color32::from_rgba_unmultiplied(50, 150, 255, 50),
            );
        }
    }
}

/// Interactive editing (Roadmap.md §4): apply a whole-image op to the displayed
/// image, undo/redo, and re-upload. The ops themselves are pure (`edit`); this
/// is the canvas/state glue.
impl PhotonApp {
    /// Drop the whole-image undo history. Called when the displayed target
    /// changes, so an edit history never leaks onto a different file.
    pub(crate) fn clear_edit_history(&mut self) {
        self.edit_undo = edit::UndoStack::new();
    }

    /// Apply edits to the displayed image and re-upload the result. The ops
    /// are the shared [`ImageOp`] vocabulary, so the editor, the command line
    /// and the batch chain run the same implementation; a chain lands as one
    /// undo step back to the pre-edit image.
    pub(crate) fn apply_edit(&mut self, ops: &[ImageOp]) {
        if ops.is_empty() {
            return;
        }
        let Some((pixels, width, height)) = crate::app::clipboard::current_rgba(self) else {
            return;
        };
        // Seed the history with the pre-edit state so the first edit is
        // undoable, then record the result as the new current state.
        let seed = self
            .edit_undo
            .is_empty()
            .then(|| edit::Rgba::new(pixels.clone(), width, height));
        let (out, out_w, out_h) = image_op::apply(pixels, width, height, ops);
        let out = edit::Rgba::new(out, out_w, out_h);

        if let Some(seed) = seed {
            self.edit_undo.push(seed);
        }
        self.edit_undo.push(out.clone());
        self.show_edited(out);
    }

    pub(crate) fn undo_edit(&mut self) {
        if let Some(snapshot) = self.edit_undo.undo() {
            self.show_edited(snapshot);
        }
    }

    pub(crate) fn redo_edit(&mut self) {
        if let Some(snapshot) = self.edit_undo.redo() {
            self.show_edited(snapshot);
        }
    }

    /// The current selection as a crop op (margins, matching `/crop:(L,T,R,B)`),
    /// if any. Empty or degenerate selections are rejected (a crop needs a
    /// positive extent).
    pub(crate) fn crop_op_from_selection(&self) -> Option<ImageOp> {
        let sel = self.selection_image_rect?;
        let (img_w, img_h) = self.image_dimensions?;
        let x0 = (sel.min.x.max(0.0).round() as u32).min(img_w);
        let y0 = (sel.min.y.max(0.0).round() as u32).min(img_h);
        let x1 = (sel.max.x.max(0.0).round() as u32).min(img_w);
        let y1 = (sel.max.y.max(0.0).round() as u32).min(img_h);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(ImageOp::Crop {
            left: x0,
            top: y0,
            right: img_w - x1,
            bottom: img_h - y1,
        })
    }

    /// Upload an edited buffer as the displayed image.
    ///
    /// Built on the main thread, not the loader worker, and kept out of the LRU
    /// cache: the cache holds unmodified disk images, so a cache-hit copy/save
    /// always matches the pixels on screen and an edit stays transient to the
    /// current view (it is discarded when navigating away).
    fn show_edited(&mut self, image: edit::Rgba) {
        let (width, height) = (image.width, image.height);
        // Keep the source's colour space so a linear image stays linear through
        // an edit (the encoded/linear branch in the shader is unchanged).
        let color_space = self
            .current_color_space
            .unwrap_or(crate::image_loader::ColorSpace::Srgb);
        let built = {
            let (Some(device), Some(queue), Some(renderer)) = (
                self.device.as_ref(),
                self.queue.as_deref(),
                self.image_renderer.as_ref(),
            ) else {
                return;
            };
            let ctx = crate::render::RenderContext {
                device,
                queue,
                tile_layout: &renderer.tile_layout,
                max_dim: device.limits().max_texture_dimension_2d,
            };
            Arc::new(crate::render::build_render_image(
                &ctx,
                vec![image.pixels.clone()],
                width,
                height,
                false,
                crate::image_loader::DecodedFormat::Rgba8,
                color_space,
            ))
        };

        let dimensions_changed = self.image_dimensions != Some((width, height));
        self.displayed_image = Some(built);
        self.current_layer = 0;
        self.layers_are_cubemap = false;
        self.image_dimensions = Some((width, height));
        self.is_loading = false;
        self.error_overlay = None;
        self.selection_image_rect = None;
        self.selection_start = None;

        // Editing a pasted image edits the paste buffer; editing a file image
        // replaces the active decoded bytes so copy/save see the result.
        if self.is_pasted {
            self.pasted_rgba = Some(image.pixels);
        } else {
            self.pasted_rgba = None;
            self.active_rgba = Some(vec![image.pixels]);
        }

        // Re-fit when the edit changed the dimensions (rotate/crop), following
        // the same policy as navigation; a locked zoom still wins.
        let reset = match self.settings.zoom_reset {
            crate::app::ZoomReset::Always => true,
            crate::app::ZoomReset::DimensionChange => dimensions_changed,
            crate::app::ZoomReset::Never => false,
        };
        if reset {
            self.zoom = self.zoom_lock.apply(1.0);
            self.pan = egui::Vec2::ZERO;
            if self.settings.scale_to_fit {
                self.needs_fit = true;
            }
        }
    }
}

/// Fit-window / zoom-to-desktop actions (Roadmap §3).
impl PhotonApp {
    /// Resize the OS window to the displayed image per `settings.fit_window`,
    /// then recentre it. No-op when the mode is `DontFit` or nothing is loaded.
    pub(crate) fn fit_window_to_image(&mut self, ctx: &egui::Context) {
        if self.settings.fit_window == view::FitWindow::DontFit {
            return;
        }
        let Some((width, height)) = self.image_dimensions else {
            return;
        };
        let window = ctx.viewport_rect().size();
        let monitor = monitor_size(ctx, window);
        let image = egui::vec2(width as f32, height as f32);
        let target = view::fit_window_size(image, window, monitor, self.settings.fit_window);
        // Only touch the window if the size actually changed (avoids churn on
        // every navigation when the window already matches).
        if (target.x - window.x).abs() >= 1.0 || (target.y - window.y).abs() >= 1.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
            self.center_window(ctx);
        }
    }

    /// Move the OS window to the centre of the monitor.
    ///
    /// Assumes the monitor's origin is the desktop origin (true on a single
    /// monitor; a window on a secondary monitor is recentred on the primary).
    /// Wayland ignores client window positioning, so this is a no-op there.
    pub(crate) fn center_window(&self, ctx: &egui::Context) {
        let window = ctx.viewport_rect().size();
        let monitor = monitor_size(ctx, window);
        let pos = view::centered_window_pos(window, monitor);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            pos.x, pos.y,
        )));
    }

    /// Zoom so the image fits the monitor, never past 1:1 (Roadmap §3).
    pub(crate) fn zoom_to_desktop(&mut self, ctx: &egui::Context) {
        let Some((width, height)) = self.image_dimensions else {
            return;
        };
        let monitor = monitor_size(ctx, ctx.viewport_rect().size());
        self.zoom = view::zoom_to_desktop(egui::vec2(width as f32, height as f32), monitor);
        self.pan = egui::Vec2::ZERO;
    }
}

/// The current monitor's size in points, falling back to the window size on
/// backends that do not report it.
fn monitor_size(ctx: &egui::Context, fallback: egui::Vec2) -> egui::Vec2 {
    ctx.input(|i| i.viewport().monitor_size).unwrap_or(fallback)
}
