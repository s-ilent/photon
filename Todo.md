# TODO — Photon

Items surfaced from AGENTS.md, CLAUDE.md, and prior notes. Grouped by area.
Per CLAUDE.md, do not delete pre-existing dead code opportunistically; list it
here and address it deliberately.

Items marked ✅ are done as of 2026-10-11 and kept here for the record.

---

## Done in earlier passes

- [x] Sidebar sorting UI (`main.rs` sidebar combo box over `SortMode`).
- [x] "Lock zoom level" option (`AppSettings.keep_zoom` + Settings checkbox).
- [x] **Implement Save** — `Cmd/Ctrl+S` opens an `rfd` save-as dialog (always
      save-as; if the user picks the original path we honor it). Full image and
      "Save Selection As…" (crop to the active selection box, button-only) are
      wired. Encodable formats: png, jpeg, webp, bmp, tiff (jpeg alpha
      flattened to RGB). Encode runs in a background thread.
- [x] **Surface loader errors in the UI** — the worker sends
      `LoadResult::Failed` on decode/IO failure; `check_load_complete` routes
      the failure (for the active target only) to `StatusOverlay::Error` /
      `UnsupportedFormat` instead of an infinite spinner. Navigation / paste /
      dir-load clear the overlay.
- [x] **DDS decode for BC4/BC5/BC7 + DX10** — `.dds` files route through
      `image_dds` instead of `image`'s built-in DDS decoder, which rejected
      anything past DXT5. Covers DXT1–DXT5, BC4, BC5 (ATI2/BC5U), BC7, and
      uncompressed variants; DX10-header files work too.

## Done 2026-10-11 (gap-hitting pass)

- [x] **DDS oversized-texture handling** — the loader worker rejects images
      wider/taller than `device.limits().max_texture_dimension_2d` (or
      zero-extent) up front and surfaces an error overlay, instead of sending
      an oversized `Loaded` that fails asynchronously in wgpu validation.
      Layers decode individually, so stacked oversized images no longer exist.
      Full fix (native-format upload, per-face viewing) still belongs to the
      shader pass below. Background in `DDS-investigation.md`.
- [x] **View all cubemap faces / array layers** — every layer decodes at load
      (capped at 64 layers), each uploads as its own texture, and a bottom-bar
      selector switches between them (cubemap face labels +X/-X/+Y/-Y/+Z/-Z,
      numbered entries for arrays). Copy/save follow the displayed layer.
      Decided 2026-10-11: face selector now, true pan-around cubemap view
      later with the shader pass.
- [x] **Zoom reset on dimension change** — zoom/pan reset only when the new
      image's dimensions differ from the previous one, superseded by the
      config work below: the behavior is now the `ZoomReset` setting.
- [x] **Config file** — `config.ron` persists all settings (`dark_mode`,
      `font_size`, `scale_to_fit`, `use_sharp_scaling`, `checkerboard_bg`,
      `zoom_reset = always|dimension_change|never`, `sort_mode`). Serialized
      with serde + RON (`#[serde(default)]` so partial files load), config
      directory from the `directories` crate (cross-platform), overridable
      with `PHOTON_CONFIG_DIR`. Loaded at startup, rewritten on UI changes,
      hand edits preserved when nothing changed.
- [x] **Open-file dialog filter** — `dds` and `exr` added (walker/decoder
      supported them but the dialog hid them).
- [x] **Clipboard copy / Save without re-decode** — the worker returns decoded
      RGBA for `Active`/`Paste` results (preloads stay metadata-only);
      `PhotonApp::active_rgba` holds the active image's bytes and
      `current_rgba` uses them. A re-decode remains as fallback for cache hits
      that never went through the worker.
- [x] **Save success/failure feedback** — the encode thread reports over a
      channel; a transient bottom-bar message shows green success / red
      failure for 4s. stdout print kept.
- [x] **Unused dependencies removed** — `tokio`, `chrono`, `parking_lot`,
      `bytemuck`, `thiserror` dropped from `Cargo.toml` (`tempfile` stays: the
      DDS tests use it).
- [x] **Compiler/lint warnings cleared** — egui 0.34 panel API migration
      (`Panel::top/bottom/left` + `show_inside`, `min_size`,
      `ComboBox::from_id_salt`), f32 stroke literals, `usize` cast,
      explicit auto-deref, `as_chunks`. Build and clippy are now warning-free.
