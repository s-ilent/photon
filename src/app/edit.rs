//! Interactive editing operations (Roadmap.md §4) and the undo stack.
//!
//! Pure, GPU-free transforms over RGBA8 buffers — rotate / flip / crop,
//! grayscale / negative — plus a whole-image undo history. Filled in by the
//! editing workstream; the canvas/UI wiring lands separately.
//!
//! Every operation is a whole-image swap: it consumes a borrowed [`Rgba`] and
//! returns a fresh one, so there is no in-place aliasing and any edit is
//! undoable by construction (Roadmap.md §2). The `image` crate's `imageops`
//! offers the same behaviour, but these transforms are small enough to spell
//! out here, which keeps the module testable without pulling an extra decode
//! path in and matches the style of [`crate::app::batch`] — without calling
//! into it.

/// A decoded RGBA8 image: row-major `pixels` (`width * height * 4` bytes) plus
/// its dimensions. The single currency of this module — every operation takes
/// `&Rgba` and returns a new `Rgba`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rgba {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// A crop region in image pixels. Clamped against the image by [`Rgba::crop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rgba {
    pub(crate) fn new(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            pixels,
            width,
            height,
        }
    }

    /// Number of pixels (`width * height`).
    #[allow(dead_code)] // retained; no current caller, kept per the no-opportunistic-deletion rule
    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Byte offset of pixel `(x, y)`. Caller guarantees the coordinates are in
    /// bounds.
    #[inline]
    fn offset(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// Rotate 90° clockwise. Width and height swap.
    pub(crate) fn rotate90(&self) -> Rgba {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0u8; self.pixels.len()];
        // new[y][x] = old[h - 1 - x][y]
        for dy in 0..w {
            for dx in 0..h {
                let src = self.offset(dy, h - 1 - dx);
                let dst = (dy as usize * h as usize + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
            }
        }
        Rgba::new(out, h, w)
    }

    /// Rotate 90° counter-clockwise. Width and height swap.
    pub(crate) fn rotate270(&self) -> Rgba {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0u8; self.pixels.len()];
        // new[y][x] = old[x][w - 1 - y]
        for dy in 0..w {
            for dx in 0..h {
                let src = self.offset(w - 1 - dy, dx);
                let dst = (dy as usize * h as usize + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
            }
        }
        Rgba::new(out, h, w)
    }

    /// Rotate 180°. Dimensions are unchanged.
    pub(crate) fn rotate180(&self) -> Rgba {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0u8; self.pixels.len()];
        // new[y][x] = old[h - 1 - y][w - 1 - x]
        for dy in 0..h {
            for dx in 0..w {
                let src = self.offset(w - 1 - dx, h - 1 - dy);
                let dst = (dy as usize * w as usize + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
            }
        }
        Rgba::new(out, w, h)
    }

    /// Mirror left-to-right. Dimensions are unchanged.
    pub(crate) fn flip_horizontal(&self) -> Rgba {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0u8; self.pixels.len()];
        // new[y][x] = old[y][w - 1 - x]
        for dy in 0..h {
            for dx in 0..w {
                let src = self.offset(w - 1 - dx, dy);
                let dst = (dy as usize * w as usize + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
            }
        }
        Rgba::new(out, w, h)
    }

    /// Mirror top-to-bottom. Dimensions are unchanged.
    pub(crate) fn flip_vertical(&self) -> Rgba {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0u8; self.pixels.len()];
        // new[y][x] = old[h - 1 - y][x]
        for dy in 0..h {
            for dx in 0..w {
                let src = self.offset(dx, h - 1 - dy);
                let dst = (dy as usize * w as usize + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
            }
        }
        Rgba::new(out, w, h)
    }

    /// Crop to `rect`, clamped to the image bounds.
    ///
    /// Choice: a rect that is empty after clamping (zero width/height, wholly
    /// outside the image, or a degenerate zero-area request) is **not** an
    /// error — the image is returned unchanged. The canvas lets the user drag a
    /// selection off-screen, so silently treating "nothing selected" as a no-op
    /// keeps the caller from having to special-case it; an error variant would
    /// invite `unwrap`s at every call site for a case that cannot lose data.
    pub(crate) fn crop(&self, rect: CropRect) -> Rgba {
        if self.width == 0 || self.height == 0 {
            return self.clone();
        }
        // Clamp both corners into [0, width] / [0, height]; `saturating_add`
        // keeps a rect whose far edge would overflow u32 from wrapping.
        let x0 = rect.x.min(self.width);
        let y0 = rect.y.min(self.height);
        let x1 = rect.x.saturating_add(rect.width).min(self.width);
        let y1 = rect.y.saturating_add(rect.height).min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return self.clone();
        }

        let cw = x1 - x0;
        let ch = y1 - y0;
        let mut out = Vec::with_capacity(cw as usize * ch as usize * 4);
        for dy in 0..ch {
            let src_start = self.offset(x0, y0 + dy);
            let src_end = src_start + cw as usize * 4;
            out.extend_from_slice(&self.pixels[src_start..src_end]);
        }
        Rgba::new(out, cw, ch)
    }

    /// Integer-luma grayscale (Appendix A): `gray = (77·R + 150·G + 29·B) >> 8`.
    /// Alpha is preserved.
    pub(crate) fn grayscale(&self) -> Rgba {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let gray =
                ((77u32 * px[0] as u32 + 150u32 * px[1] as u32 + 29u32 * px[2] as u32) >> 8) as u8;
            px[0] = gray;
            px[1] = gray;
            px[2] = gray;
        }
        Rgba::new(out, self.width, self.height)
    }

    /// Invert R, G and B. Alpha is preserved; it is not an alpha inversion.
    pub(crate) fn negative(&self) -> Rgba {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        }
        Rgba::new(out, self.width, self.height)
    }
}

