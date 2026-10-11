//! Fit/zoom geometry for the image canvas (Roadmap.md §3).
//!
//! Pure functions over viewport and image sizes — no egui context, no state.
//! Wired into `app/canvas.rs` and the Settings UI in a follow-up step.

use eframe::egui;

/// Lower bound of the interactive zoom range.
pub(crate) const ZOOM_MIN: f32 = 0.1;
/// Upper bound of the interactive zoom range.
pub(crate) const ZOOM_MAX: f32 = 50.0;

/// How an image is fitted to the available work area when it loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FitMode {
    /// Never fit; show at 100%.
    DontFit,
    /// Fit to the window, but never upscale past 100%.
    #[default]
    ReduceOnly,
    /// Always fit to the window, upscaling small images.
    AlwaysFit,
}

impl FitMode {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            FitMode::DontFit => "Don't fit",
            FitMode::ReduceOnly => "Reduce only",
            FitMode::AlwaysFit => "Always fit",
        }
    }
}

/// The zoom scalar that fits `image` inside `available` under `mode`.
///
/// Degenerate sizes (zero or negative on either axis) fall back to `1.0` so a
/// not-yet-known image size can never produce an infinite or NaN zoom.
pub(crate) fn fit_zoom(available: egui::Vec2, image: egui::Vec2, mode: FitMode) -> f32 {
    if available.x <= 0.0 || available.y <= 0.0 || image.x <= 0.0 || image.y <= 0.0 {
        return 1.0;
    }
    let scale = (available.x / image.x).min(available.y / image.y);
    match mode {
        FitMode::DontFit => 1.0,
        FitMode::ReduceOnly => scale.min(1.0),
        FitMode::AlwaysFit => scale,
    }
}

/// Clamp `z` into the interactive zoom range.
pub(crate) fn clamp_zoom(z: f32) -> f32 {
    z.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Zoom that fills the work area: the larger of the two axis scales, clamped.
///
/// Used by "zoom to desktop / fit window" commands, which enlarge a small image
/// until one axis touches the viewport (the other overflows, panning covers it).
pub(crate) fn zoom_to_fill(available: egui::Vec2, image: egui::Vec2) -> f32 {
    if available.x <= 0.0 || available.y <= 0.0 || image.x <= 0.0 || image.y <= 0.0 {
        return 1.0;
    }
    clamp_zoom((available.x / image.x).max(available.y / image.y))
}

/// Human-readable zoom percentage, e.g. `100%`.
pub(crate) fn zoom_percent_label(z: f32) -> String {
    format!("{}%", (z * 100.0).round() as i32)
}

/// "Lock zoom across navigation" primitive.
///
/// While locked, [`ZoomLock::apply`] returns the captured zoom instead of the
/// caller's freshly computed one, so paging between images keeps the same
/// magnification regardless of their dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZoomLock {
    locked: bool,
    zoom: f32,
}

impl ZoomLock {
    pub(crate) fn new() -> Self {
        Self {
            locked: false,
            zoom: 1.0,
        }
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    /// Capture `zoom` and start holding it across navigation.
    pub(crate) fn lock(&mut self, zoom: f32) {
        self.locked = true;
        self.zoom = zoom;
    }

    pub(crate) fn unlock(&mut self) {
        self.locked = false;
    }

    /// The zoom to use: the held value while locked, else `next_zoom`.
    pub(crate) fn apply(&self, next_zoom: f32) -> f32 {
        if self.locked {
            self.zoom
        } else {
            next_zoom
        }
    }
}

impl Default for ZoomLock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> egui::Vec2 {
        egui::vec2(x, y)
    }

    #[test]
    fn fit_mode_default_is_reduce_only() {
        assert_eq!(FitMode::default(), FitMode::ReduceOnly);
    }

    #[test]
    fn dont_fit_is_always_100_percent() {
        // Smaller-than-window image.
        assert_eq!(fit_zoom(v(100.0, 100.0), v(50.0, 50.0), FitMode::DontFit), 1.0);
        // Larger-than-window image.
        assert_eq!(fit_zoom(v(100.0, 100.0), v(400.0, 200.0), FitMode::DontFit), 1.0);
    }

    #[test]
    fn reduce_only_shrinks_large_images() {
        // 400x200 into 100x100: limiting axis gives 0.25.
        assert_eq!(
            fit_zoom(v(100.0, 100.0), v(400.0, 200.0), FitMode::ReduceOnly),
            0.25
        );
    }

