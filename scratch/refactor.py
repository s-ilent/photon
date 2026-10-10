import sys

app_rs_path = "src/app.rs"
with open(app_rs_path, "r") as f:
    content = f.read()

# 1. Imports
content = content.replace(
    "use std::sync::mpsc::{channel, Receiver};",
    """use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::VecDeque;

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
    Terminate,
}

pub struct LocalCache {
    capacity: usize,
    entries: VecDeque<(PathBuf, egui::TextureId, u32, u32)>,
}

impl LocalCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: VecDeque::new(),
        }
    }
    
    fn get(&mut self, path: &PathBuf) -> Option<(egui::TextureId, u32, u32)> {
        if let Some(pos) = self.entries.iter().position(|e| &e.0 == path) {
            let entry = self.entries.remove(pos).unwrap();
            self.entries.push_back(entry.clone());
            Some((entry.1, entry.2, entry.3))
        } else {
            None
        }
    }
    
    fn contains(&self, path: &PathBuf) -> bool {
        self.entries.iter().any(|e| &e.0 == path)
    }

    fn put(&mut self, path: PathBuf, tex: egui::TextureId, w: u32, h: u32) -> Option<egui::TextureId> {
        let mut evicted = None;
        if let Some(pos) = self.entries.iter().position(|e| e.0 == path) {
            self.entries.remove(pos);
        } else if self.entries.len() >= self.capacity {
            evicted = self.entries.pop_front().map(|e| e.1);
        }
        self.entries.push_back((path, tex, w, h));
        evicted
    }
}"""
)

# 2. Struct struct PhotonApp
content = content.replace(
    """pub struct PhotonApp {
    pub(crate) images: Vec<PathBuf>,
    pub(crate) current_index: usize,
    pub(crate) texture: Option<egui::TextureHandle>,
    pub(crate) image_dimensions: Option<(u32, u32)>,
    pub(crate) pending_load: Option<PathBuf>,
    pub(crate) load_receiver: Option<Receiver<(TextureView, u32, u32)>>,""",
    """pub struct PhotonApp {
    pub(crate) images: Vec<PathBuf>,
    pub(crate) current_index: usize,
    pub(crate) texture_cache: LocalCache,
    pub(crate) load_receiver: Option<Receiver<(PathBuf, TextureView, u32, u32)>>,
    pub(crate) request_sender: Option<Sender<WorkerMessage>>,
    pub(crate) latest_req_id: Arc<AtomicU64>,
    pub(crate) sidebar_open: bool,
    pub(crate) is_loading: bool,
    pub(crate) target_path: Option<PathBuf>,
    pub(crate) image_dimensions: Option<(u32, u32)>,"""
)

# 3. PhotonApp::new()
content = content.replace(
    """    pub(crate) fn new() -> Self {
        Self {
            images: Vec::new(),
            current_index: 0,
            texture: None,
            image_dimensions: None,
            pending_load: None,
            load_receiver: None,""",
    """    pub(crate) fn new() -> Self {
        Self {
            images: Vec::new(),
            current_index: 0,
            texture_cache: LocalCache::new(5),
            load_receiver: None,
            request_sender: None,
            latest_req_id: Arc::new(AtomicU64::new(0)),
            sidebar_open: false,
            is_loading: false,
            target_path: None,
            image_dimensions: None,"""
)

# 4. load_custom_font -> add init_worker
init_worker_code = """
    pub(crate) fn init_worker(&mut self) {
        if self.request_sender.is_some() {
            return;
        }

        let device = self.device.clone().expect("wgpu device not initialized");
        let queue = self.queue.clone().expect("wgpu queue not initialized");

        let (req_tx, req_rx) = channel::<WorkerMessage>();
        let (res_tx, res_rx) = channel();

        self.request_sender = Some(req_tx);
        self.load_receiver = Some(res_rx);
        let latest_req_id = self.latest_req_id.clone();

        std::thread::spawn(move || {
            let mut pending_requests: VecDeque<LoadRequest> = VecDeque::new();
            
            loop {
                // Try blocking recv if queue is empty
                if pending_requests.is_empty() {
                    match req_rx.recv() {
                        Ok(WorkerMessage::Load(req)) => pending_requests.push_back(req),
                        Ok(WorkerMessage::Terminate) | Err(_) => break,
                    }
                }

                // Read all available without blocking
                while let Ok(msg) = req_rx.try_recv() {
                    match msg {
                        WorkerMessage::Load(req) => pending_requests.push_back(req),
                        WorkerMessage::Terminate => return,
                    }
                }

                let current_target_id = latest_req_id.load(Ordering::SeqCst);
                
                // Cleanup stale Active requests
                pending_requests.retain(|r| r.priority == Priority::Preload || r.request_id == current_target_id);

                let req_to_process = if let Some(idx) = pending_requests.iter().position(|r| r.priority == Priority::Active) {
                    pending_requests.remove(idx)
                } else {
                    pending_requests.pop_front()
                };

                let Some(req) = req_to_process else { continue };

                // Process request
                let start_total = Instant::now();
                
                let (rgba, width, height, stats) = match crate::image_loader::load_image(&req.path) {
                    Ok(data) => data,
                    Err(e) => {
                        println!("Failed to load image {:?}: {}", req.path, e);
                        continue;
                    }
                };

                let upload_start = Instant::now();
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("image_texture"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
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
                    &rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        rows_per_image: None,
                        bytes_per_row: Some(width * 4),
                    },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );

                let view = texture.create_view(&Default::default());
                let upload_time = upload_start.elapsed();
                
                println!("Worker loaded: {:?} (Priority: {:?}) | Total: {:?}", req.path.file_name(), req.priority == Priority::Active, start_total.elapsed());

                let _ = res_tx.send((req.path, view, width, height));
            }
        });
    }
"""