/// Maximum number of whole-image snapshots retained by [`UndoStack`]. Past this
/// the oldest state is evicted, bounding memory to `UNDO_LIMIT ×` one image.
pub(crate) const UNDO_LIMIT: usize = 32;

/// Whole-image undo/redo history.
///
/// Semantics: the stack holds a linear history of full snapshots and a cursor
/// pointing at the *current* state. [`push`](UndoStack::push) records a new
/// current state (truncating any redo branch); [`undo`](UndoStack::undo) steps
/// the cursor back and returns the state now current; [`redo`](UndoStack::redo)
/// steps it forward. Because each entry is a complete image, any operation is
/// undoable no matter how it was composed.
#[derive(Debug, Default)]
pub(crate) struct UndoStack {
    history: Vec<Rgba>,
    /// Index of the current state; only meaningful when `history` is non-empty.
    cursor: usize,
}

impl UndoStack {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// True when no snapshot has been recorded yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Push a new snapshot as the current state. Any snapshots past the cursor
    /// (an undone redo branch) are discarded. Evicts the oldest snapshot when
    /// the history would exceed [`UNDO_LIMIT`].
    pub(crate) fn push(&mut self, snapshot: Rgba) {
        if !self.history.is_empty() {
            self.history.truncate(self.cursor + 1);
        }
        self.history.push(snapshot);
        self.cursor = self.history.len() - 1;

        if self.history.len() > UNDO_LIMIT {
            let excess = self.history.len() - UNDO_LIMIT;
            self.history.drain(0..excess);
            self.cursor = self.history.len() - 1;
        }
    }

    /// Step back one state and return it, or `None` at the start of history.
    pub(crate) fn undo(&mut self) -> Option<Rgba> {
        if self.cursor == 0 || self.history.is_empty() {
            return None;
        }
        self.cursor -= 1;
        Some(self.history[self.cursor].clone())
    }

    /// Step forward one state and return it, or `None` at the end of history.
    pub(crate) fn redo(&mut self) -> Option<Rgba> {
        if self.history.is_empty() || self.cursor + 1 >= self.history.len() {
            return None;
        }
        self.cursor += 1;
        Some(self.history[self.cursor].clone())
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.history.is_empty() && self.cursor > 0
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.history.is_empty() && self.cursor + 1 < self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a row-major RGBA buffer from grayscale values (alpha = 255).
    fn gray_pixels(values: &[u8]) -> Vec<u8> {
        values.iter().flat_map(|&v| [v, v, v, 255u8]).collect()
    }

    /// The luma channel of every pixel, in row-major order.
    fn levels(rgba: &Rgba) -> Vec<u8> {
        rgba.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| px[0])
            .collect()
    }

    /// 3×2 source with distinguishable pixels:
    /// ```text
    /// 1 2 3
    /// 4 5 6
    /// ```
    fn sample_3x2() -> Rgba {
        Rgba::new(gray_pixels(&[1, 2, 3, 4, 5, 6]), 3, 2)
    }

    #[test]
    fn rotate90_swaps_dimensions_and_moves_pixels() {
        let out = sample_3x2().rotate90();
        assert_eq!((out.width, out.height), (2, 3));
        assert_eq!(levels(&out), vec![4, 1, 5, 2, 6, 3]);
    }

    #[test]
    fn rotate270_swaps_dimensions_and_moves_pixels() {
        let out = sample_3x2().rotate270();
        assert_eq!((out.width, out.height), (2, 3));
        assert_eq!(levels(&out), vec![3, 6, 2, 5, 1, 4]);
    }

