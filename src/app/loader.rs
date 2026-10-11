use crate::app::{
    LoadRequest, LoadResult, PhotonApp, Priority, SortMode, StatusOverlay, WorkerMessage, ZoomReset,
};
use crate::dir_walker;
use eframe::egui;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::Instant;

impl PhotonApp {
    pub(crate) fn init_worker(&mut self) {
        if self.request_sender.is_some() {
            return;
        }

        let device = self.device.clone().expect("wgpu device not initialized");
        let queue = self.queue.clone().expect("wgpu queue not initialized");
        let renderer = self
            .image_renderer
            .clone()
            .expect("image renderer not initialized");
        let tile_layout = renderer.tile_layout.clone();

        let (req_tx, req_rx) = mpsc::channel::<WorkerMessage>();
        let (res_tx, res_rx) = mpsc::sync_channel(5);

        self.request_sender = Some(req_tx);
        self.load_receiver = Some(res_rx);
        let latest_req_id = self.latest_req_id.clone();

        std::thread::spawn(move || {
            let mut pending_requests: VecDeque<WorkerMessage> = VecDeque::new();

            loop {
                // Try blocking recv if queue is empty
                if pending_requests.is_empty() {
                    match req_rx.recv() {
                        Ok(msg) => pending_requests.push_back(msg),
                        Err(_) => break,
                    }
                }

                // Read all available without blocking
                while let Ok(msg) = req_rx.try_recv() {
                    pending_requests.push_back(msg);
                }

                // Cleanup stale preloads keeping only requests close to target
                let current_target_id = latest_req_id.load(Ordering::SeqCst);
                pending_requests.retain(|msg| {
                    match msg {
                        WorkerMessage::Load(r) => {
                            // Drop any Active request that isn't the current target
                            if r.priority == Priority::Active {
                                r.request_id == current_target_id
                            } else {
                                true
                            }
                        }
                        WorkerMessage::Paste { .. } => true, // Paste is always active
                    }
                });

                // Find the best request to process
                let mut target_idx = None;
                let mut has_active = false;
                for (i, m) in pending_requests.iter().enumerate() {
                    match m {
                        WorkerMessage::Load(r) if r.priority == Priority::Active => {
                            target_idx = Some(i);
                            has_active = true;
                            break;
                        }
                        WorkerMessage::Paste { .. } => {
                            target_idx = Some(i);
                            has_active = true;
                            break;
                        }
                        _ => {}
                    }
                }

                let msg_to_process: Option<WorkerMessage> = if let Some(idx) = target_idx {
                    let msg = pending_requests.remove(idx);
                    // If we just picked up an Active/Paste request, drop all PREVIOUS preloads
                    // as they are likely from an old folder/index
                    if has_active {
                        pending_requests.retain(|m| matches!(m, WorkerMessage::Paste { .. }));
                    }
                    msg
                } else {
                    pending_requests.pop_front()
                };

                // Process request
                let start_total = Instant::now();
                let max_dim_limit = device.limits().max_texture_dimension_2d;

                let (decoded, path_for_tx, rgba_for_res, is_active_load) = match msg_to_process {
                    Some(WorkerMessage::Load(ref req)) => {
                        let opts = crate::image_loader::DecodeOptions {
                            // Compressed DDS uploads straight to VRAM when the
                            // device supports BC textures; otherwise (and for
                            // every other format) it CPU-decodes to RGBA8.
                            allow_bc: device
                                .features()
                                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
                            max_dim: max_dim_limit,
                        };
                        match crate::image_loader::load_image_opts(&req.path, opts) {
                            Ok(decoded) => {
                                let active = req.priority == Priority::Active;
                                log::debug!(
                                    "decoded {:?}: format {:?}, colour space {:?}",
                                    req.path.file_name(),
                                    decoded.format,
                                    decoded.color_space
                                );
                                (decoded, Some(req.path.clone()), None, active)
                            }
                            Err(e) => {
                                // Surface the error to the main thread instead of swallowing it.
                                // The typed `DecodeError` classifies itself: decode-time
                                // failures (bad/malformed pixel data) select the
                                // UnsupportedFormat overlay; open and format-guess
                                // failures fall back to the general Error overlay.
                                let is_format_error = e.is_unsupported_format();
                                log::warn!("Failed to load image {:?}: {}", req.path, e);
                                let _ = res_tx.send(LoadResult::Failed {
                                    path: Some(req.path.clone()),
                                    is_format_error,
                                    message: e.to_string(),
                                });
                                continue;
                            }
                        }
                    }
                    Some(WorkerMessage::Paste {
                        rgba,
                        width,
                        height,
                    }) => (
                        crate::image_loader::DecodedImage {
                            layers: vec![rgba.clone()],
                            width,
                            height,
                            is_cubemap: false,
                            // Clipboard images arrive as 8-bit sRGB bytes.
                            format: crate::image_loader::DecodedFormat::Rgba8,
                            color_space: crate::image_loader::ColorSpace::Srgb,
                        },
                        None,
                        Some(vec![rgba]),
                        true,
                    ),
                    None => continue,
                };

                let (width, height) = (decoded.width, decoded.height);
                let is_cubemap = decoded.is_cubemap;

                // Images larger than the GPU's per-texture limit are chunked
                // into tiles here and reassembled by the render pass, so the
                // old oversized rejection only needs to cover zero extents.
                let max_dim = max_dim_limit;
                if width == 0 || height == 0 {
                    let reason = format!("Image has zero extent ({}x{})", width, height);
                    log::warn!("Failed to load image {:?}: {}", path_for_tx, reason);
                    let _ = res_tx.send(LoadResult::Failed {
                        path: path_for_tx,
                        is_format_error: false,
                        message: reason,
                    });
                    continue;
                }

                // Build the GPU-resident image: every layer, chunked, with
                // bind groups and vertex quads. Active/Paste results hand the
                // decoded bytes back too so copy/save can reuse them — but only
                // for RGBA8 layers: a compressed layer has no decoded pixels, so
                // copy/save/edit re-decode from disk via the `current_rgba` fallback.
                let upload_start = Instant::now();
                let render_ctx = crate::render::RenderContext {
                    device: &device,
                    queue: &queue,
                    tile_layout: &tile_layout,
                    max_dim,
                };
                let image = Arc::new(crate::render::build_render_image(
                    &render_ctx,
                    decoded.layers.clone(),
                    width,
                    height,
                    is_cubemap,
                    decoded.format,
                    decoded.color_space,
                ));
                let _upload_time = upload_start.elapsed();

                let is_rgba8 = decoded.format == crate::image_loader::DecodedFormat::Rgba8;
                let rgba_for_res =
                    rgba_for_res.or_else(|| (is_active_load && is_rgba8).then_some(decoded.layers));

                if let Some(ref path) = path_for_tx {
                    log::info!(
                        "Worker loaded: {:?} | {} layer(s), {} tile(s) | Total: {:?}",
                        path.file_name(),
                        image.layers.len(),
                        image.layers.iter().map(|l| l.tiles.len()).sum::<usize>(),
                        start_total.elapsed()
                    );
                } else {
                    log::info!(
                        "Worker created pasted texture | Total: {:?}",
                        start_total.elapsed()
                    );
                }

                // This will block if res_rx is full (sync_channel(5))
                let _ = res_tx.send(LoadResult::Loaded {
                    path: path_for_tx,
                    image,
                    rgba: rgba_for_res,
                });
            }
        });
    }

