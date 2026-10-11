# TODO — Photon

Living worklist, re-prioritized 2026-10-11 after the config/logging/shortcut
pass. Per CLAUDE.md, do not delete pre-existing dead code opportunistically;
list it here and address it deliberately.

The long horizon — the full forward feature map, grouped by capability, with
the decided scope and the explicit out-of-scope list — lives in `Roadmap.md`.
This file is the near-term, moving slice of it; when something on the roadmap
comes into focus, add it here with a concrete plan.

---

## Recommended order

What's next, in the order I'd do it (full arcs in `Roadmap.md` §13):

1. **Shader M4: HDR + tonemap** — `Rgba32Float`/`Rgba16Float` float textures for
   EXR and BC6H, exposure/gamma tonemap in the shader. The colour-space field
   and the `linear_source` shader branch from M3 are its substrate. (M5 cubemap
   view and the normal-map preview are the other shader-pass items.)
2. **Command registry + params serialisation** (`Roadmap.md` §1) — the spine
   that makes key-rebinding, the CLI, and batch cheap; build it before those
   so they share one dispatch table.
3. **Metadata + Image Info** (`Roadmap.md` §9) — Exif panel; ICC (now with the
   `ColorSpace` field ready to consume).
4. **Batch rename + the remaining batch items** (`Roadmap.md` §5) — recipe
   import/export and the interactive overwrite dialog (the chain, save options
   and progress landed 2026-10-12).
5. **Full browser** (`Roadmap.md` §7) with the persistent thumbnail cache.
6. **Texture inspection sidebar** — DEFERRED: the user is designing this.
   Implement only against their design when it's ready.
7. **Property map mode** — when a spec exists.

Done recently (2026-10-12): viewing polish (§3), the editing core (§4), the
batch polish (§5), and shader **M3 native BCn upload + the colour-space model**
(§5b) are all complete; see the Done log. Next up is the shader M4 arc.

---

## Features & behavior gaps

- [ ] **Texture inspection sidebar** — DEFERRED by the user (2026-10-11):
      they are designing it; implement only against that design. Covers mip
      levels, texture-array entries, and cubemap faces in one tree,
      superseding the bottom-bar face buttons. Needs a mip-decode decision:
      the pipeline decodes/uploads mip 0 only
      (`decode_layers_mipmaps_dds(.., 0..1)`), so a mip selector needs either
      per-mip decode on demand or the full chain at load.
- [~] **Batch converter + operation chain** (`Roadmap.md` §5) — engine core
      (`app/batch.rs`) and the **Convert… window** (`ui/batch_window.rs`) are
      DONE (2026-10-11): file list, format/quality/folder, operation chain,
      non-blocking `rayon` run, per-file status. The chain, save options and
      progress advanced 2026-10-12: the seven convolution filters
      (blur/soften/sharpen/Gaussian3/Gaussian5/emboss/edge, Roadmap Appendix A
      kernels) as the retained CPU path, `SaveOptions` (JPEG quality + PNG
      compression; WebP quality and TIFF compression are unavailable in
      `image` 0.25), and a determinate progress bar via
      `run_batch_with_progress`. REMAINING: recipe import/export, the
      interactive per-file overwrite dialog (`OverwritePolicy::Ask` still
      behaves as skip), and the compute-shader path for filters (§5b).
- [ ] **Batch rename** (`Roadmap.md` §5) — mask tokens (file/image attrs,
      counters), search-and-replace, live preview, reorderable list.
- [x] **Light editing core** (`Roadmap.md` §4) — DONE 2026-10-12. Pure ops +
      bounded undo in `app/edit.rs` (2026-10-11) plus the canvas/menu wiring:
      `apply_edit`/`undo_edit`/`redo_edit` re-upload the result on the main
      thread (kept out of the LRU cache, so an edit is transient and copy/save
      always match the screen), the eight edit ops + Undo/Redo are registry
      commands, and the Edit menu drives them.
