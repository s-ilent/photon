use crate::app::{
    LoadRequest, LoadResult, PhotonApp, Priority, SortMode, StatusOverlay, WorkerMessage,
};
use crate::dir_walker;
use eframe::egui;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, sync_channel};
use std::time::Instant;
use wgpu::FilterMode;

impl PhotonApp {
    pub(crate) fn init_worker(&mut self) {
        if self.request_sender.is_some() {
            return;
        }

        let device = self.device.clone().expect("wgpu device not initialized");
        let queue = self.queue.clone().expect("wgpu queue not initialized");

        let (req_tx, req_rx) = channel::<WorkerMessage>();
        let (res_tx, res_rx) = sync_channel(5);

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

                let (rgba_for_texture, width, height, path_for_tx, rgba_for_res, is_active_load) =
                    match msg_to_process {
                        Some(WorkerMessage::Load(ref req)) => {
                            match crate::image_loader::load_image(&req.path) {
                                Ok((data, w, h, _)) => {
                                    let active = req.priority == Priority::Active;
                                    (data, w, h, Some(req.path.clone()), None, active)
                                }
                                Err(e) => {
                                    // Surface the error to the main thread instead of swallowing it.
                                    // Classify by the prefix of the string error from image_loader:
                                    //   "Failed to decode image" → UnsupportedFormat
                                    //   anything else (open/guess) → Error
                                    // The fallback is Error so an unknown prefix yields the more
                                    // general overlay — never a misleadingly specific one.
                                    let is_format_error = e.starts_with("Failed to decode");
                                    println!("Failed to load image {:?}: {}", req.path, e);
                                    let _ = res_tx.send(LoadResult::Failed {
                                        path: Some(req.path.clone()),
                                        is_format_error,
                                        message: e,
                                    });
                                    continue;
                                }
                            }
                        }
                        Some(WorkerMessage::Paste {
                            rgba,
                            width,
                            height,
                        }) => (rgba.clone(), width, height, None, Some(rgba), true),
                        None => continue,
                    };

                // Reject images the GPU can't represent before touching wgpu.
                // An oversized texture used to sail through as Loaded and only
                // fail asynchronously in wgpu validation, leaving the UI
                // spinning forever with no error surfaced.
                let max_dim = device.limits().max_texture_dimension_2d;
                if width == 0 || height == 0 || width > max_dim || height > max_dim {
                    let reason = if width == 0 || height == 0 {
                        format!("Image has zero extent ({}x{})", width, height)
                    } else {
                        format!(
                            "Image is {}x{}, exceeding the GPU texture limit of {}px",
                            width, height, max_dim
                        )
                    };
                    println!("Failed to load image {:?}: {}", path_for_tx, reason);
                    let _ = res_tx.send(LoadResult::Failed {
                        path: path_for_tx,
                        is_format_error: false,
                        message: reason,
                    });
                    continue;
                }

                let upload_start = Instant::now();
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("image_texture"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: Default::default(),
                    },
                    &rgba_for_texture,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        rows_per_image: None,
                        bytes_per_row: Some(width * 4),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );

                let view = texture.create_view(&Default::default());
                let _upload_time = upload_start.elapsed();

                // Hand the decoded bytes back with Active/Paste results so
                // copy/save can reuse them without re-reading the file. The
                // texture upload only borrows, so the buffer moves for free.
                let rgba_for_res =
                    rgba_for_res.or_else(|| is_active_load.then_some(rgba_for_texture));

                if let Some(ref path) = path_for_tx {
                    println!(
                        "Worker loaded: {:?} | Total: {:?}",
                        path.file_name(),
                        start_total.elapsed()
                    );
                } else {
                    println!(
                        "Worker created pasted texture | Total: {:?}",
                        start_total.elapsed()
                    );
                }

