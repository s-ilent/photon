//! Fit/zoom geometry for the image canvas (Roadmap.md §3).
//!
//! Pure functions over viewport and image sizes — no egui context, no state.
//! `fit_zoom` and `zoom_limits` are live in `app/canvas.rs`; `zoom_percent_label`
//! in the info bar. `FitMode`'s non-default variants and `ZoomLock` await the
//! Settings UI (fit-mode choice / lock-zoom).

use eframe::egui;

/// Cap on magnification, expressed as screen pixels per image pixel: zooming in
/// never lets one image pixel span more than this, so a hairline cannot be
/// smeared into soup. Independent of image size, so a tiny image can still be
/// blown up until it fills the viewport (see [`zoom_limits`]).
pub(crate) const MAX_SCREEN_PX_PER_IMAGE_PX: f32 = 32.0;

/// How far below "fits the window" the user may zoom out, as a fraction of the
/// fit zoom. There is no absolute floor: a 100k-pixel panorama fits at ~0.02
/// zoom, and the limit scales down with it rather than pinning at 10%.
pub(crate) const MIN_FIT_FRACTION: f32 = 0.25;

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

/// How the window itself is resized to an image on load (Roadmap.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FitWindow {
    /// Leave the window size alone.
    #[default]
    DontFit,
    /// Grow the window when it is smaller than the image.
    EnlargeIfSmaller,
    /// Always size the window to the image.
    AlwaysResize,
}