- [x] **Texture leak audit** — traced `check_load_complete` (dedup, eviction,
      old-display cleanup) and `load_directory` (full free on reload): every
      path frees what it drops. One deliberate deferral: if the LRU victim is
      the currently displayed texture it is popped from cache but not freed —
      step 5 of the active-completion path frees it when the next active image
      arrives. No leak found.
- [x] **Speculative preload bounds verified** — `request_current` skips
      neighbors already in the cache; the worker drops stale `Active` requests
      via `latest_req_id`; preloads queued *before* a picked Active/Paste are
      discarded, ones queued after survive. No starvation path found.

---

## Features & behavior gaps

- [ ] **Imagine-style texture sidebar** — a sidebar (like the Imagine image
      viewer) exposing mip levels, texture-array entries, and cubemap faces in
      one tree. Supersedes the bottom-bar face buttons for multi-layer files
      once it exists. Needs a mip-decode path first: the current pipeline
      decodes and uploads mip 0 only (`decode_layers_mipmaps_dds(.., 0..1)`),
      so a mip selector needs either per-mip decode on demand or decoding the
      full chain at load.
- [ ] **Keep one image alive during folder scroll** — when rapidly scrolling
      through a folder, the displayed image doesn't update until its load
      completes; meanwhile any completed preload is shown dimmed as an
      intermediate (possibly the *neighbor's* pixels). Consider a "pinned
      active" slot in the texture cache that eviction can't touch, or
      suppressing the intermediate display when its path isn't the target.
- [ ] **Property map mode for lightramp setup** — proposed extra mouse/view
      mode; needs a spec before implementation.
- [ ] **Normal-map preview mode** — BC5/BC7 normal maps decode *correctly*
      (channels are the file's actual RGBA), but appear as white/cyan/pink/red.
      Skyrim normal maps in particular pack extra data in B/A. A preview mode
      that reconstructs `B = sqrt(1 - R² - G²)` and/or shows single channels
      would make these legible. Belongs to the shader + view-mode pass.
      (Sample: `mountainslab02_n.dds`.)
- [ ] **Shader + tonemap render pass** — the planned future rendering path:
      upload textures as their native format (incl. `Rgba32Float` for HDR
      DDS), do gamma correction, sharp filtering, and tonemap on the GPU.
      Replaces the "decode to RGBA8 then `egui` painter image" pipeline.
      Unblocks proper sharp scaling, correct gamma, HDR, and the normal-map
      preview above. This is the long-term answer to several open items.
- [ ] **Verify image copy/paste on Wayland** — Sophia's Cargo notes record
      arboard as "text-only on Wayland (image copy works on X11 / Windows /
      macOS)". Photon enables `wayland-data-control` (the data-control
      protocols do carry image MIME types), and arboard's docs only warn
      about compositor support — but the claim deserves a 30-second test on
      this machine: copy in Photon → paste into a native Wayland app, and
      the reverse. If images genuinely don't cross, that's a latent bug on
      the primary dev platform; fallbacks: XWayland or wl-clipboard-rs
      directly.

## Dependency / format notes

- [ ] **egui-ecosystem shortlist** (crate-shopped from Sophia 2026-10-11 —
      same egui 0.34.3 line, so their version pins and gotchas transfer
      directly):
  - **Adopt candidates (low risk):**
    - `egui-phosphor 0.12.0` — Phosphor icon font; replaces the emoji
      toolbar buttons (📁 ⚙ 💾 ✂) with crisp, consistent icons. Same
      version Sophia uses.
    - `log` + `env_logger` 0.11 + `egui_logger 0.10.1` — the logging move,
      using Sophia's exact vetted stack (egui_logger also provides an
      in-app console widget if we ever want one; its backend works without
      showing any UI). Note: Sophia vendors a narrow-pane wrapping fix —
      only relevant if we display the console in a tight space.
    - Bumps: `rfd 0.15 → 0.17`, `lru 0.17 → 0.18.5` (Sophia vetted 0.18.5
      for exactly our use: eviction-ordered handle registry).
  - **Feature-gated adopts (pick up with their feature):**
    - `notify-debouncer-full 0.7` — folder watching for the open directory
      (debounced; re-exports `notify`, so it's the only direct dep). Use
      this instead of raw `notify` — aligns with Sophia's plugin watcher.
    - `profiling 1` (+ opt-in `puffin`/`tracy` backends) — zone spans for
      the shader pass / loader perf work. Sophia's opt-in-feature pattern
      keeps default builds clean.
  - **Options, only on demand:**
    - `egui-notify 0.22` — toast queue; supersedes our hand-rolled
      bottom-bar save message if we ever want multiple/queued messages.
      House rule applies (plain text styling).
    - `egui_taffy 0.12` — flexbox layout inside egui; Sophia uses it for
      their status bar. Our bottom bar is native-layout for now.
  - **Not applicable (noted for completeness):** `egui_dock` (no docking),
    `egui-arbor` (no tree outliner yet — revisit for the Imagine-style
    texture sidebar), `egui_plot`, `egui_code_editor`, `egui_field_editor`,
    `ab_glyph` (our font is vendored, not user-supplied), `mimalloc`
    (only if decode profiling shows allocation cost).
  - **Gotchas carried over from Sophia's vetting:** egui-ecosystem crates
    have offset versions that must be pinned to avoid pulling a second
    egui (e.g. `egui_plot 0.35.0` targets egui 0.34; `egui_code_editor`
    needed an exact `=0.3.6` pin; `egui_taffy 0.12` is the last 0.34
    line). Check the crate's egui req against our 0.34 lock before adding
    any of these. Sophia's vendored-fork pattern (`VENDOR_NOTES.md` +
    `[patch.crates-io]`) is the escalation path if a picked crate needs a
    fix upstream lacks.