- [ ] **Full browser** (`Roadmap.md` §7) — thumbnail grid + disk cache, views,
      sort, filters, folder tree/history, preview pane, file ops.
- [ ] **Panorama creator** (`Roadmap.md` §6) — concatenation stitch only.
- [ ] **ICC / color profiles** (`Roadmap.md` §9) — read ICC and apply in the
      shader pass; deferred with it.
- [ ] **Watcher niceties** (post-MVP folder watching, shipped 2026-10-11) —
      handle the watched directory itself being deleted/renamed (currently
      the watcher just stops producing useful listings); consider a
      directory-picker affordance for when the open folder empties.
- [ ] **Drag-and-drop on Wayland** — works on X11/Windows/macOS but winit
      0.30's Wayland backend has no file-drop support (verified in winit
      source: `DroppedFile` is emitted from the X11 event processor only).
      Blocked on winit; revisit on winit upgrades. The `open_path` plumbing
      is done and platform-agnostic.
- [x] **Tiled/chunked viewing of very large images** — SHIPPED with the
      shader pass (2026-10-11): oversized images are chunked at the GPU's
      max texture dimension and reassembled in image space; the oversized
      rejection is gone. Remaining limit is RAM for the CPU-side decode.
- [ ] **Key configuration** — user-configurable shortcut map (config.ron);
      F11 fullscreen and all others become remappable then. The substrate — the
      `Command` registry + binding table + `run_command` dispatch — landed
      2026-10-11 (Roadmap §1 step 1); remaining is the config-file table and a
      Settings rebinding UI.
- [x] **Exif auto-rotate on load** (`Roadmap.md` §3) — DONE 2026-10-11: applied
      at decode in `image_loader.rs` via the `image` crate's
      `into_decoder`/`orientation`/`apply_orientation` (no new dep); dimensions
      swap for 90/270. Landscape/portrait phone photos now load upright.
- [x] **Fit modes + zoom set** (`Roadmap.md` §3) — DONE 2026-10-12. Fit +
      zoom limits + the fit-mode Setting + `Fit`/`100%` buttons (2026-10-11),
      then fit-window-per-axis (`FitWindow` + Settings combo + View menu),
      centre-window and zoom-to-desktop actions, and `ZoomLock` (persisted
      `lock_zoom`, Settings checkbox + View menu toggle) — the last
      `#[allow(dead_code)]` in `app/view.rs` is gone.
- [x] **Status-bar pixel readout** (`Roadmap.md` §3) — DONE 2026-10-11:
      hovered pixel (RGB/alpha/hex) in the info bar, click-to-copy; sampled from
      `active_rgba` (blank on cache-hit loads). Tile-repeat ("seamless texture")
      view also landed: `settings.tile_repeat` draws the image in a viewport-
      covering grid (canvas-only, no shader change).
- [x] **Batch conversion engine** (`Roadmap.md` §5) — DONE 2026-10-11 (core):
      `app/batch.rs` — `Operation` chain (rotate/flip/grayscale/negative/resize
      with the resample filters), `OutputFormat` encoders, overwrite policy,
      per-file `FileStatus`, `rayon` `par_iter` `run_batch`. Remaining:
      convolution filters (**CPU fallback now; compute path is future —
      `Roadmap.md` §5b**),
      per-format save options, recipe import/export, and the "Convert…" UI
      dialog + progress.
- [ ] **Content-based format detection** (`Roadmap.md` §5) — sniff bytes,
      honour them when the extension lies.
- [x] **Decode-error enum** (`Roadmap.md` §8) — DONE 2026-10-11: typed
      `DecodeError` in `image_loader.rs` (hand-rolled `Display`/`Error`, no new
      dep); `LoadResult::Failed.is_format_error` now derives from
      `is_unsupported_format()` instead of the `"Failed to decode"` prefix. The
      `thiserror` reintroduction stays optional (the manual impl is fine).