    pub(crate) fn load_image(&mut self, path: PathBuf, priority: Priority) {
        if let Some(sender) = &self.request_sender {
            let req_id = if priority == Priority::Active {
                self.target_path = Some(path.clone());
                let id = self.latest_req_id.fetch_add(1, Ordering::SeqCst) + 1;
                self.latest_req_id.store(id, Ordering::SeqCst);
                id
            } else {
                0
            };

            let _ = sender.send(WorkerMessage::Load(LoadRequest {
                path,
                priority,
                request_id: req_id,
            }));
        }
    }

    pub(crate) fn request_current(&mut self) {
        if self.images.is_empty() {
            return;
        }

        self.error_overlay = None;

        let path = self.images[self.current_index].clone();

        // Drop the previous image's decoded bytes when the target changes;
        // refreshed when the new target's Active load completes. Its edit
        // history goes with it.
        if self.target_path.as_ref() != Some(&path) {
            self.active_rgba = None;
            self.clear_edit_history();
        }

        // 1. Check if already in cache and fully loaded
        if let Some(cached) = self.texture_cache.get(&path).cloned() {
            // Compare against the *previous* image's dimensions before they
            // are replaced below: zoom resets only when the new image is a
            // different size (per the configured ZoomReset behavior).
            let (w, h) = cached.dims();
            let dimensions_changed = self.image_dimensions != Some((w, h));
            let color_space = cached.color_space;
            self.displayed_image = Some(cached);
            self.current_color_space = Some(color_space);
            self.pending_window_fit = true;
            self.current_layer = 0;
            self.layers_are_cubemap = self
                .displayed_image
                .as_ref()
                .is_some_and(|image| image.is_cubemap);
            self.image_dimensions = Some((w, h));
            self.target_path = Some(path.clone());
            self.is_loading = false;
            self.is_pasted = false;

            // Reset zoom on cached loads according to the configured behavior.
            let reset = match self.settings.zoom_reset {
                ZoomReset::Always => true,
                ZoomReset::DimensionChange => dimensions_changed,
                ZoomReset::Never => false,
            };
            if reset {
                // Locked magnification overrides the reset (Roadmap §3).
                self.zoom = self.zoom_lock.apply(1.0);
                self.pan = egui::Vec2::ZERO;
                self.selection = None;
                self.selection_image_rect = None;
                self.selection_start = None;

                if self.settings.scale_to_fit {
                    self.needs_fit = true;
                }
            }
        } else {
            self.target_path = Some(path.clone());
            self.is_loading = true;
            self.load_image(path, Priority::Active);
        }

        // Stamp the displayed file's mtime for the directory watcher's
        // changed-on-disk check.
        self.current_mtime = self
            .target_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());