impl FitWindow {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            FitWindow::DontFit => "Don't resize the window",
            FitWindow::EnlargeIfSmaller => "Enlarge the window if smaller than the image",
            FitWindow::AlwaysResize => "Always resize the window",
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

/// Zoom that fills the work area: the larger of the two axis scales, so one
/// axis touches the viewport and the other overflows (panning covers it).
///
/// Used by "zoom to desktop / fit window" and to derive the upper zoom limit.
/// Unclamped by image size — a 24px icon legitimately needs ~100× to fill a
/// 4K display. Degenerate sizes fall back to `1.0`.
pub(crate) fn zoom_to_fill(available: egui::Vec2, image: egui::Vec2) -> f32 {
    if available.x <= 0.0 || available.y <= 0.0 || image.x <= 0.0 || image.y <= 0.0 {
        return 1.0;
    }
    (available.x / image.x).max(available.y / image.y)
}

/// New window size when fitting it to `image`, per axis and independently.
///
/// `DontFit` keeps `current_window`; `EnlargeIfSmaller` grows an axis only when
/// the window is smaller than the image on it; `AlwaysResize` takes the image
/// size. Each axis is then clamped to `monitor` so the window never exceeds the
/// work area. A degenerate monitor axis (zero or negative) leaves that axis
/// unclamped, and non-finite inputs collapse to zero rather than propagating.
pub(crate) fn fit_window_size(
    image: egui::Vec2,
    current_window: egui::Vec2,
    monitor: egui::Vec2,
    mode: FitWindow,
) -> egui::Vec2 {
    let target = match mode {
        FitWindow::DontFit => current_window,
        FitWindow::EnlargeIfSmaller => current_window.max(image),
        FitWindow::AlwaysResize => image,
    };
    egui::vec2(
        clamp_window_axis(target.x, monitor.x),
        clamp_window_axis(target.y, monitor.y),
    )
}

/// Clamp one window axis to a monitor axis. A degenerate limit (zero or
/// negative) only floors the axis at zero; a non-finite length becomes zero.
fn clamp_window_axis(len: f32, limit: f32) -> f32 {
    if !len.is_finite() {
        return 0.0;
    }
    if limit > 0.0 {
        len.clamp(0.0, limit)
    } else {
        len.max(0.0)
    }
}

/// Top-left position that centres a `window` of the given size inside `monitor`.
///
/// Negative on an axis when the window is larger than the monitor there, which
/// pushes the window's overflow evenly off both edges.
pub(crate) fn centered_window_pos(window: egui::Vec2, monitor: egui::Vec2) -> egui::Vec2 {
    (monitor - window) * 0.5
}

/// Zoom that fits `image` inside `monitor` without upscaling past `1.0`: the
/// limiting-axis ratio, capped at 1.0.
///
/// This is fit-to-monitor, distinct from fit-to-window. Degenerate sizes fall
/// back to `1.0` so an unknown image size cannot produce NaN or infinity.
pub(crate) fn zoom_to_desktop(image: egui::Vec2, monitor: egui::Vec2) -> f32 {
    if image.x <= 0.0 || image.y <= 0.0 || monitor.x <= 0.0 || monitor.y <= 0.0 {
        return 1.0;
    }
    (monitor.x / image.x).min(monitor.y / image.y).min(1.0)
}

/// The interactive `(min, max)` zoom range for this image and viewport.
///
/// Both bounds are derived from the image size, not fixed constants:
/// - `min` is a fraction below the fit zoom, so "fits the window" is always
///   attainable however huge the image (and the first wheel tick after a fit
///   never jumps).
/// - `max` is at least the fill zoom, so a tiny image can always fill the
///   screen, and otherwise caps magnification at [`MAX_SCREEN_PX_PER_IMAGE_PX`].
pub(crate) fn zoom_limits(available: egui::Vec2, image: egui::Vec2) -> (f32, f32) {
    let min = fit_zoom(available, image, FitMode::ReduceOnly) * MIN_FIT_FRACTION;
    let max = zoom_to_fill(available, image).max(MAX_SCREEN_PX_PER_IMAGE_PX);
    // Never hand out an inverted range (degenerate sizes are handled by the
    // fallbacks above, but be defensive: `f32::clamp` panics if min > max).
    (min.min(max), max.max(min))
}

/// Clamp `z` into a `(min, max)` range from [`zoom_limits`]. Tolerates an
/// inverted range rather than panicking in `f32::clamp`.
pub(crate) fn clamp_to_limits(z: f32, limits: (f32, f32)) -> f32 {
    let (min, max) = limits;
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    z.clamp(lo, hi)
}

/// Human-readable zoom percentage, e.g. `100%`.
pub(crate) fn zoom_percent_label(z: f32) -> String {
    format!("{}%", (z * 100.0).round() as i32)
}

/// Map a screen position to the image pixel under it, for the status-bar
/// readout. `image_screen_rect` is the drawn (on-screen) rect of the whole
/// image, `zoom` the screen-pixels-per-image-pixel scalar.
///
/// Returns `None` when `screen` is outside the drawn rect, when the rect or
/// zoom is degenerate, or when the computed index falls outside the image
/// bounds (the rect's max edge maps to `width`/`height`, which are not pixels).
pub(crate) fn image_pixel_at(
    screen: egui::Pos2,
    image_screen_rect: egui::Rect,
    zoom: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    if zoom <= 0.0 || width == 0 || height == 0 {
        return None;
    }
    if !image_screen_rect.contains(screen) {
        return None;
    }
    let rel = screen - image_screen_rect.min;
    let fx = rel.x / zoom;
    let fy = rel.y / zoom;
    if !fx.is_finite() || !fy.is_finite() || fx < 0.0 || fy < 0.0 {
        return None;
    }
    // Non-negative float -> int casts truncate (== floor here).
    let x = fx as u32;
    let y = fy as u32;
    if x >= width || y >= height {
        return None;
    }
    Some((x, y))
}

/// `#RRGGBB` (uppercase) for an RGBA8 sample; alpha is deliberately omitted as
/// HTML hex has no alpha channel in this form.
pub(crate) fn html_hex(rgba: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])
}