content = content.replace("        ctx.set_fonts(fonts);\n    }", "        ctx.set_fonts(fonts);\n    }\n" + init_worker_code)

# 5. refactor load_image
old_load_image = """    pub(crate) fn load_image(&mut self, path: PathBuf) {
        // Don't reload if already loading this image
        if self.pending_load.as_ref() == Some(&path) {
            return;
        }

        let device = self.device.clone().expect("wgpu device not initialized");
        let queue = self.queue.clone().expect("wgpu queue not initialized");

        let (tx, rx) = channel();
        self.pending_load = Some(path.clone());
        self.load_receiver = Some(rx);

        // Spawn background thread for decode + GPU transfer
        std::thread::spawn(move || {
            let start_total = Instant::now();
            
            // 1. Decode image (CPU)
            let (rgba, width, height, stats) = match image_loader::load_image(&path) {
                Ok(data) => data,
                Err(e) => {
                    println!("Failed to load image: {}", e);
                    return;
                }
            };
            let process_time = start_total.elapsed();

            // 2. Create wgpu texture on background (GPU)
            let upload_start = Instant::now();
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("image_texture"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            // 3. Write data to texture (GPU transfer)
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: Default::default(),
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    rows_per_image: None,
                    bytes_per_row: Some(width * 4),
                },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );

            // 4. Create view (fast)
            let view = texture.create_view(&Default::default());
            let upload_time = upload_start.elapsed();

            println!(
                "Image loaded: {:?} | IO: {:?} | Decode: {:?} | RGBA: {:?} | Upload: {:?} | Total: {:?}",
                path.file_name().unwrap_or_default(),
                stats.io_time,
                stats.decode_time,
                stats.conversion_time,
                upload_time,
                start_total.elapsed()
            );

            // Send ready-to-use view to main thread
            let _ = tx.send((view, width, height));
        });
    }"""

new_load_image = """    pub(crate) fn load_image(&mut self, path: PathBuf, priority: Priority) {
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
        if self.images.is_empty() { return; }
        
        let path = self.images[self.current_index].clone();
        self.target_path = Some(path.clone());
        
        if self.texture_cache.contains(&path) {
            // Already cached, just promote it and we're good
            let (tex_id, w, h) = self.texture_cache.get(&path).unwrap();
            self.image_texture_id = Some(tex_id);
            self.image_dimensions = Some((w, h));
            self.is_loading = false;
        } else {
            self.is_loading = true;
            self.load_image(path.clone(), Priority::Active);
        }
        
        // Preload next and prev
        if self.images.len() > 1 {
            let next_idx = (self.current_index + 1) % self.images.len();
            let prev_idx = if self.current_index == 0 { self.images.len() - 1 } else { self.current_index - 1 };
            
            let next_path = self.images[next_idx].clone();
            if !self.texture_cache.contains(&next_path) && Some(&next_path) != self.target_path.as_ref() {
                self.load_image(next_path, Priority::Preload);
            }
            
            let prev_path = self.images[prev_idx].clone();
            if !self.texture_cache.contains(&prev_path) && Some(&prev_path) != self.target_path.as_ref() {
                self.load_image(prev_path, Priority::Preload);
            }
        }
    }"""
content = content.replace(old_load_image, new_load_image)

# 6. check_load_complete
old_check = """    pub(crate) fn check_load_complete(&mut self, ctx: &egui::Context) {
        if let Some(ref mut receiver) = self.load_receiver {
            if let Ok((texture_view, width, height)) = receiver.try_recv() {
                // We have a pre-created TextureView ready
                // Register it with egui renderer
                if let Some(ref render_state) = self.render_state {
                    let device = &render_state.device;
                    let mut renderer = render_state.renderer.write();

                    if let Some(tex_id) = self.image_texture_id {
                        // Update existing texture
                        renderer.update_egui_texture_from_wgpu_texture(
                            device,
                            &texture_view,
                            FilterMode::Linear,
                            tex_id,
                        );
                    } else {
                        // First load - register new texture
                        let new_id = renderer.register_native_texture(
                            device,
                            &texture_view,
                            FilterMode::Linear,
                        );
                        self.image_texture_id = Some(new_id);
                    }

                    self.image_dimensions = Some((width, height));
                    self.pending_load = None;
                    self.load_receiver = None;
                    
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                    self.selection = None;
                    self.selection_image_rect = None;
                    self.selection_start = None;
                    
                    if self.settings.scale_to_fit {
                        self.needs_fit = true;
                    }
                }
            }
        }
    }"""

