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

use crate::commands::Command;
use crate::render;

pub(crate) use settings::{AppSettings, SortMode, ZoomReset};

// Submodule Declarations
mod canvas;
pub(crate) mod cli;
mod clipboard;
mod config;
mod loader;
mod saver;
// Batch conversion engine (Roadmap.md §5) — driven by the Batch Convert window.
pub(crate) mod batch;
// Interactive editing ops + undo (Roadmap.md §4) — applied from the canvas,
// which re-uploads the result on the main thread.
pub(crate) mod edit;
// Exif tag rows behind the Image Information window (Roadmap §9).
pub(crate) mod exif;
// The shared operation vocabulary (Roadmap.md §1): one ImageOp enum for the
// transforms the editor and the batch chain both apply, with the Appendix-B
// text form that recipes and the CLI serialise to.
pub(crate) mod image_op;
// Headers-only metadata probe behind the Image Information window (Roadmap §9).
pub(crate) mod image_info;
pub(crate) mod settings;
// Dracula palette + theme setup; the palette consts are shared with the UI
// modules (sidebar, overlay).
pub(crate) mod theme;
// Fit/zoom geometry (Roadmap.md §3): `fit_zoom`/`zoom_limits`/`ZoomLock` are
// wired into the canvas, `zoom_percent_label` into the info bar,
// `image_pixel_at`/`html_hex` into the cursor pixel readout, `tile_offsets`
// into the tile-repeat view, and `FitMode` is user-selectable.
pub(crate) mod view;

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

/// Message from the batch-convert worker thread back to the main thread.
///
/// The worker runs the whole job list through [`batch::run_batch`] (CPU-parallel
/// via `rayon`) and sends exactly one message when every file is done, so the
/// render loop is never blocked on conversion.
pub(crate) enum BatchMessage {
    /// Files completed so far, for the determinate progress bar.
    Progress { done: usize, total: usize },
    /// Every job finished; one status per submitted job, in job order.
    Finished(Vec<batch::FileStatus>),
}

/// State for the interactive Batch Convert window (Roadmap.md §5). Empty and
/// closed by default; the conversion engine itself lives in [`batch`].
pub(crate) struct BatchState {
    /// Whether the window is shown.
    pub(crate) open: bool,
    /// The queued source files, in display order.
    pub(crate) files: Vec<PathBuf>,
    /// Target output format for every file in the run.
    pub(crate) format: batch::OutputFormat,
    /// Output folder chosen with the folder picker; `None` until picked.
    pub(crate) output_dir: Option<PathBuf>,
    /// Encoder settings for the run (JPEG quality, PNG compression).
    pub(crate) save_options: batch::SaveOptions,
    /// What to do when a target file already exists.
    pub(crate) overwrite: batch::OverwritePolicy,
    /// The operation chain applied to every file, in order.
    pub(crate) operations: Vec<image_op::ImageOp>,
    /// Per-file status rows, parallel to `files`. `None` before a run and for
    /// files the worker has not reported yet.
    pub(crate) statuses: Vec<Option<batch::FileStatus>>,
    /// Receiver for the background run's result; `Some` only while running.
    pub(crate) receiver: Option<Receiver<BatchMessage>>,
    /// The recipe field's text, kept in sync with `operations` whenever the
    /// chain changes through the buttons.
    pub(crate) recipe_text: String,
    /// True from the moment Run is clicked until the worker reports back.
    pub(crate) running: bool,
    /// `(done, total)` files for the running batch, for the progress bar.
    pub(crate) progress: (usize, usize),
    /// Last recipe-import failure, shown in the Convert window until a later
    /// import succeeds.
    pub(crate) recipe_error: Option<String>,
    /// UI scratch: index into the "add step" combo's choice list.
    pub(crate) pending_op: usize,
    /// UI scratch: pending resize width (`0` = keep aspect ratio).
    pub(crate) resize_width: u32,
    /// UI scratch: pending resize height (`0` = keep aspect ratio).
    pub(crate) resize_height: u32,
    /// UI scratch: pending resize kernel.
    pub(crate) resize_filter: image_op::ResampleFilter,
}