## Shader pass milestones (the long arc)

Upload textures as their native format, do gamma/tonemap/sharp filtering on
the GPU via `egui_wgpu::CallbackTrait` (feasibility reviewed 2026-10-11 —
viable and architecturally clean). Unblocks: correct sharp scaling, gamma,
HDR, normal-map preview, cubemap 3D view, compressed-VRAM DDS.

- [x] **M1 + M2 + chunked viewing** (2026-10-11) — the image canvas renders
      through a custom WGSL pipeline (`src/render.rs`): per-frame uniform
      (viewport transform, tint, sharp flag), tiles drawn per layer, and
      oversized images chunked at the GPU limit and reassembled in image
      space. Sharp (pixel) sampling works as designed — nearest in the
      shader when magnified, linear otherwise. GPU lifetime is wgpu
      refcounting (`Arc<RenderImage>`); all manual free bookkeeping is gone.
      Color space: RGBA8 passes through untouched (sRGB-encoded, matching
      egui).
- [x] **M3: native BCn upload** — DONE 2026-10-12. BC1–BC7 DDS bytes upload
      straight to VRAM (no CPU decode) when the device supports
      `TEXTURE_COMPRESSION_BC` and the image fits one texture; the CPU decode
      is the fallback (unsupported/float formats, oversize, no feature). No new
      pipeline was needed — a wgpu pipeline does not depend on the sampled
      texture's format, only the texture does; the tile layout and single
      linear sampler are unchanged (all BC unorm/snorm formats are filterable).
      A compressed layer is one non-sRGB BC texture; copy/save/edit re-decode
      from disk (the worker hands back no RGBA8 for a compressed layer).
      Plus the **colour-space model**: `ColorSpace` (Srgb/Linear) on the image,
      chosen to match the display pipeline (egui is a non-sRGB, gamma-space
      target), so linear sources (EXR/HDR, non-sRGB DDS) are encoded to display
      in `image.wgsl` — the field EXR/RAW and §9 ICC build on. Info bar shows
      the colour space.
- [x] **GPU test harness** (`Roadmap.md` §5b) — DONE 2026-10-11: headless
      adapter + `#[cfg(test)] mod gpu_tests` in `render.rs`; tests the real
      render pipeline builds and a compute pass round-trips, skipping cleanly
      with no adapter (`pollster` dev-dep drives the async wgpu calls). AGENTS'
      "don't unit-test wgpu" rule replaced with "use the harness".
- [ ] **Compute-shader image filters (future)** (`Roadmap.md` §5b) — move
      convolution/effect filters to a `wgpu` compute pass (input texture →
      output texture), faster than CPU especially for convolution. **Keep the
      CPU implementations as the fallback path**; resampling (resize) stays
      CPU. Not a now-mandate.
- [ ] **M4: HDR + tonemap** — `Rgba32Float` (needs `float32_filterable` or
      nearest sampling), EXR and BC6H float paths, exposure/gamma tonemap.
      BC6H currently uses the RGBA8 path; revisit `imagef32_from_dds`.
- [ ] **M5: cubemap view** — true pan-around-the-sphere navigation (decided
      2026-10-11: face buttons now, 3D view here).
- [ ] **Normal-map preview mode** — reconstruct `B = sqrt(1 - R² - G²)` and/or
      single-channel views; trivial now that the image goes through our own
      shader. (Sample: `mountainslab02_n.dds`.)

## Deferred / needs a spec

- [ ] **Property map mode for lightramp setup** — proposed extra mouse/view
      mode; needs a spec before implementation.
- [ ] **Logging console UI** — `egui_logger 0.10.1` widget if we ever want
      in-app log display; the `log`+`env_logger` backend landed 2026-10-11.