new_check = """    pub(crate) fn check_load_complete(&mut self, ctx: &egui::Context) {
        if let Some(ref mut receiver) = self.load_receiver {
            while let Ok((path, texture_view, width, height)) = receiver.try_recv() {
                if let Some(ref render_state) = self.render_state {
                    let device = &render_state.device;
                    let mut renderer = render_state.renderer.write();

                    let new_id = renderer.register_native_texture(
                        device,
                        &texture_view,
                        FilterMode::Linear,
                    );
                    
                    if let Some(evict_id) = self.texture_cache.put(path.clone(), new_id, width, height) {
                        // Never free the texture ID if it's currently on screen
                        if Some(evict_id) != self.image_texture_id {
                            renderer.free_texture(evict_id);
                        }
                    }

                    // If this was the image we are actively waiting for, show it
                    if self.target_path.as_ref() == Some(&path) {
                        self.image_texture_id = Some(new_id);
                        self.image_dimensions = Some((width, height));
                        self.is_loading = false;
                        
                        self.zoom = 1.0;
                        self.pan = egui::Vec2::ZERO;
                        self.selection = None;
                        self.selection_image_rect = None;
                        self.selection_start = None;
                        
                        if self.settings.scale_to_fit {
                            self.needs_fit = true;
                        }
                    }
                    
                    ctx.request_repaint();
                }
            }
        }
    }"""
content = content.replace(old_check, new_check)

# 7. next, prev, load_directory
content = content.replace(
    """    pub(crate) fn load_directory(&mut self, path: &Path) {
        let walker = dir_walker::DirWalker::new(path);
        self.images = walker.map(|w| w.files).unwrap_or_default();
        self.images.sort();
        self.current_index = 0;
    }

    pub(crate) fn next_image(&mut self) {
        if self.images.is_empty() || self.pending_load.is_some() {
            return;
        }
        self.current_index = (self.current_index + 1) % self.images.len();

        if let Some(path) = self.images.get(self.current_index).cloned() {
            self.load_image(path);
        }
    }

    pub(crate) fn prev_image(&mut self) {
        if self.images.is_empty() || self.pending_load.is_some() {
            return;
        }
        if self.current_index == 0 {
            self.current_index = self.images.len() - 1;
        } else {
            self.current_index -= 1;
        }

        if let Some(path) = self.images.get(self.current_index).cloned() {
            self.load_image(path);
        }
    }""",
    """    pub(crate) fn load_directory(&mut self, path: &Path) {
        let walker = dir_walker::DirWalker::new(path);
        self.images = walker.map(|w| w.files).unwrap_or_default();
        self.images.sort();
        self.current_index = 0;
        self.request_current();
    }

    pub(crate) fn next_image(&mut self) {
        if self.images.is_empty() { return; }
        self.current_index = (self.current_index + 1) % self.images.len();
        self.request_current();
    }

    pub(crate) fn prev_image(&mut self) {
        if self.images.is_empty() { return; }
        if self.current_index == 0 {
            self.current_index = self.images.len() - 1;
        } else {
            self.current_index -= 1;
        }
        self.request_current();
    }"""
)

# 8. open_file Fix: replace self.load_image(path) with request_current()
content = content.replace(
    """                // Load the selected image
                if let Some(path) = self.images.get(self.current_index).cloned() {
                    self.load_image(path);
                }""",
    """                // Load the selected image
                self.request_current();"""
)

# 9. Workspace loading spin
workspace_old = """        // Draw the image
        painter.image(
            tex_id,
            image_screen_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );"""
workspace_new = """        // Draw the image
        painter.image(
            tex_id,
            image_screen_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            if self.is_loading { egui::Color32::from_gray(128) } else { egui::Color32::WHITE },
        );

        if self.is_loading {
            let center = image_screen_rect.center();
            ui.put(egui::Rect::from_center_size(center, egui::vec2(300.0, 50.0)), |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    if let Some(p) = &self.target_path {
                        ui.label(format!("Loading {}...", p.file_name().unwrap_or_default().to_string_lossy()));
                    } else {
                        ui.label("Loading...");
                    }
                }).response
            });
        }"""
content = content.replace(workspace_old, workspace_new)

with open(app_rs_path, "w") as f:
    f.write(content)

print("app.rs patched")
