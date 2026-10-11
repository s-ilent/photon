//! Interactive editing operations (Roadmap.md §4) and the undo stack.
//!
//! Pure, GPU-free transforms over RGBA8 buffers — rotate / flip / crop,
//! grayscale / negative — plus a whole-image undo history. Filled in by the
//! editing workstream; the canvas/UI wiring lands separately.
