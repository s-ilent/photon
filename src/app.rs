use eframe::egui;
use egui_wgpu::RenderState;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use wgpu::Device;

// Submodule Declarations
mod canvas;
mod clipboard;
mod loader;
mod theme;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusOverlay {
    NoImages,
    Loading(String),
    Error(String),
    UnsupportedFormat(String),
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    Alphabetical,
    AlphabeticalDesc,
    #[default]
    DateModified,
    DateModifiedOldest,
    FileSize,
    FileSizeSmallest,
}

impl SortMode {
    pub fn label(&self) -> &'static str {
        match self {
            SortMode::Alphabetical => "Name (A-Z)",
            SortMode::AlphabeticalDesc => "Name (Z-A)",
            SortMode::DateModified => "Date (Newest)",
            SortMode::DateModifiedOldest => "Date (Oldest)",
            SortMode::FileSize => "Size (Largest)",
            SortMode::FileSizeSmallest => "Size (Smallest)",
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
    pub keep_zoom: bool,
    pub sort_mode: SortMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            font_size: 16.0,
            scale_to_fit: true,
            use_sharp_scaling: false,
            checkerboard_bg: true,
            keep_zoom: false,
            sort_mode: SortMode::default(),
        }
    }
}

pub struct PhotonApp {
    pub(crate) images: Vec<PathBuf>,
    pub(crate) current_index: usize,
    pub(crate) texture_cache: LruCache<PathBuf, (egui::TextureId, u32, u32)>,
    pub(crate) load_receiver: Option<
        Receiver<(
            Option<PathBuf>,
            wgpu::TextureView,
            u32,
            u32,
            Option<Vec<u8>>,
        )>,
    >,
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
    pub(crate) scroll_accumulator: f32,
    pub(crate) pasted_rgba: Option<Vec<u8>>,
}

impl PhotonApp {
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
            scroll_accumulator: 0.0,
            pasted_rgba: None,
        }
    }

    pub(crate) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
        // Clear selection when switching modes
        self.selection = None;
        self.selection_start = None;
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
}
