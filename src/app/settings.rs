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
    /// How an image is fitted to the window on load / re-fit; see `FitMode`.
    pub fit_mode: crate::app::view::FitMode,
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
            fit_mode: crate::app::view::FitMode::ReduceOnly,
            sort_mode: SortMode::default(),
        }
    }
}