                // This will block if res_rx is full (sync_channel(5))
                let _ = res_tx.send(LoadResult::Loaded {
                    path: path_for_tx,
                    view,
                    width,
                    height,
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
        // refreshed when the new target's Active load completes.
        if self.target_path.as_ref() != Some(&path) {
            self.active_rgba = None;
        }

        // 1. Check if already in cache and fully loaded
        if let Some((id, w, h)) = self.texture_cache.get(&path).cloned() {
            // Compare against the *previous* image's dimensions before they
            // are replaced below: zoom only resets when the new image is a
            // different size (and keep_zoom doesn't opt out entirely).
            let dimensions_changed = self.image_dimensions != Some((w, h));
            self.image_texture_id = Some(id);
            self.image_dimensions = Some((w, h));
            self.target_path = Some(path.clone());
            self.is_loading = false;
            self.is_pasted = false;

            // Reset zoom on cached loads if keep_zoom is disabled
            if !self.settings.keep_zoom && dimensions_changed {
                self.zoom = 1.0;
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
                    LoadResult::Loaded {
                        path,
                        view: texture_view,
                        width,
                        height,
                        rgba,
                    } => {
                        if let Some(ref render_state) = self.render_state {
                            let device = &render_state.device;
                            let mut renderer = render_state.renderer.write();

                            let effective_path =
                                path.clone().unwrap_or_else(|| PathBuf::from("__pasted__"));

                            // 1. Check if we already have this in cache from another load
                            if let Some((old_id, _, _)) = self.texture_cache.get(&effective_path) {
                                if Some(*old_id) != self.image_texture_id {
                                    let oid = *old_id;
                                    renderer.free_texture(&oid);
                                }
                            }

                            // 2. Register the new texture
                            let new_id = renderer
                                .register_native_texture(device, &texture_view, FilterMode::Linear);

                            // 3. Put in cache and handle eviction correctly
                            if self.texture_cache.len() == self.texture_cache.cap().into() {
                                // Manually pop to avoid leaking the GPU resource
                                if let Some((_, (evict_id, _, _))) = self.texture_cache.pop_lru() {
                                    // Only free if it's not the one currently on screen
                                    if Some(evict_id) != self.image_texture_id {
                                        renderer.free_texture(&evict_id);
                                    }
                                }
                            }
                            self.texture_cache
                                .put(effective_path.clone(), (new_id, width, height));

                            // 4. Update display if this was the active target
                            let is_active = if let Some(ref p) = path {
                                self.target_path.as_ref() == Some(p)
                            } else {
                                self.is_pasted
                            };

                            if is_active {
                                // 5. Cleanup the OLD displayed texture if it's no longer in cache
                                if let Some(old_id) = self.image_texture_id {
                                    let mut in_cache = false;
                                    for (_, (id, _, _)) in self.texture_cache.iter() {
                                        if *id == old_id {
                                            in_cache = true;
                                            break;
                                        }
                                    }
                                    if !in_cache {
                                        renderer.free_texture(&old_id);
                                    }
                                }

                                let dimensions_changed =
                                    self.image_dimensions != Some((width, height));
                                self.image_texture_id = Some(new_id);
                                self.image_dimensions = Some((width, height));
                                self.is_loading = false;
                                self.error_overlay = None;

                                if !self.settings.keep_zoom && dimensions_changed {
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
                                    self.pasted_rgba = rgba;
                                } else {
                                    self.pasted_rgba = None;
                                    self.active_rgba = rgba;
                                }

                                if self.settings.scale_to_fit {
                                    self.needs_fit = true;
                                }
                            } else if self.is_loading {
                                // Display the progressive intermediate image, keeping is_loading active
                                self.image_texture_id = Some(new_id);
                                self.image_dimensions = Some((width, height));
                            }

                            ctx.request_repaint();
                        }
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
                            println!("Stale preload failed for {:?}: {}", path, message);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn load_directory(&mut self, path: &Path) {
        // Clear any stale error from the previous folder before starting anew.
        self.error_overlay = None;

        // Clear all old textures to prevent leaks
        if let Some(ref render_state) = self.render_state {
            let mut renderer = render_state.renderer.write();

            // 1. Free current image
            if let Some(id) = self.image_texture_id {
                renderer.free_texture(&id);
            }

            // 2. Free everything in cache
            while let Some((_, (id, _, _))) = self.texture_cache.pop_lru() {
                renderer.free_texture(&id);
            }
        }

        let (tx, rx) = channel();
        self.dir_load_receiver = Some(rx);
        self.is_loading_dir = true;
        self.images.clear();
        self.current_index = 0;
        self.image_texture_id = None;
        self.is_pasted = false;

        let path_buf = path.to_path_buf();
        std::thread::spawn(move || {
            let walker = dir_walker::DirWalker::new(&path_buf);
            let mut files = walker.map(|w| w.files).unwrap_or_default();
            files.sort();
            let _ = tx.send(files);
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
    use crate::app::PhotonApp;

    /// App with every image already in the texture cache, so `request_current`
    /// exercises the cached-load path without a worker or GPU device.
    fn cached_app(dims: &[(&str, u32, u32)]) -> PhotonApp {
        let mut app = PhotonApp::new();
        app.settings.keep_zoom = false;
        app.settings.scale_to_fit = false;
        for (name, w, h) in dims {
            app.texture_cache
                .put(PathBuf::from(name), (egui::TextureId::User(0), *w, *h));
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
    fn keep_zoom_survives_dimension_change() {
        let mut app = cached_app(&[("a.png", 100, 100), ("b.png", 200, 200)]);
        app.settings.keep_zoom = true;
        app.current_index = 0;
        app.request_current();
        app.zoom = 5.0;

        app.current_index = 1;
        app.request_current();
        assert_eq!(app.zoom, 5.0);
    }
}