        self.scroll_to_target = true;

        // 2. Preload neighbors
        if self.images.len() > 1 {
            let next_idx = (self.current_index + 1) % self.images.len();
            let prev_idx = if self.current_index == 0 {
                self.images.len() - 1
            } else {
                self.current_index - 1
            };

            let next_path = self.images[next_idx].clone();
            if !self.texture_cache.contains(&next_path) {
                self.load_image(next_path, Priority::Preload);
            }

            let prev_path = self.images[prev_idx].clone();
            if !self.texture_cache.contains(&prev_path) {
                self.load_image(prev_path, Priority::Preload);
            }
        }
    }

    pub(crate) fn check_load_complete(&mut self, ctx: &egui::Context) {
        if let Some(ref mut receiver) = self.load_receiver {
            while let Ok(result) = receiver.try_recv() {
                match result {
                    LoadResult::Loaded { path, image, rgba } => {
                        // No manual GPU frees: cached and displayed images are
                        // Arcs; wgpu frees the resources when the last
                        // reference (including in-flight frames) drops.
                        let effective_path =
                            path.clone().unwrap_or_else(|| PathBuf::from("__pasted__"));
                        self.texture_cache.put(effective_path, image.clone());

                        // Update display if this was the active target
                        let is_active = if let Some(ref p) = path {
                            self.target_path.as_ref() == Some(p)
                        } else {
                            self.is_pasted
                        };

                        if is_active {
                            let (width, height) = image.dims();
                            let dimensions_changed = self.image_dimensions != Some((width, height));
                            let is_cubemap = image.is_cubemap;
                            let color_space = image.color_space;
                            self.displayed_image = Some(image);
                            self.current_color_space = Some(color_space);
                            self.pending_window_fit = true;
                            self.current_layer = 0;
                            self.layers_are_cubemap = is_cubemap;
                            self.image_dimensions = Some((width, height));
                            self.is_loading = false;
                            self.error_overlay = None;

                            let reset = match self.settings.zoom_reset {
                                ZoomReset::Always => true,
                                ZoomReset::DimensionChange => dimensions_changed,
                                ZoomReset::Never => false,
                            };
                            if reset {
                                self.zoom = 1.0;
                                self.pan = egui::Vec2::ZERO;
                                self.selection = None;
                                self.selection_image_rect = None;
                                self.selection_start = None;

                                if self.settings.scale_to_fit {
                                    self.needs_fit = true;
                                }
                            }

                            if path.is_none() {
                                self.pasted_rgba = rgba.and_then(|mut layers| layers.pop());
                            } else {
                                self.pasted_rgba = None;
                                self.active_rgba = rgba;
                            }

                            if self.settings.scale_to_fit {
                                self.needs_fit = true;
                            }
                        }
                        // Non-target results (neighbor preloads, stale
                        // actives) are cached above but never displayed:
                        // showing them as a dimmed "intermediate" put the
                        // neighbor's pixels on screen for the wrong file.
                        // While loading, the previous image stays visible
                        // under the spinner until the target arrives.

                        ctx.request_repaint();
                    }
                    LoadResult::Failed {
                        path,
                        is_format_error,
                        message,
                    } => {
                        // Only react if the failure is for the currently displayed target,
                        // not a stale preload for a previously-current image.
                        let is_active = if let Some(ref p) = &path {
                            self.target_path.as_ref() == Some(p)
                        } else {
                            self.is_pasted
                        };

                        if is_active {
                            self.is_loading = false;
                            let filename = path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            self.error_overlay = Some(if is_format_error {
                                StatusOverlay::UnsupportedFormat(filename)
                            } else {
                                StatusOverlay::Error(message)
                            });
                            ctx.request_repaint();
                        } else {
                            // Stale preload failure — leave the active image alone.
                            log::debug!("Stale preload failed for {:?}: {}", path, message);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn load_directory(&mut self, path: &Path) {
        // Clear any stale error from the previous folder before starting anew.
        self.error_overlay = None;

        // Drop all cached and displayed images; wgpu frees the GPU resources
        // when the last reference (including in-flight frames) is gone.
        self.texture_cache.clear();
        self.displayed_image = None;

        let (tx, rx) = mpsc::channel();
        self.dir_load_receiver = Some(rx);
        self.is_loading_dir = true;
        self.images.clear();
        self.current_index = 0;
        self.is_pasted = false;

        let path_buf = path.to_path_buf();
        std::thread::spawn(move || {
            let walker = dir_walker::DirWalker::new(&path_buf);
            let mut files = walker.map(|w| w.files).unwrap_or_default();
            files.sort();
            let _ = tx.send(files);
        });

        // Watch the directory: re-walk on debounced changes and send fresh
        // listings. Dropping `dir_watch_stop` (replaced here) stops any
        // previous watcher thread; the Debouncer lives in the thread, so its
        // drop unregisters the watch on exit.
        let (watch_tx, watch_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        self.dir_watch_stop = Some(stop_tx);
        self.dir_watch_receiver = Some(watch_rx);

        let watch_dir = path.to_path_buf();
        std::thread::spawn(move || {
            let (event_tx, event_rx) = mpsc::channel();
            let mut debouncer = match notify_debouncer_full::new_debouncer(
                std::time::Duration::from_millis(500),
                None,
                move |result| {
                    let _ = event_tx.send(result);
                },
            ) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("Failed to create watcher for {:?}: {}", watch_dir, e);
                    return;
                }
            };
            if let Err(e) = debouncer.watch(
                &watch_dir,
                notify_debouncer_full::notify::RecursiveMode::NonRecursive,
            ) {
                log::warn!("Failed to watch {:?}: {}", watch_dir, e);
                return;
            }

            loop {
                match stop_rx.try_recv() {
                    Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
                match event_rx.recv_timeout(std::time::Duration::from_millis(250)) {
                    Ok(Ok(_events)) => {
                        let files = dir_walker::DirWalker::new(&watch_dir)
                            .map(|w| w.files)
                            .unwrap_or_default();
                        if watch_tx.send(files).is_err() {
                            break;
                        }
                    }
                    Ok(Err(errors)) => {
                        log::warn!("Watch errors on {:?}: {:?}", watch_dir, errors)
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    }

    pub(crate) fn check_dir_load_complete(&mut self) {
        if let Some(ref receiver) = self.dir_load_receiver {
            if let Ok(files) = receiver.try_recv() {
                self.images = files;
                self.dir_load_receiver = None;
                self.is_loading_dir = false;

                if !self.images.is_empty() {
                    self.error_overlay = None;
                    self.sort_images(self.settings.sort_mode);

                    // Try to restore old target path or index
                    let mut found = false;
                    if let Some(target) = &self.target_path {
                        if let Some(idx) = self.images.iter().position(|p| p == target) {
                            self.current_index = idx;
                            found = true;
                        }
                    }
                    if !found {
                        self.current_index = 0;
                    }
                    self.request_current();
                }
            }
        }
    }

    /// Poll the directory watcher for a refreshed listing and apply it.
    /// Skipped while the initial directory walk is still in flight so its
    /// result can't be processed against a stale image list.
    pub(crate) fn check_dir_watch(&mut self) {
        if self.is_loading_dir {
            return;
        }
        let new_files = match self
            .dir_watch_receiver
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            Some(files) => files,
            None => return,
        };
        self.apply_dir_refresh(new_files);
    }

    /// Apply a watcher-produced directory listing while preserving the user's
    /// place: the displayed image stays selected (tracked by path through the
    /// re-sort) and is NOT reloaded unless its file actually changed on disk
    /// (mtime moved since it was displayed). If the displayed file vanished,
    /// land on the nearest remaining entry and load that.
    pub(crate) fn apply_dir_refresh(&mut self, new_files: Vec<PathBuf>) {
        // The debouncer can emit several batches for one user action; skip
        // no-op refreshes (the re-sort stats every file on date/size modes).
        if new_files == self.images {
            return;
        }
        log::debug!(
            "Directory refresh: {} images (was {})",
            new_files.len(),
            self.images.len()
        );
        let current = self.images.get(self.current_index).cloned();
        self.images = new_files;
        // Park current_index on the displayed path so the sort below
        // preserves exactly that image's position.
        if let Some(cur) = &current {
            if let Some(idx) = self.images.iter().position(|p| p == cur) {
                self.current_index = idx;
            }
        }
        self.sort_images(self.settings.sort_mode);

        match current {
            Some(cur) if self.images.iter().any(|p| p == &cur) => {
                let mtime = std::fs::metadata(&cur).and_then(|m| m.modified()).ok();
                if mtime != self.current_mtime {
                    // The file changed on disk: drop its stale cache entry
                    // (freeing textures) and load it again. request_current
                    // re-issues the Active load and refreshes active_rgba.
                    // The file changed on disk: drop its stale cache entry and
                    // load it again (request_current re-issues the Active
                    // load; the dropped Arc frees the old GPU resources).
                    self.texture_cache.pop(&cur);
                    self.request_current();
                }
                self.current_mtime = mtime;
                self.scroll_to_target = true;
            }
            _ => {
                if self.images.is_empty() {
                    self.current_index = 0;
                } else {
                    self.current_index = self.current_index.min(self.images.len() - 1);
                }
                self.request_current();
                self.scroll_to_target = true;
            }
        }
    }

    pub(crate) fn sort_images(&mut self, mode: SortMode) {
        if self.images.is_empty() {
            return;
        }

        let current_path = self.images.get(self.current_index).cloned();

        match mode {
            SortMode::Alphabetical => {
                self.images.sort_by(|a, b| {
                    let a_name = a
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    let b_name = b
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    a_name.cmp(&b_name)
                });
            }
            SortMode::AlphabeticalDesc => {
                self.images.sort_by(|a, b| {
                    let a_name = a
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    let b_name = b
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    b_name.cmp(&a_name)
                });
            }
            SortMode::DateModified => {
                self.images.sort_by(|a, b| {
                    let a_meta = std::fs::metadata(a).and_then(|m| m.modified()).ok();
                    let b_meta = std::fs::metadata(b).and_then(|m| m.modified()).ok();
                    b_meta.cmp(&a_meta)
                });
            }
            SortMode::DateModifiedOldest => {
                self.images.sort_by(|a, b| {
                    let a_meta = std::fs::metadata(a).and_then(|m| m.modified()).ok();
                    let b_meta = std::fs::metadata(b).and_then(|m| m.modified()).ok();
                    a_meta.cmp(&b_meta)
                });
            }
            SortMode::FileSize => {
                self.images.sort_by(|a, b| {
                    let a_size = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
                    let b_size = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
                    b_size.cmp(&a_size)
                });
            }
            SortMode::FileSizeSmallest => {
                self.images.sort_by(|a, b| {
                    let a_size = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
                    let b_size = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
                    a_size.cmp(&b_size)
                });
            }
        }

        if let Some(path) = current_path {
            if let Some(idx) = self.images.iter().position(|p| p == &path) {
                self.current_index = idx;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{PhotonApp, ZoomReset};

    /// App with every image already in the texture cache, so `request_current`
    /// exercises the cached-load path without a worker or GPU device.
    fn cached_app(dims: &[(&str, u32, u32)]) -> PhotonApp {
        let mut app = PhotonApp::new();
        app.settings.zoom_reset = ZoomReset::DimensionChange;
        app.settings.scale_to_fit = false;
        for (name, w, h) in dims {
            app.texture_cache.put(
                PathBuf::from(name),
                std::sync::Arc::new(crate::render::RenderImage {
                    width: *w,
                    height: *h,
                    is_cubemap: false,
                    color_space: crate::image_loader::ColorSpace::Srgb,
                    layers: vec![crate::render::Layer { tiles: Vec::new() }],
                }),
            );
        }
        app.images = dims.iter().map(|(n, _, _)| PathBuf::from(*n)).collect();
        app
    }

    #[test]
    fn zoom_resets_only_when_dimensions_change() {
        let mut app = cached_app(&[
            ("a.png", 100, 100),
            ("b.png", 100, 100),
            ("c.png", 200, 200),
        ]);
        app.current_index = 0;
        app.request_current();
        app.zoom = 5.0;
        app.pan = egui::vec2(10.0, 10.0);

        // Same dimensions → zoom and pan survive the switch.
        app.current_index = 1;
        app.request_current();
        assert_eq!(app.zoom, 5.0);
        assert_eq!(app.pan, egui::vec2(10.0, 10.0));

        // Different dimensions → back to defaults.
        app.current_index = 2;
        app.request_current();
        assert_eq!(app.zoom, 1.0);
        assert_eq!(app.pan, egui::Vec2::ZERO);
    }

    #[test]
    fn zoom_reset_never_preserves_zoom() {
        let mut app = cached_app(&[("a.png", 100, 100), ("b.png", 200, 200)]);
        app.settings.zoom_reset = ZoomReset::Never;
        app.current_index = 0;
        app.request_current();
        app.zoom = 5.0;

        app.current_index = 1;
        app.request_current();
        assert_eq!(app.zoom, 5.0);
    }

    #[test]
    fn zoom_reset_always_resets_even_for_same_dimensions() {
        let mut app = cached_app(&[("a.png", 100, 100), ("b.png", 100, 100)]);
        app.settings.zoom_reset = ZoomReset::Always;
        app.current_index = 0;
        app.request_current();
        app.zoom = 5.0;

        app.current_index = 1;
        app.request_current();
        assert_eq!(app.zoom, 1.0);
    }

    #[test]
    fn dir_refresh_preserves_place_without_reload() {
        let mut app = cached_app(&[("a.png", 10, 10), ("b.png", 10, 10)]);
        app.current_index = 1; // b.png
        app.request_current();
        let target_before = app.target_path.clone();
        assert_eq!(target_before, Some(PathBuf::from("b.png")));

        // New file appears; listing arrives sorted differently.
        app.apply_dir_refresh(vec![
            PathBuf::from("b.png"),
            PathBuf::from("a.png"),
            PathBuf::from("c.png"),
        ]);

        assert_eq!(app.images.len(), 3);
        // The displayed image is still the target and still selected.
        assert_eq!(app.target_path, target_before);
        assert_eq!(app.images[app.current_index], PathBuf::from("b.png"));
        // No reload was issued for the unchanged image.
        assert!(!app.is_loading);
    }

    #[test]
    fn dir_refresh_reloads_when_file_changed_on_disk() {
        let mut app = cached_app(&[("a.png", 10, 10), ("b.png", 10, 10)]);
        app.current_index = 1;
        app.request_current();
        // Pretend the displayed file's mtime moved since it was loaded.
        app.current_mtime = Some(std::time::SystemTime::UNIX_EPOCH);

        // A new file appears alongside the changed one.
        app.apply_dir_refresh(vec![
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            PathBuf::from("c.png"),
        ]);

        // request_current re-issued the Active load for the same path.
        assert!(app.is_loading);
        assert_eq!(app.target_path, Some(PathBuf::from("b.png")));
        assert_eq!(app.images[app.current_index], PathBuf::from("b.png"));
    }

    #[test]
    fn dir_refresh_lands_on_nearest_image_when_current_vanishes() {
        let mut app = cached_app(&[("a.png", 10, 10), ("b.png", 10, 10)]);
        // An uncached third image so the landing actually issues a load.
        app.images.push(PathBuf::from("c.png"));
        app.current_index = 1; // b.png
        app.request_current();

        // b.png is deleted externally.
        app.apply_dir_refresh(vec![PathBuf::from("a.png"), PathBuf::from("c.png")]);

        // Index clamps onto the nearest surviving entry and it loads.
        assert_eq!(app.images[app.current_index], PathBuf::from("c.png"));
        assert_eq!(app.target_path, Some(PathBuf::from("c.png")));
        assert!(app.is_loading);
    }
}
