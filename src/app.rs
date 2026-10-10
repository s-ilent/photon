use eframe::egui;
use egui_wgpu::RenderState;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use wgpu::Device;

use crate::render;

// Submodule Declarations
mod canvas;
mod clipboard;
mod config;
mod loader;
mod saver;
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

/// Result sent from the loader worker thread back to the main thread.
pub enum LoadResult {
    Loaded {
        path: Option<PathBuf>,
        /// Fully GPU-resident image: every layer chunked into tiles, bind
        /// groups and vertex quads built (built on the worker, which owns
        /// the device/queue).
        image: Arc<render::RenderImage>,
        /// Set for `Paste` and `Active` loads so copy/save can reuse the
        /// decoded bytes; `None` for `Preload` so the bounded result channel
        /// doesn't pin several full-size buffers at once.
        rgba: Option<Vec<Vec<u8>>>,
    },
    Failed {
        path: Option<PathBuf>,
        /// `true` → `StatusOverlay::UnsupportedFormat`, `false` → `StatusOverlay::Error`.
        is_format_error: bool,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusOverlay {
    NoImages,
    Loading(String),
    Error(String),
    UnsupportedFormat(String),
}

/// Transient bottom-bar feedback for the background save encoder.
#[derive(Debug, Clone)]
pub struct SaveStatus {
    pub(crate) message: String,
    pub(crate) expires: Instant,
}

/// Labels for cubemap faces in DirectX file order (+X, -X, +Y, -Y, +Z, -Z).
///
/// (The texture cache stores `Arc<RenderImage>` directly — all metadata lives
/// on the RenderImage, and GPU lifetime is wgpu refcounting, so eviction is
/// just dropping the Arc.)
const CUBEMAP_FACE_LABELS: [&str; 6] = ["+X", "-X", "+Y", "-Y", "+Z", "-Z"];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// When the view resets (back to fit) on an image switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomReset {
    /// Reset on every switch.
    Always,
    /// Reset only when the new image's dimensions differ from the previous one.
    #[default]
    DimensionChange,
    /// Never reset; zoom and pan carry over between all images.
    Never,
}

impl ZoomReset {
    pub fn label(&self) -> &'static str {
        match self {
            ZoomReset::Always => "Always",
            ZoomReset::DimensionChange => "When dimensions change",
            ZoomReset::Never => "Never",
        }
    }
}

/// User-facing settings. Persisted to `config.ron` (see `app/config.rs`);
/// `#[serde(default)]` fills missing fields from `Default` so partial or
/// older config files stay loadable.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub dark_mode: bool,
    pub font_size: f32,
    pub scale_to_fit: bool,
    pub use_sharp_scaling: bool,
    pub checkerboard_bg: bool,
    /// Zoom-reset behavior on image switch; see `ZoomReset`.
    pub zoom_reset: ZoomReset,
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
            zoom_reset: ZoomReset::default(),
            sort_mode: SortMode::default(),
        }
    }
}