impl Default for BatchState {
    fn default() -> Self {
        Self {
            open: false,
            files: Vec::new(),
            format: batch::OutputFormat::Png,
            output_dir: None,
            save_options: batch::SaveOptions::default(),
            overwrite: batch::OverwritePolicy::Overwrite,
            operations: Vec::new(),
            statuses: Vec::new(),
            receiver: None,
            running: false,
            progress: (0, 0),
            recipe_text: String::new(),
            recipe_error: None,
            pending_op: 0,
            resize_width: 0,
            resize_height: 0,
            resize_filter: image_op::ResampleFilter::Lanczos3,
        }
    }
}

/// Image pixel under the cursor, sampled each frame for the status-bar
/// readout (`R,G,B  A  #RRGGBB`, click to copy the hex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoveredPixel {
    pub x: u32,
    pub y: u32,
    pub rgba: [u8; 4],
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
    /// Colour space of the displayed image (Roadmap §9): shown in the info bar
    /// and used when re-uploading an edit so a linear source stays linear.
    pub(crate) current_color_space: Option<crate::image_loader::ColorSpace>,
    /// True when the displayed image is a float/HDR source (tonemapped in the
    /// shader); shown in the info bar.
    pub(crate) current_hdr: bool,
    /// Image Information window state (Roadmap.md §9): open flag, the finished
    /// probe for the displayed target, and the probe still in flight.
    pub(crate) info_open: bool,
    pub(crate) image_info: Option<(PathBuf, Result<image_info::ImageInfo, String>)>,
    pub(crate) info_probe: Option<(PathBuf, Receiver<Result<image_info::ImageInfo, String>>)>,
    pub(crate) zoom: f32,
    pub(crate) pan: egui::Vec2,
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
    /// Pixel under the cursor in the image canvas, for the status-bar readout.
    /// `None` when the pointer is away from the image or no decoded bytes are
    /// available to sample (e.g. a cache-hit load).
    pub(crate) hovered_pixel: Option<HoveredPixel>,
    /// Batch Convert window state (Roadmap.md §5).
    pub(crate) batch: BatchState,
    /// Whole-image undo/redo history for interactive edits (Roadmap.md §4).
    /// Reset whenever the displayed target changes, so an edit history never
    /// leaks onto a different file.
    pub(crate) edit_undo: edit::UndoStack,
    /// Holds the current magnification across navigation (Roadmap.md §3).
    /// Gated by `settings.lock_zoom`; capturing the zoom is an explicit action.
    pub(crate) zoom_lock: view::ZoomLock,
    /// Set when a new image is displayed; the canvas consumes it to resize the
    /// OS window to the image per `settings.fit_window` (Roadmap.md §3).
    pub(crate) pending_window_fit: bool,
    /// Effective shortcut table: defaults with `settings.shortcut_overrides`
    /// applied; rebuilt when the rebinding UI changes a binding (Roadmap §10).
    pub(crate) bindings: Vec<(Command, Option<egui::KeyboardShortcut>)>,
    /// While `Some`, the next captured key rebinds this command instead of
    /// dispatching (Settings → Shortcuts).
    pub(crate) binding_capture: Option<Command>,
    /// Operation chain from the command line, applied once the first image is
    /// displayed (Roadmap §10); consumed by `cli::check_cli_launch`.
    pub(crate) cli_ops: Option<Vec<image_op::ImageOp>>,
    /// `--save` target from the command line, written once `cli_ops` have
    /// applied.
    pub(crate) cli_save_to: Option<PathBuf>,
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
            current_color_space: None,
            current_hdr: false,
            info_open: false,
            image_info: None,
            info_probe: None,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
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
            hovered_pixel: None,
            batch: BatchState::default(),
            edit_undo: edit::UndoStack::new(),
            zoom_lock: view::ZoomLock::new(),
            pending_window_fit: false,
            bindings: crate::commands::effective_bindings(&[]),
            binding_capture: None,
            cli_ops: None,
            cli_save_to: None,
        }
    }

    pub(crate) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
        // Clear selection when switching modes
        self.selection_image_rect = None;
        self.selection_start = None;
    }

    /// Enable or disable lock-zoom (Roadmap §3). Enabling captures the current
    /// magnification; disabling releases it. Shared by the Settings checkbox and
    /// the View menu so both stay in sync.
    pub(crate) fn set_zoom_lock(&mut self, locked: bool) {
        self.settings.lock_zoom = locked;
        if locked {
            self.zoom_lock.lock(self.zoom);
        } else {
            self.zoom_lock.unlock();
        }
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
            // Same list the directory walker uses, so the dialog and the
            // listing can't drift apart.
            .add_filter("Images", crate::dir_walker::SUPPORTED_EXTENSIONS)
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
