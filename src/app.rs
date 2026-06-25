use arboard::Clipboard;
use eframe::egui;
use egui_wgpu::RenderState;
use lru::LruCache;
use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use wgpu::{Device, FilterMode, TextureView};

#[derive(Clone, PartialEq, Eq)]
pub enum Priority {
    Active,
    Preload,
}

pub struct LoadRequest {
    pub path: PathBuf,
    pub priority: Priority,
    pub request_id: u64,
}

pub enum WorkerMessage {
    Load(LoadRequest),
    Paste {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

use crate::dir_walker;
use crate::image_loader;

/// Mouse modes that change how input is interpreted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseMode {
    #[default]
    Browse, // Default: pan + zoom
    Navigate, // Mouse wheel = navigation
    Select,   // Box selection + zoom
}

impl MouseMode {
    pub fn next(&self) -> Self {
        match self {
            MouseMode::Browse => MouseMode::Navigate,
            MouseMode::Navigate => MouseMode::Select,
            MouseMode::Select => MouseMode::Browse,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            MouseMode::Browse => "Browse",
            MouseMode::Navigate => "Navigate",
            MouseMode::Select => "Select",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub dark_mode: bool,
    pub font_size: f32,
    pub scale_to_fit: bool,
    pub use_sharp_scaling: bool,
    pub checkerboard_bg: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            font_size: 16.0,
            scale_to_fit: true,
            use_sharp_scaling: false,
            checkerboard_bg: true,
        }
    }
}

pub struct PhotonApp {
    pub(crate) images: Vec<PathBuf>,
    pub(crate) current_index: usize,
    pub(crate) texture_cache: LruCache<PathBuf, (egui::TextureId, u32, u32)>,
    pub(crate) load_receiver:
        Option<Receiver<(Option<PathBuf>, TextureView, u32, u32, Option<Vec<u8>>)>>,
    pub(crate) request_sender: Option<Sender<WorkerMessage>>,
    pub(crate) latest_req_id: Arc<AtomicU64>,
    pub(crate) sidebar_open: bool,
    pub(crate) is_loading: bool,
    pub(crate) target_path: Option<PathBuf>,
    pub(crate) image_dimensions: Option<(u32, u32)>,
    pub(crate) zoom: f32,
    pub(crate) pan: egui::Vec2,
    pub(crate) selection: Option<egui::Rect>,
    pub(crate) selection_image_rect: Option<egui::Rect>,
    pub(crate) selection_start: Option<egui::Pos2>,
    pub(crate) mode: MouseMode,
    pub(crate) last_mouse_pos: egui::Pos2,
    pub(crate) device: Option<Device>,
    pub(crate) queue: Option<Arc<wgpu::Queue>>,
    pub(crate) render_state: Option<Arc<RenderState>>,
    pub(crate) image_texture_id: Option<egui::TextureId>,
    pub(crate) settings: AppSettings,
    pub(crate) last_font_size: f32,
    pub(crate) settings_open: bool,
    pub(crate) is_pasted: bool,
    pub(crate) needs_fit: bool,
    pub(crate) dir_load_receiver: Option<Receiver<Vec<PathBuf>>>,
    pub(crate) is_loading_dir: bool,
    pub(crate) scroll_to_target: bool,
    pub(crate) pasted_rgba: Option<Vec<u8>>,
}

impl PhotonApp {
    pub(crate) fn ui_status_overlay(&self, ctx: &egui::Context, text: &str) {
        egui::Area::new(egui::Id::new("status_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::widgets::Spinner::new().size(32.0));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(text).strong().size(18.0));
                });
            });
    }

    pub(crate) fn new() -> Self {
        Self {
            images: Vec::new(),
            current_index: 0,
            texture_cache: LruCache::new(NonZeroUsize::new(24).unwrap()),
            load_receiver: None,
            request_sender: None,
            latest_req_id: Arc::new(AtomicU64::new(0)),
            sidebar_open: false,
            is_loading: false,
            target_path: None,
            image_dimensions: None,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            selection: None,
            selection_image_rect: None,
            selection_start: None,
            mode: MouseMode::Browse,
            last_mouse_pos: egui::Pos2::ZERO,
            device: None,
            queue: None,
            render_state: None,
            image_texture_id: None,
            settings: AppSettings::default(),
            last_font_size: 16.0,
            settings_open: false,
            is_pasted: false,
            needs_fit: false,
            dir_load_receiver: None,
            is_loading_dir: false,
            scroll_to_target: false,
            pasted_rgba: None,
        }
    }

    pub(crate) fn setup_dracula_theme(ctx: &egui::Context, font_size: f32) {
        let mut visuals = egui::Visuals::dark();
        let bg_color = egui::Color32::from_rgb(40, 42, 54);
        let fg_color = egui::Color32::from_rgb(248, 248, 242);
        let current_line = egui::Color32::from_rgb(68, 71, 90);
        let purple = egui::Color32::from_rgb(189, 147, 249);

        visuals.widgets.noninteractive.bg_fill = bg_color;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, fg_color);
        visuals.widgets.inactive.bg_fill = current_line;
        visuals.widgets.hovered.bg_fill = purple;
        visuals.window_fill = bg_color;
        visuals.panel_fill = bg_color;

        ctx.set_visuals(visuals);

        let mut style = (*ctx.global_style()).clone();
        for text_style in [
            egui::TextStyle::Body,
            egui::TextStyle::Monospace,
            egui::TextStyle::Button,
            egui::TextStyle::Heading,
        ] {
            if let Some(font_id) = style.text_styles.get_mut(&text_style) {
                font_id.size = font_size;
            }
        }
        ctx.set_global_style(style);
    }

    pub(crate) fn load_custom_font(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "custom_font".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../font.ttf"))),
        );

        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            vec.insert(0, "custom_font".to_owned());
        }

        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            vec.insert(0, "custom_font".to_owned());
        }

        ctx.set_fonts(fonts);
    }

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

                let (rgba_for_texture, width, height, path_for_tx, rgba_for_res) =
                    match msg_to_process {
                        Some(WorkerMessage::Load(req)) => {
                            match crate::image_loader::load_image(&req.path) {
                                Ok((data, w, h, _)) => (data, w, h, Some(req.path), None),
                                Err(e) => {
                                    println!("Failed to load image {:?}: {}", req.path, e);
                                    continue;
                                }
                            }
                        }
                        Some(WorkerMessage::Paste {
                            rgba,
                            width,
                            height,
                        }) => (rgba.clone(), width, height, None, Some(rgba)),
                        None => continue,
                    };

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
                let _ = res_tx.send((path_for_tx, view, width, height, rgba_for_res));
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

        let path = self.images[self.current_index].clone();

        // 1. Check if already in cache and fully loaded
        if let Some((id, w, h)) = self.texture_cache.get(&path).cloned() {
            self.image_texture_id = Some(id);
            self.image_dimensions = Some((w, h));
            self.target_path = Some(path.clone());
            self.is_loading = false;
            self.is_pasted = false;
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
            while let Ok((path, texture_view, width, height, rgba)) = receiver.try_recv() {
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
                    let new_id =
                        renderer.register_native_texture(device, &texture_view, FilterMode::Linear);

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

                        self.image_texture_id = Some(new_id);
                        self.image_dimensions = Some((width, height));
                        self.is_loading = false;

                        self.zoom = 1.0;
                        self.pan = egui::Vec2::ZERO;
                        self.selection = None;
                        self.selection_image_rect = None;
                        self.selection_start = None;

                        if path.is_none() {
                            self.pasted_rgba = rgba;
                        } else {
                            self.pasted_rgba = None;
                        }

                        if self.settings.scale_to_fit {
                            self.needs_fit = true;
                        }
                    }

                    ctx.request_repaint();
                }
            }
        }
    }

    pub(crate) fn load_directory(&mut self, path: &Path) {
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

    pub(crate) fn next_image(&mut self) {
        if self.images.is_empty() {
            return;
        }
        if self.is_pasted {
            self.is_pasted = false;
        }
        self.current_index = (self.current_index + 1) % self.images.len();
        self.request_current();
    }

    pub(crate) fn prev_image(&mut self) {
        if self.images.is_empty() {
            return;
        }
        if self.is_pasted {
            self.is_pasted = false;
            self.request_current(); // Return to the pre-paste image
        } else {
            if self.current_index == 0 {
                self.current_index = self.images.len() - 1;
            } else {
                self.current_index -= 1;
            }
            self.request_current();
        }
    }

    pub(crate) fn open_file(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "gif", "bmp", "tiff", "webp"],
            )
            .pick_file();

        if let Some(path) = file {
            self.is_pasted = false;
            self.target_path = Some(path.clone());
            if let Some(parent) = path.parent() {
                self.load_directory(parent);
            }
        }
    }

    pub(crate) fn copy_to_clipboard(&self) {
        let (rgba, width, height) = if self.is_pasted {
            let Some(rgba) = &self.pasted_rgba else {
                return;
            };
            let Some((w, h)) = self.image_dimensions else {
                return;
            };
            (rgba.clone(), w, h)
        } else {
            let path = match self.images.get(self.current_index) {
                Some(p) => p,
                None => return,
            };

            match image_loader::load_image(path) {
                Ok(data) => (data.0, data.1, data.2),
                Err(e) => {
                    println!("Failed to load image: {}", e);
                    return;
                }
            }
        };

        let (img_width, img_height) = (width as usize, height as usize);

        // If there's a selection in image-space, crop to it
        let (final_rgba, final_width, final_height) = if let Some(sel) = &self.selection_image_rect
        {
            let crop_x = sel.min.x.max(0.0) as usize;
            let crop_y = sel.min.y.max(0.0) as usize;
            let crop_w = sel.width() as usize;
            let crop_h = sel.height() as usize;

            let x = crop_x.min(img_width);
            let y = crop_y.min(img_height);
            let w = crop_w.min(img_width.saturating_sub(x));
            let h = crop_h.min(img_height.saturating_sub(y));

            if w == 0 || h == 0 {
                (rgba, img_width, img_height)
            } else {
                let mut cropped = Vec::with_capacity(w * h * 4);
                for row in y..(y + h) {
                    for col in x..(x + w) {
                        let idx = (row * img_width + col) * 4;
                        cropped.push(rgba[idx]);
                        cropped.push(rgba[idx + 1]);
                        cropped.push(rgba[idx + 2]);
                        cropped.push(rgba[idx + 3]);
                    }
                }
                (cropped, w, h)
            }
        } else {
            (rgba, img_width, img_height)
        };

        // Copy to clipboard
        std::thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    println!("Clipboard error: {}", e);
                    return;
                }
            };

            let img_data = arboard::ImageData {
                width: final_width as usize,
                height: final_height,
                bytes: final_rgba.into(),
            };

            if let Err(e) = clipboard.set_image(img_data) {
                println!("Failed to set clipboard: {}", e);
            }
        });
    }

    pub(crate) fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                println!("Clipboard error: {}", e);
                return;
            }
        };

        match clipboard.get_image() {
            Ok(img) => {
                let width = img.width as u32;
                let height = img.height as u32;
                let rgba = img.bytes.to_vec();

                if let Some(sender) = &self.request_sender {
                    self.is_pasted = true;
                    self.is_loading = true;
                    self.target_path = None; // Important so check_load_complete knows it's the paste
                    let _ = sender.send(WorkerMessage::Paste {
                        rgba,
                        width,
                        height,
                    });
                    ctx.request_repaint();
                }
            }
            Err(e) => {
                println!("No image in clipboard or failed to get image: {}", e);
            }
        }
    }

    pub(crate) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
        // Clear selection when switching modes
        self.selection = None;
        self.selection_start = None;
    }

    pub(crate) fn draw_image_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(tex_id) = self.image_texture_id else {
            self.ui_status_overlay(ui.ctx(), "No images loaded.");
            return;
        };

        let Some((img_w, img_h)) = self.image_dimensions else {
            self.ui_status_overlay(ui.ctx(), "No images loaded.");
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
            self.ui_status_overlay(ui.ctx(), &text);
        }

        // Calculate scale factors for image <-> screen conversion
        let scale_x = image_size.x / texture_size.x;
        let scale_y = image_size.y / texture_size.y;

        // Handle input based on mode
        let mouse_pos = response.interact_pointer_pos();

        // Scroll wheel - zoom (smooth and sensitive)
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 && response.hovered() {
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
            // delta_pan = -(zoom_center - image_center) * (new_zoom / old_zoom - 1)
            let center_dist = zoom_center - image_screen_rect.center();
            self.pan -= center_dist * (new_zoom / old_zoom - 1.0);
            self.zoom = new_zoom;

            // Update selection in image-space when zooming
            if let Some(sel) = self.selection {
                let new_sel = egui::Rect::from_min_size(
                    egui::pos2(
                        (sel.min.x - response.rect.min.x - center_offset.x - self.pan.x) / scale_x,
                        (sel.min.y - response.rect.min.y - center_offset.y - self.pan.y) / scale_y,
                    ),
                    egui::vec2(sel.width() / scale_x, sel.height() / scale_y),
                );
                self.selection_image_rect = Some(new_sel);
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