pub struct PhotonApp {
    pub(crate) images: Vec<PathBuf>,
    pub(crate) current_index: usize,
    pub(crate) texture_cache: LruCache<PathBuf, Arc<render::RenderImage>>,
    pub(crate) load_receiver: Option<Receiver<LoadResult>>,
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
    /// Pipeline + bind group layouts for the image canvas pass; built at
    /// startup against the surface format and shared with the loader worker.
    pub(crate) image_renderer: Option<Arc<render::ImageRendererState>>,
    /// The displayed image, GPU-resident. `None` while nothing is loaded.
    pub(crate) displayed_image: Option<Arc<render::RenderImage>>,
    pub(crate) settings: AppSettings,
    pub(crate) last_font_size: f32,
    pub(crate) settings_open: bool,
    pub(crate) is_pasted: bool,
    pub(crate) needs_fit: bool,
    pub(crate) dir_load_receiver: Option<Receiver<Vec<PathBuf>>>,
    /// Watcher-produced refreshed listings for the open directory.
    pub(crate) dir_watch_receiver: Option<Receiver<Vec<PathBuf>>>,
    /// Dropping this sender stops the directory watcher thread. Replaced on
    /// every `load_directory` so only the newest directory is watched.
    pub(crate) dir_watch_stop: Option<Sender<()>>,
    /// Last-seen modification time of the displayed image, used by the
    /// directory watcher to decide whether the file actually changed on disk.
    pub(crate) current_mtime: Option<std::time::SystemTime>,
    /// Which layer (cubemap face / array entry) of `displayed_image` is shown.
    pub(crate) current_layer: usize,
    /// True when the loaded layers are cubemap faces (drives face labels).
    pub(crate) layers_are_cubemap: bool,
    pub(crate) is_loading_dir: bool,
    pub(crate) scroll_to_target: bool,
    pub(crate) scroll_accumulator: f32,
    pub(crate) pasted_rgba: Option<Vec<u8>>,
    /// Decoded RGBA bytes of the image at `target_path`, one buffer per layer,
    /// set when its `Active` load completes. Lets copy/save work from memory
    /// instead of re-reading and re-decoding the file; cleared whenever the
    /// target changes.
    pub(crate) active_rgba: Option<Vec<Vec<u8>>>,
    /// Receiver for the result of the most recent background save encode.
    pub(crate) save_status_receiver: Option<Receiver<Result<String, String>>>,
    /// Last save outcome, shown in the bottom bar until it expires.
    pub(crate) save_status: Option<SaveStatus>,
    /// Settings as of the last config-file write; `None` before the first
    /// load. Compared each frame to detect changes worth persisting.
    pub(crate) saved_settings: Option<AppSettings>,
    /// Active error overlay for the currently targeted image. `None` when
    /// the current target is fine (loading, loaded, or no target). Set by
    /// `check_load_complete` when a `LoadResult::Failed` for the active
    /// target arrives; cleared by navigation / paste / dir load.
    pub(crate) error_overlay: Option<StatusOverlay>,
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
            image_renderer: None,
            displayed_image: None,
            settings: AppSettings::default(),
            last_font_size: 16.0,
            settings_open: false,
            is_pasted: false,
            needs_fit: false,
            dir_load_receiver: None,
            dir_watch_receiver: None,
            dir_watch_stop: None,
            current_mtime: None,
            current_layer: 0,
            layers_are_cubemap: false,
            is_loading_dir: false,
            scroll_to_target: false,
            scroll_accumulator: 0.0,
            pasted_rgba: None,
            active_rgba: None,
            save_status_receiver: None,
            save_status: None,
            saved_settings: None,
            error_overlay: None,
        }
    }

    pub(crate) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
        // Clear selection when switching modes
        self.selection = None;
        self.selection_start = None;
    }

    /// Display layer `index` of the loaded image (cubemap face / array entry).
    /// All layers share dimensions, so zoom and pan are untouched.
    pub(crate) fn select_layer(&mut self, index: usize) {
        let layers = self
            .displayed_image
            .as_ref()
            .map_or(0, |image| image.layers.len());
        if index >= layers {
            return;
        }
        self.current_layer = index;
    }

    /// Bottom-bar label for layer `index` of the displayed image: cubemap
    /// face names where known, 1-based numbers otherwise.
    pub(crate) fn layer_label(&self, index: usize) -> String {
        if self.layers_are_cubemap {
            CUBEMAP_FACE_LABELS
                .get(index)
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| (index + 1).to_string())
        } else {
            (index + 1).to_string()
        }
    }

    pub(crate) fn next_image(&mut self) {
        if self.images.is_empty() {
            return;
        }
        self.error_overlay = None;
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
        self.error_overlay = None;
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
                &[
                    "png", "jpg", "jpeg", "gif", "bmp", "tiff", "webp", "dds", "exr",
                ],
            )
            .pick_file();

        if let Some(path) = file {
            self.open_path(path);
        }
    }

    /// Open a file (loading its parent folder around it, like the Open
    /// dialog) or a folder directly. Also the drag-and-drop entry point.
    pub(crate) fn open_path(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.load_directory(&path);
        } else {
            self.is_pasted = false;
            self.target_path = Some(path.clone());
            if let Some(parent) = path.parent() {
                self.load_directory(parent);
            }
        }
    }
}