- [ ] **Perf: profiling zones** — `profiling` facade + opt-in
      puffin/tracy backends (Sophia's pattern) when the shader pass needs
      frame-time work.

## Verify

- [ ] **Image copy/paste across apps on Wayland** — Sophia's notes claim
      arboard is "text-only on Wayland". Evidence update 2026-10-11: Photon's
      app-internal copy→paste works on this Wayland session (buttons), so the
      data-control path functions; still worth one manual cross-app test
      (Photon → GIMP/Krita and back) to confirm other apps accept our image
      MIME offers. Fallbacks if broken: XWayland or wl-clipboard-rs.

## Dependency notes

Crate picks and pins are taken from Sophia (same egui 0.34.3 line). Gotcha:
egui-ecosystem crates have offset versions; check the crate's egui req
against our lock before adding (e.g. `egui_plot 0.35.0` targets egui 0.34).
Sophia's vendored-fork pattern (`VENDOR_NOTES.md` + `[patch.crates-io]`) is
the escalation path if a picked crate needs a fix upstream lacks.

- [ ] **egui-ecosystem (remaining)**: `notify-debouncer-full 0.7` (folder
      watching, re-exports notify), `profiling 1` (+opt-in puffin/tracy),
      `egui-notify 0.22` (toasts — only if we need queued messages; house
      rule: plain text), `egui_taffy 0.12` (flexbox — only if native layout
      stops coping). Vetted-but-inapplicable: `egui_dock`, `egui-arbor`
      (revisit for the texture sidebar), `egui_plot`, `egui_code_editor`,
      `egui_field_editor`, `ab_glyph`, `mimalloc`.
- [ ] **Roadmap: reintroduce removed deps where they earn their keep** —
      `tokio` (async walk/IO, file watching substrate), `thiserror` (typed
      loader errors; first job: replace the `"Failed to decode"` prefix
      classification between `image_loader.rs` and `app/loader.rs`),
      `bytemuck` (GPU staging in the shader pass), `glam` (cubemap view
      math), `half` (f16/HDR), `chrono`/`jiff`
      (human-readable dates), `humansize` (file sizes). Removed as unused
      2026-10-11 except where noted. **`rayon` reintroduced 2026-10-11** (batch
      engine driver).

---

## Done

### 2026-10-12 (UI polish + cleanup pass)
- **Dracula theme fixed** — it was only half-applied. egui buttons and
  checkboxes read `weak_bg_fill`/`bg_fill` per widget state, and the theme
  set only `bg_fill`, so button backgrounds fell back to egui's default grey
  and the `hovered.bg_fill = purple` override was dead; selected items
  (sidebar row, layer buttons, combo entries) used egui's default blue
  `selection`. Now both fills are set per state (rest `#44475a`, hover
  `#6272a4`, pressed/selected accent `#bd93f9`), `selection.bg_fill` is the
  accent with dark text, `fg_stroke` is Dracula foreground so labels are not
  default grey, and `extreme_bg_color` matches. Palette lives as `pub(crate)`
  consts in `app/theme.rs`, shared with the sidebar and overlay.
- **Light theme gets the font size** — the font-size slider only re-applied in
  dark mode, so it was a no-op in light mode. Both themes now go through
  `PhotonApp::apply_theme(dark, size)` (startup, font-size change, theme
  toggle).
- **Toolbar**: the sidebar toggle no longer shares the prev/next caret icons
  (`SIDEBAR_SIMPLE`), and the mouse-mode button, gear, sidebar and prev/next
  carry hover text.
- **Comment cleanup**: removed narration comments in `ui/overlay.rs`; the
  Settings sharp-scaling comment claimed the toggle "has no effect until then"
  — sharp scaling has been live since M1/M2, so that was stale. Overlay icon
  spacing unified (8px).
- **Window**: minimum inner size 640×480.
- No logic changes; 139 tests still green, build and clippy warning-free.

### 2026-10-12 (shader M3: native BCn upload + colour-space model)
- **Native BC upload** (Roadmap §5b, shader M3): a BC1–BC7 DDS uploads its
  compressed mip-0 surfaces straight to VRAM when the device supports
  `TEXTURE_COMPRESSION_BC` and the image fits one texture — no CPU decode.
  `DecodedFormat { Rgba8, Bc(BcKind) }` + `DecodeOptions { allow_bc, max_dim }`
  + `load_image_opts`; `load_image` still returns RGBA8 for batch/clipboard/save.
  The render path is format-aware: RGBA8 keeps chunked tiling, a BC layer is one
  non-sRGB BC texture (`bytes_per_row = block_copy_size * ceil(width/4)`).
  Fallbacks: no feature, Bc6h/float, uncompressed, oversize → CPU decode.
  No new pipeline or WGSL sampler change (a pipeline doesn't depend on the
  sampled texture's format).
- **Colour-space model**: `ColorSpace { Srgb, Linear }` on `DecodedImage`,
  carried to `RenderImage` and `PhotonApp::current_color_space` (info-bar
  readout). Verified the pass is display/gamma-space (egui prefers a non-sRGB
  surface and emits sRGB-encoded values), so an sRGB image is stored in a
  **non-sRGB** texture and passed through, and a **linear** source (EXR/HDR,
  non-sRGB DDS) is encoded to display in `image.wgsl` (`gamma_from_linear_rgb`
  + the `Globals::linear_source` flag) before tint. Existing appearance is
  unchanged for PNG/JPEG/legacy-DXT (sRGB); non-sRGB-flagged DDS and EXR now
  render correctly (they were previously shown linear-as-if-encoded).
- GPU test uploads a hand-built BC1 texture and reads back the sampled pixel
  (skips without the BC feature). Tests: 131 → 139.
- Deviation from the plan: no `*UnormSrgb` texture variants — in a gamma-space
  pass they would hardware-linearise and show wrong; the flag drives a shader
  encode instead. The colour-space field is the substrate for §9 ICC and M4 HDR.

### 2026-10-12 (editing + viewing polish + batch polish)
- **Interactive editing wired** (Roadmap §4): `apply_edit`/`undo_edit`/`redo_edit`
  apply rotate/flip/crop/grayscale/negative to the displayed image and re-upload
  it built on the main thread — deliberately *not* through the loader worker and
  kept out of the LRU cache, so the cache holds only unmodified disk images and a
  cache-hit copy/save always matches the screen. Undo history cleared on target
  change / paste. Eight edit commands + Undo (Cmd+Z) / Redo (Cmd+Shift+Z) added to
  the registry. The temp `#[allow(dead_code)]` on `mod edit` is gone.
- **Fit window / centre / zoom-to-desktop + `ZoomLock`** (Roadmap §3): `FitWindow`
  (don't / enlarge-if-smaller / always) with monitor clamping, `centered_window_pos`,
  `zoom_to_desktop`, a persisted `fit_window` setting + Settings combo, and View-menu
  actions. `ZoomLock` wired: persisted `lock_zoom` + Settings checkbox + View toggle,
  overriding the navigation reset policy. The `#[allow(dead_code)]` on `mod view` is gone.
- **Registry-driven menu bar**: File/Edit/View menus built from `Command::all()` +
  `menu_group()` + `command_enabled()`, so the registry's name surface is live and a
  new command joins a menu by declaring its group. Removes the `impl Command` allow.
- **Batch polish** (Roadmap §5/§5b): the seven convolution filters as the retained CPU
  path (Appendix A kernels), `SaveOptions` (JPEG quality + PNG compression) threaded
  through encode/convert_file/run_batch, and a determinate progress bar
  (`run_batch_with_progress` + `BatchMessage::Progress`).
- Tests: 103 → 128.
- Deviation from the plan: edits are re-uploaded on the main thread rather than through
  a new `WorkerMessage::Replace`, to avoid caching an edited texture under the original
  path (which would desync copy/save from the display).
- Not done (still open): the interactive `OverwritePolicy::Ask` dialog; WebP/TIFF save
  options (unavailable in `image` 0.25).

### 2026-10-11 (render fixes)
- Canvas drew nothing: `callback_resources` (TypeId-keyed) had `Arc<ImageRendererState>`
  inserted but looked up as `ImageRendererState` → both callback hooks bailed
  silently. Now via `insert_into`/`from_resources`.
- Wrong scale / zoom-in clamped to the viewport: egui-wgpu sets the callback
  viewport to rect∩clip. Absolute-pixel coords + a full-target viewport override
  in `paint` (scissor does the clipping). Tile quads carry their sub-rect, so
  oversized tiling places tiles.
- Removed the nearest sampler; sharp scaling snaps the sample coord to the texel
  centre in `image.wgsl` over the one linear sampler.
- New headless offscreen draw test (103 tests).

### 2026-10-11 (batch 4: GPU harness, batch UI, editing core)
- **GPU test harness** (`render.rs` `gpu_tests`): headless adapter; the real
  render pipeline is asserted to build on the GPU and a compute pass round-trips
  — both skip with no adapter. AGENTS' "don't unit-test wgpu" rule replaced.
- **Batch Convert window** (`ui/batch_window.rs`): file list, format/quality/
  folder, operation chain, non-blocking `rayon` run, per-file status.
- **Editing core** (`app/edit.rs`): rotate/flip/crop/grayscale/negative +
  bounded whole-image undo (canvas wiring pending).
- 102 unit tests; build and clippy warning-free.

### 2026-10-11 (batch 3: viewing + batch engine)
- **Concern split** — `AppSettings`/`SortMode`/`ZoomReset` moved to
  `app/settings.rs` (re-exported) to cut `app.rs` contention.
- **Pixel readout + tile-repeat view** (Roadmap §3).
- **Batch conversion engine core** — `app/batch.rs` (Roadmap §5; `rayon`
  `par_iter`).
- 80 unit tests; build and clippy warning-free.

### 2026-10-11 (batch 2: viewing/extras)
- **Exif auto-rotate on load** — orientation applied at decode in
  `image_loader.rs` (image crate API; dims swap for 90/270).
- **Fit-mode setting + `Fit`/`100%` toolbar buttons** — `AppSettings.fit_mode`
  drives the canvas fit; buttons re-fit / reset to 100%.
- **Codecs** — `image` features tga/pnm/ico/hdr enabled (in-crate, no new
  deps); `dir_walker::SUPPORTED_EXTENSIONS` is now the single source of truth
  and the Open dialog reuses it.
- 60 unit tests; build and clippy warning-free.

### 2026-10-11 (roadmap intake + first parallel workstreams)
- Docs: `Roadmap.md` added (forward feature map; scope fixed by a scoping
  review — see its §0/§12). `Todo.md` re-pointed at it.
- **Concern split** (mechanical): `src/main.rs` reduced to bootstrap;
  panel bodies moved to `ui/{toolbar,info_bar,sidebar,settings_window,overlay}.rs`,
  the shortcut scanner to `input.rs`. 29 tests stayed green.
- **Command registry** (`Roadmap.md` §1, step 1): `Command` enum +
  `default_bindings()` + `PhotonApp::run_command()`; `take_commands()` scans
  the table; `main.rs` dispatches through it. Registry is now the single
  key→action source. CLI/params/menus/rebinding are the follow-ups.
- **Typed `DecodeError`** (`Roadmap.md` §8): replaces the string-prefix
  classification.
- **`app/view.rs`** (`Roadmap.md` §3): pure `FitMode` / `fit_zoom` /
  `zoom_to_fill` / `ZoomLock` geometry (wiring pending).
- 51 unit tests; build and clippy warning-free.

### 2026-10-11 (shader pass)
- Image canvas renders through a custom wgpu pipeline (WGSL, M1/M2):
  shader-based sharp scaling (nearest when magnified, per `use_sharp_scaling`),
  per-frame uniform viewport/tint transform.
- Chunked viewing: oversized images split at the GPU's max texture dimension
  and reassembled in image space; the oversized-image rejection is gone.
- Texture lifecycle simplified to wgpu refcounting (`Arc<RenderImage>`);
  manual free bookkeeping deleted. Ctrl+C copies the error message text
  while the error overlay is up.

### 2026-10-11 (review-fix pass)
- Sampler-based sharp scaling reverted (samplers stay linear; the feature is
  shader-pass work — the dual-sampler approach wasn't the design).
- Fullscreen on F11 instead of Enter.
- Global no-selectable-text rule (fixes offset selection cursor on overlay
  text and the sidebar jump on modifier presses — same root cause).
- Copy / save-selection / save-as no-op while the error overlay is up.
- GPU texture limit raised to 16384 (clamped per adapter); tiling of truly
  oversized images remains a shader-pass item.

### 2026-10-11 (quick-wins pass)
- `use_sharp_scaling` wired (was dead since introduction): every layer
  registered with linear + nearest samplers, live toggle in Settings.
- Neighbor-pixel intermediate display removed: while loading, the previous
  image stays under the spinner until the target arrives.
- Drag-and-drop open (files and folders, via `open_path`) and Enter →
  fullscreen toggle (Plan.md input parity).
- Folder watching via `notify-debouncer-full`: debounced re-walk on changes;
  the displayed image stays selected and is not reloaded unless its mtime
  changed on disk; vanished files land on the nearest entry.

### 2026-10-11 (follow-up passes)
- Ctrl+C/V shortcuts fixed (egui-winit rewrites them into
  `Event::Copy`/`Paste`; dispatch listens for the command events with key
  fallbacks; synthetic-input tests).
- egui-ecosystem adoptions: `egui-phosphor 0.12` icons, `log`+`env_logger`
  (filter `photon=info,warn`), `rfd 0.17`, `lru 0.18.5`.
- Config via serde + RON (`config.ron`, `directories` crate,
  `PHOTON_CONFIG_DIR` override); `ZoomReset` setting (always /
  dimension_change / never); GUI-vs-config parity for `dark_mode`.
- All DDS layers viewable: per-face textures, bottom-bar face/layer
  selector, copy/save follow the displayed layer (cap 64 layers).
- Oversized-texture guard in the worker (max_texture_dimension_2d +
  zero-extent), error overlay instead of async wgpu failure.
- Zoom resets only on dimension change by default (`ZoomReset`).
- Copy/save reuse the loader's decoded bytes (`active_rgba`), re-decode only
  as fallback.
- Save results surface as transient bottom-bar text (plain, per house rule).
- Open-dialog filter includes dds/exr. Unused deps removed.
- Warning-free build; egui 0.34 panel API migration.

### Earlier passes
- Save / Save Selection As (background encode, format picker).
- Loader errors surfaced in the UI (`StatusOverlay::Error`/`UnsupportedFormat`).
- DDS decode via image_dds (BC1–BC7, DX10).
- Sidebar sorting UI; keep-zoom option (superseded by `ZoomReset`).

---

## Out of scope / reference only

- `examples~/`, `gemini/`, `iced.md`, `Plan.md`, `scratch/`, `agents/all.txt` —
  AI scratch and historical design docs. Not part of the product. (`Plan.md` is
  the original design vision and predates the shader pass; trust `src/` over it.)
- `Roadmap.md` — the forward feature map (tracked, unlike the files above).
  Long-horizon plan; this file pulls the near-term items out of it.
- `DDS-investigation.md` — investigation record; its short-term mitigations
  shipped (size guard + per-layer decode). `/tmp/ddstest` is obsolete.
- GUI framework decision: **stay on egui** (reviewed 2026-10-11 against
  iced/Slint/gpui — `PaintCallback` is the sanctioned shader hook; Slint is
  GPL-licensed; iced's Shader widget is the only credible alternative if
  egui's callback path ever proves fighty).