    #[test]
    fn rotate180_keeps_dimensions_and_reverses_order() {
        let out = sample_3x2().rotate180();
        assert_eq!((out.width, out.height), (3, 2));
        assert_eq!(levels(&out), vec![6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn flip_horizontal_reverses_each_row() {
        let out = sample_3x2().flip_horizontal();
        assert_eq!((out.width, out.height), (3, 2));
        assert_eq!(levels(&out), vec![3, 2, 1, 6, 5, 4]);
    }

    #[test]
    fn flip_vertical_reverses_rows() {
        let out = sample_3x2().flip_vertical();
        assert_eq!((out.width, out.height), (3, 2));
        assert_eq!(levels(&out), vec![4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn rotate90_then_rotate270_is_identity() {
        let src = sample_3x2();
        let roundtrip = src.rotate90().rotate270();
        assert_eq!(roundtrip, src);
    }

    #[test]
    fn crop_keeps_requested_region() {
        let out = sample_3x2().crop(CropRect {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        });
        assert_eq!((out.width, out.height), (2, 1));
        assert_eq!(levels(&out), vec![5, 6]);
    }

    #[test]
    fn crop_clamps_to_image_bounds() {
        // Request runs off the right/bottom edge; only the overlapping 2×1
        // region [x=1..3, y=1..2] survives.
        let out = sample_3x2().crop(CropRect {
            x: 1,
            y: 1,
            width: 99,
            height: 99,
        });
        assert_eq!((out.width, out.height), (2, 1));
        assert_eq!(levels(&out), vec![5, 6]);

        // Offset past the image entirely clamps to an empty region.
        let out = sample_3x2().crop(CropRect {
            x: 50,
            y: 50,
            width: 2,
            height: 2,
        });
        assert_eq!((out.width, out.height), (3, 2));
        assert_eq!(out, sample_3x2());
    }

    #[test]
    fn crop_empty_returns_image_unchanged() {
        let src = sample_3x2();
        let out = src.crop(CropRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        assert_eq!(out, src);
    }

    #[test]
    fn grayscale_uses_documented_luma_weights() {
        // (77*100 + 150*150 + 29*200) >> 8 = 36000 >> 8 = 140.
        let src = Rgba::new(vec![100, 150, 200, 255], 1, 1);
        let out = src.grayscale();
        assert_eq!(out.pixels, vec![140, 140, 140, 255]);
    }

    #[test]
    fn negative_inverts_rgb_but_preserves_alpha() {
        let src = Rgba::new(vec![10, 20, 30, 40], 1, 1);
        let out = src.negative();
        assert_eq!(out.pixels, vec![245, 235, 225, 40]);
    }

    #[test]
    fn grayscale_and_negative_keep_dimensions() {
        let out = sample_3x2().grayscale();
        assert_eq!((out.width, out.height), (3, 2));
        let out = sample_3x2().negative();
        assert_eq!((out.width, out.height), (3, 2));
    }

    /// A minimal image whose bytes identify its history slot.
    fn state(v: u8) -> Rgba {
        Rgba::new(vec![v, 0, 0, 255], 1, 1)
    }

    #[test]
    fn undo_redo_walks_the_history() {
        let mut stack = UndoStack::new();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.undo(), None);
        assert_eq!(stack.redo(), None);

        stack.push(state(1));
        stack.push(state(2));
        stack.push(state(3));
        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        assert_eq!(stack.undo(), Some(state(2)));
        assert_eq!(stack.undo(), Some(state(1)));
        assert!(!stack.can_undo());
        assert_eq!(stack.undo(), None);

        assert_eq!(stack.redo(), Some(state(2)));
        assert_eq!(stack.redo(), Some(state(3)));
        assert!(!stack.can_redo());
        assert_eq!(stack.redo(), None);
    }

    #[test]
    fn push_after_undo_discards_redo_branch() {
        let mut stack = UndoStack::new();
        stack.push(state(1));
        stack.push(state(2));
        stack.push(state(3));
        assert_eq!(stack.undo(), Some(state(2)));

        stack.push(state(9));
        assert!(!stack.can_redo());
        assert_eq!(stack.redo(), None);
        assert_eq!(stack.undo(), Some(state(2)));
        assert_eq!(stack.undo(), Some(state(1)));
    }

    #[test]
    fn undo_evicts_oldest_past_capacity() {
        let mut stack = UndoStack::new();
        for i in 0..(UNDO_LIMIT as u8 + 5) {
            stack.push(state(i));
        }
        // Only the most recent `UNDO_LIMIT` snapshots survive: the current
        // state is the last push, and walking all the way back reaches the
        // oldest retained state (5), not the original (0).
        let mut current = None;
        while let Some(s) = stack.undo() {
            current = Some(s);
        }
        assert_eq!(current, Some(state(5)));
        assert_eq!(stack.undo(), None);
    }
}