/// Extra grid offsets needed so copies of `image_rect` tile across `viewport`.
///
/// The first entry is always [`egui::Vec2::ZERO`] (the centre tile the app
/// already draws); the rest are integer multiples of the image size that push
/// neighbouring copies into view. Results are ordered nearest-first and capped
/// at `max_tiles` total entries, so a tiny image zoomed far out cannot
/// enumerate an unbounded grid.
pub(crate) fn tile_offsets(
    viewport: egui::Rect,
    image_rect: egui::Rect,
    max_tiles: usize,
) -> Vec<egui::Vec2> {
    if max_tiles == 0 {
        return Vec::new();
    }
    let mut offsets = Vec::with_capacity(max_tiles.min(64));
    offsets.push(egui::Vec2::ZERO);
    let w = image_rect.width();
    let h = image_rect.height();
    if max_tiles == 1 || w <= 0.0 || h <= 0.0 || !viewport.is_positive() {
        return offsets;
    }

    // Index window on the tile grid: the copies whose rect intersects the
    // viewport. A copy at (i, j) spans image_rect translated by (i*w, j*h), so
    // it intersects horizontally when
    //   i >= (viewport.min.x - image_rect.max.x) / w
    //   i <= (viewport.max.x - image_rect.min.x) / w
    // Clamped (in each direction) to `max_tiles`, since more copies than that
    // could never be drawn anyway.
    let cap = max_tiles as i64;
    let k_lo = (((viewport.min.x - image_rect.max.x) / w).ceil() as i64).max(-cap);
    let k_hi = (((viewport.max.x - image_rect.min.x) / w).floor() as i64).min(cap);
    let j_lo = (((viewport.min.y - image_rect.max.y) / h).ceil() as i64).max(-cap);
    let j_hi = (((viewport.max.y - image_rect.min.y) / h).floor() as i64).min(cap);

    let mut candidates: Vec<(i64, i64)> = Vec::new();
    for i in k_lo..=k_hi {
        for j in j_lo..=j_hi {
            if i == 0 && j == 0 {
                continue;
            }
            candidates.push((i, j));
        }
    }
    candidates.sort_by_key(|&(i, j)| i * i + j * j);

    for (i, j) in candidates {
        if offsets.len() >= max_tiles {
            break;
        }
        offsets.push(egui::vec2(i as f32 * w, j as f32 * h));
    }
    offsets
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
    fn zoom_to_fill_picks_the_larger_scale() {
        // 50x25 into 100x100: axis scales 2.0 and 4.0 -> 4.0.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(50.0, 25.0)), 4.0);
        // Mirror: 25x50 into 100x100 -> 4.0.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(25.0, 50.0)), 4.0);
        // A large image gets a small scale, still the larger axis.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(400.0, 200.0)), 0.5);
    }

    #[test]
    fn zoom_to_fill_is_unclamped_and_guarded() {
        // A 1x1 image in a 100x100 area needs 100x — not clipped to 50.
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(1.0, 1.0)), 100.0);
        assert_eq!(zoom_to_fill(v(0.0, 100.0), v(50.0, 50.0)), 1.0);
        assert_eq!(zoom_to_fill(v(100.0, 100.0), v(0.0, 0.0)), 1.0);
    }

    #[test]
    fn huge_image_can_be_zoomed_out_to_fit() {
        // 100k-wide panorama in a 1920x1080 viewport.
        let (min, max) = zoom_limits(v(1920.0, 1080.0), v(100_000.0, 1000.0));
        let fit = fit_zoom(v(1920.0, 1080.0), v(100_000.0, 1000.0), FitMode::ReduceOnly);
        // The old fixed floor was 0.1; fit here is ~0.019 and must be reachable.
        assert!(fit < 0.1, "fit zoom should be below the old floor");
        assert!(min <= fit, "fit must be within the limits (min {min}, fit {fit})");
        assert!(max >= MAX_SCREEN_PX_PER_IMAGE_PX);
    }

    #[test]
    fn tiny_image_can_be_zoomed_in_to_fill() {
        // 24px icon on a 2560x1440 display needs ~106x to fill.
        let (min, max) = zoom_limits(v(2560.0, 1440.0), v(24.0, 24.0));
        let fill = zoom_to_fill(v(2560.0, 1440.0), v(24.0, 24.0));
        assert!(fill > 100.0);
        assert!(max >= fill, "fill must be reachable (max {max}, fill {fill})");
        assert!(min <= 1.0);
    }

    #[test]
    fn limits_guards_degenerate_sizes() {
        let (min, max) = zoom_limits(v(0.0, 100.0), v(50.0, 50.0));
        assert!(min.is_finite() && max.is_finite());
        assert!(min <= max);
    }

    #[test]
    fn clamp_to_limits_bounds() {
        let limits = (0.25, 4.0);
        assert_eq!(clamp_to_limits(0.01, limits), 0.25);
        assert_eq!(clamp_to_limits(100.0, limits), 4.0);
        assert_eq!(clamp_to_limits(1.0, limits), 1.0);
        // Inverted range does not panic.
        assert_eq!(clamp_to_limits(1.0, (4.0, 0.25)), 1.0);
    }

    #[test]
    fn zoom_percent_label_formats() {
        assert_eq!(zoom_percent_label(1.0), "100%");
        assert_eq!(zoom_percent_label(0.5), "50%");
        assert_eq!(zoom_percent_label(2.0), "200%");
        assert_eq!(zoom_percent_label(0.1), "10%");
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

    #[test]
    fn image_pixel_at_maps_inside() {
        // 50x25 image at zoom 2 -> 100x50 on screen starting at (10, 20).
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        assert_eq!(
            image_pixel_at(egui::pos2(10.0, 20.0), rect, 2.0, 50, 25),
            Some((0, 0))
        );
        assert_eq!(
            image_pixel_at(egui::pos2(12.0, 22.0), rect, 2.0, 50, 25),
            Some((1, 1))
        );
        assert_eq!(
            image_pixel_at(egui::pos2(109.0, 69.0), rect, 2.0, 50, 25),
            Some((49, 24))
        );
    }

    #[test]
    fn image_pixel_at_rejects_outside_and_edge() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        // Left / above the rect.
        assert_eq!(image_pixel_at(egui::pos2(9.0, 20.0), rect, 2.0, 50, 25), None);
        assert_eq!(image_pixel_at(egui::pos2(10.0, 19.0), rect, 2.0, 50, 25), None);
        // Below the rect (past the last row).
        assert_eq!(image_pixel_at(egui::pos2(10.0, 71.0), rect, 2.0, 50, 25), None);
        // Max edge maps to width/height, which are not pixels.
        assert_eq!(image_pixel_at(egui::pos2(110.0, 70.0), rect, 2.0, 50, 25), None);
    }

    #[test]
    fn image_pixel_at_guards_degenerate() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        assert_eq!(image_pixel_at(egui::pos2(10.0, 20.0), rect, 0.0, 50, 25), None);
        assert_eq!(image_pixel_at(egui::pos2(10.0, 20.0), rect, -1.0, 50, 25), None);
        assert_eq!(image_pixel_at(egui::pos2(10.0, 20.0), rect, 2.0, 0, 25), None);
        assert_eq!(image_pixel_at(egui::pos2(10.0, 20.0), rect, 2.0, 50, 0), None);
    }

    #[test]
    fn html_hex_formats_uppercase_rgb() {
        assert_eq!(html_hex([0, 0, 0, 255]), "#000000");
        assert_eq!(html_hex([255, 255, 255, 255]), "#FFFFFF");
        assert_eq!(html_hex([18, 52, 86, 255]), "#123456");
        // Alpha is not part of the hex string.
        assert_eq!(html_hex([18, 52, 86, 7]), "#123456");
    }

    fn covers(offsets: &[egui::Vec2], base: egui::Rect, p: egui::Pos2) -> bool {
        offsets.iter().any(|o| base.translate(*o).contains(p))
    }

    #[test]
    fn tile_offsets_covers_viewport() {
        // 10x10 image centred in a 100x100 viewport at the origin -> needs
        // 11x11 = 121 copies to cover; a generous cap admits them all.
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let image = egui::Rect::from_min_size(egui::pos2(45.0, 45.0), egui::vec2(10.0, 10.0));
        let offsets = tile_offsets(viewport, image, 400);
        assert_eq!(offsets[0], egui::Vec2::ZERO);
        for p in [
            egui::pos2(0.0, 0.0),
            egui::pos2(99.0, 0.0),
            egui::pos2(0.0, 99.0),
            egui::pos2(99.0, 99.0),
            egui::pos2(50.0, 50.0),
        ] {
            assert!(covers(&offsets, image, p), "point {p:?} not covered");
        }
    }

    #[test]
    fn tile_offsets_is_bounded_by_max_tiles() {
        // A tiny image in a huge viewport would otherwise need millions of
        // copies; the cap must bound the count.
        let viewport =
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100_000.0, 100_000.0));
        let image = egui::Rect::from_min_size(egui::pos2(50_000.0, 50_000.0), egui::vec2(1.0, 1.0));
        for cap in [1usize, 4, 16, 64] {
            let offsets = tile_offsets(viewport, image, cap);
            assert!(offsets.len() <= cap, "cap {cap} exceeded: {}", offsets.len());
            if cap > 0 {
                assert_eq!(offsets[0], egui::Vec2::ZERO);
            }
        }
        assert!(tile_offsets(viewport, image, 0).is_empty());
    }

    #[test]
    fn tile_offsets_degenerate_image_returns_centre() {
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let zero = egui::Rect::from_min_size(egui::pos2(50.0, 50.0), egui::Vec2::ZERO);
        assert_eq!(tile_offsets(viewport, zero, 64), vec![egui::Vec2::ZERO]);
    }

    #[test]
    fn fit_window_default_is_dont_fit() {
        assert_eq!(FitWindow::default(), FitWindow::DontFit);
    }

    #[test]
    fn fit_window_labels() {
        assert_eq!(FitWindow::DontFit.label(), "Don't resize the window");
        assert_eq!(
            FitWindow::EnlargeIfSmaller.label(),
            "Enlarge the window if smaller than the image"
        );
        assert_eq!(FitWindow::AlwaysResize.label(), "Always resize the window");
    }

    #[test]
    fn fit_window_dont_fit_keeps_current_size() {
        let size = fit_window_size(
            v(800.0, 600.0),
            v(400.0, 300.0),
            v(1920.0, 1080.0),
            FitWindow::DontFit,
        );
        assert_eq!(size, v(400.0, 300.0));
    }

    #[test]
    fn fit_window_enlarge_if_smaller_only_grows() {
        // Window smaller than the image on x only: x grows, y stays.
        assert_eq!(
            fit_window_size(
                v(800.0, 300.0),
                v(400.0, 500.0),
                v(1920.0, 1080.0),
                FitWindow::EnlargeIfSmaller
            ),
            v(800.0, 500.0)
        );
        // Window larger than the image on both axes: unchanged.
        assert_eq!(
            fit_window_size(
                v(200.0, 100.0),
                v(400.0, 500.0),
                v(1920.0, 1080.0),
                FitWindow::EnlargeIfSmaller
            ),
            v(400.0, 500.0)
        );
    }

    #[test]
    fn fit_window_always_resize_takes_image_size() {
        assert_eq!(
            fit_window_size(
                v(800.0, 600.0),
                v(400.0, 300.0),
                v(1920.0, 1080.0),
                FitWindow::AlwaysResize
            ),
            v(800.0, 600.0)
        );
    }

    #[test]
    fn fit_window_clamps_to_monitor() {
        assert_eq!(
            fit_window_size(
                v(4000.0, 2000.0),
                v(400.0, 300.0),
                v(1920.0, 1080.0),
                FitWindow::AlwaysResize
            ),
            v(1920.0, 1080.0)
        );
    }

    #[test]
    fn fit_window_guards_degenerate_sizes() {
        // Zero image: AlwaysResize collapses to zero, still finite.
        let size = fit_window_size(
            v(0.0, 0.0),
            v(400.0, 300.0),
            v(1920.0, 1080.0),
            FitWindow::AlwaysResize,
        );
        assert_eq!(size, v(0.0, 0.0));
        // Degenerate monitor axes leave the axis unclamped, never NaN.
        let size = fit_window_size(
            v(800.0, 600.0),
            v(400.0, 300.0),
            v(0.0, -10.0),
            FitWindow::AlwaysResize,
        );
        assert_eq!(size, v(800.0, 600.0));
        assert!(size.x.is_finite() && size.y.is_finite());
        // Non-finite target collapses to zero.
        let size = fit_window_size(
            v(f32::INFINITY, 600.0),
            v(400.0, 300.0),
            v(1920.0, 1080.0),
            FitWindow::AlwaysResize,
        );
        assert_eq!(size, v(0.0, 600.0));
    }

    #[test]
    fn centered_window_pos_centers() {
        assert_eq!(centered_window_pos(v(800.0, 600.0), v(1920.0, 1080.0)), v(560.0, 240.0));
    }

    #[test]
    fn centered_window_pos_goes_negative_when_larger() {
        // A window twice the monitor overhangs equally on both edges.
        assert_eq!(centered_window_pos(v(200.0, 100.0), v(100.0, 100.0)), v(-50.0, 0.0));
    }

    #[test]
    fn zoom_to_desktop_uses_limiting_axis() {
        // 2000x1000 into 1000x2000: axis ratios 0.5 and 2.0 -> 0.5.
        assert_eq!(zoom_to_desktop(v(2000.0, 1000.0), v(1000.0, 2000.0)), 0.5);
    }

    #[test]
    fn zoom_to_desktop_never_upscales() {
        // 100x100 into 1920x1080 would be 10.8, capped at 1.0.
        assert_eq!(zoom_to_desktop(v(100.0, 100.0), v(1920.0, 1080.0)), 1.0);
    }

    #[test]
    fn zoom_to_desktop_guards_degenerate_sizes() {
        assert_eq!(zoom_to_desktop(v(0.0, 100.0), v(1920.0, 1080.0)), 1.0);
        assert_eq!(zoom_to_desktop(v(100.0, 100.0), v(0.0, 1080.0)), 1.0);
        assert_eq!(zoom_to_desktop(v(100.0, 100.0), v(1920.0, -1.0)), 1.0);
    }
}