- [ ] **Crate-shopping shortlist** (surveyed 2026-10-11; adopt per milestone,
      not all at once):
  - **Now-adjacent UI/UX**: `notify` — watch the open directory and refresh
    `images` when files appear/change (folder browser correctness); `log` +
    `env_logger` (or `tracing`) — replace `println!`/`DEBUG:` printing with
    leveled, configurable logging.
  - **Shader pass**: `bytemuck` (zero-copy RGBA/vertex staging),
    `glam` (view-direction math for pan-around cubemap view; pairs with wgpu
    conventions), possibly `half` (f16 intermediates for HDR/BC6H work).
  - **Batch converter** (Plan.md): `rayon` — `par_iter` encode/convert.
  - **When errors outgrow strings**: `thiserror` — typed error enum; first
    candidate is replacing the `"Failed to decode"` prefix classification
    between `image_loader.rs` and `app/loader.rs`.
  - **If the pipeline goes async**: `tokio` (async walk/IO) — possibly with
    `flume` for sync+async channels; only when a real async need exists.
  - **UI niceties**: `jiff` (or `chrono`) for human-readable modified times;
    `humansize` for file sizes in the sidebar/status bar.
- [ ] **Roadmap: reintroduce removed dependencies where they earn their
      keep.** Removed 2026-10-11 as unused; each has a planned role:
  - `tokio` — async directory walking (`walkdir` is currently synchronous on
    the walker thread) and async file IO if the loader moves off
    `std::thread` + `mpsc`; also the substrate for file watching (`notify`).
  - `thiserror` — a typed loader/error enum once string errors get unwieldy;
    the first candidate is replacing the error-prefix classification between
    `image_loader.rs` and `app/loader.rs` (`"Failed to decode"` string
    matching) with a real error kind.
  - `bytemuck` — zero-copy casting of RGBA buffers for GPU uploads in the
    shader pass (vertex/uniform staging, `Rgba32Float` conversion).
  - `parking_lot` — faster/smaller locks if state sharing between the render
    thread and background workers grows beyond the current mpsc channels.
  - `chrono` — human-readable dates in the UI (e.g. modified time in the
    sidebar or status bar; sort itself uses raw `SystemTime`).
- [ ] **DDS compressed-VRAM path** — `Plan.md` describes uploading BC1–BC7
      compressed DDS bytes straight to VRAM. Current pipeline decodes
      everything to RGBA8 and uploads as `Rgba8Unorm` (via `image_dds`). This
      is an optimization, not a regression; covered by the shader pass above.
- [ ] **BC6H (HDR float) DDS decode** — `image_dds::image_from_dds` returns
      RGBA8; BC6H_UF16 float textures need the `imagef32_from_dds` path plus a
      tonemap/normalize step. None observed in the Fallout 4 tree sampled;
      deferred. Related to the shader pass.

---

## Out of scope / reference only

- `examples~/`, `gemini/`, `iced.md`, `Plan.md`, `scratch/`, `agents/all.txt` —
  AI scratch and historical design docs. Not part of the product; do not treat
  as work items. `iced.md` already records the decision to stay on `egui`.
- `DDS-investigation.md` — record of the oversized-DDS investigation; the
  short-term mitigations it proposed are implemented (size guard + face-0
  decode); the standalone wgpu-29 test app at `/tmp/ddstest` is obsolete.