    #[test]
    fn reduce_only_never_upscales() {
        // 50x50 into 100x100 would be 2.0, clamped back to 1.0.
        assert_eq!(
            fit_zoom(v(100.0, 100.0), v(50.0, 50.0), FitMode::ReduceOnly),
            1.0
        );
    }

    #[test]
    fn always_fit_upscales_small_images() {
        assert_eq!(
            fit_zoom(v(100.0, 100.0), v(50.0, 50.0), FitMode::AlwaysFit),
            2.0
        );
        // Limiting axis is the narrower one: 200x50 into 100x100 -> 0.5.
        assert_eq!(
            fit_zoom(v(100.0, 100.0), v(200.0, 50.0), FitMode::AlwaysFit),
            0.5
        );
    }

    #[test]
    fn fit_zoom_guards_degenerate_sizes() {
        assert_eq!(fit_zoom(v(0.0, 100.0), v(50.0, 50.0), FitMode::AlwaysFit), 1.0);
        assert_eq!(fit_zoom(v(100.0, 0.0), v(50.0, 50.0), FitMode::AlwaysFit), 1.0);
        assert_eq!(fit_zoom(v(100.0, 100.0), v(0.0, 50.0), FitMode::ReduceOnly), 1.0);
        assert_eq!(fit_zoom(v(100.0, 100.0), v(50.0, 0.0), FitMode::ReduceOnly), 1.0);
        assert_eq!(fit_zoom(v(-5.0, 100.0), v(50.0, 50.0), FitMode::AlwaysFit), 1.0);
        assert_eq!(fit_zoom(v(100.0, 100.0), v(-5.0, 50.0), FitMode::AlwaysFit), 1.0);
    }

    #[test]
    fn clamp_zoom_bounds() {
        assert_eq!(clamp_zoom(0.01), ZOOM_MIN);
        assert_eq!(clamp_zoom(100.0), ZOOM_MAX);
        assert_eq!(clamp_zoom(1.0), 1.0);
        assert_eq!(clamp_zoom(ZOOM_MIN), ZOOM_MIN);
        assert_eq!(clamp_zoom(ZOOM_MAX), ZOOM_MAX);
    }

    #[test]
    fn zoom_to_fill_picks_the_larger_scale() {
        // 50x25 into 100x100: axis scales 2.0 and 4.0 -> 4.0.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(50.0, 25.0)), 4.0);
        // Mirror: 25x50 into 100x100 -> 4.0.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(25.0, 50.0)), 4.0);
        // A large image gets a small scale, still the larger axis.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(400.0, 200.0)), 0.5);
    }

    #[test]
    fn zoom_to_fill_is_clamped_and_guarded() {
        // Extreme upscale clamps to ZOOM_MAX.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(1.0, 1.0)), ZOOM_MAX);
        assert_eq!(zoom_to_fill(v(0.0, 100.0), v(50.0, 50.0)), 1.0);
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(0.0, 0.0)), 1.0);
    }

    #[test]
    fn zoom_percent_label_formats() {
        assert_eq!(zoom_percent_label(1.0), "100%");
        assert_eq!(zoom_percent_label(0.5), "50%");
        assert_eq!(zoom_percent_label(2.0), "200%");
        assert_eq!(zoom_percent_label(ZOOM_MIN), "10%");
    }

    #[test]
    fn zoom_lock_passthrough_when_unlocked() {
        let mut lock = ZoomLock::new();
        assert!(!lock.is_locked());
        assert_eq!(lock.apply(2.5), 2.5);
        lock.lock(1.0);
        lock.unlock();
        assert!(!lock.is_locked());
        assert_eq!(lock.apply(3.0), 3.0);
    }

    #[test]
    fn zoom_lock_holds_captured_zoom() {
        let mut lock = ZoomLock::new();
        lock.lock(2.0);
        assert!(lock.is_locked());
        assert_eq!(lock.apply(0.5), 2.0);
        assert_eq!(lock.apply(10.0), 2.0);
        // Re-locking with a new value takes effect.
        lock.lock(0.75);
        assert_eq!(lock.apply(3.0), 0.75);
    }

    #[test]
    fn fit_mode_labels() {
        assert_eq!(FitMode::DontFit.label(), "Don't fit");
        assert_eq!(FitMode::ReduceOnly.label(), "Reduce only");
        assert_eq!(FitMode::AlwaysFit.label(), "Always fit");
    }
}
