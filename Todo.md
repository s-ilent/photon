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

1. **Metadata follow-ups** (`Roadmap.md` §9) — Exif panel and Image Information
   landed 2026-10-13; what remains is **Exif/IPTC/XMP preservation on save**
   (saver/batch encoder threading), **ICC colour management** (shader arc; the
   `ColorSpace` field is the substrate), and **DPI** (not exposed by `image`
   0.25 — needs a direct `png` dependency and per-format header parsing).
2. **Mouse-mode mapping** (`Roadmap.md` §10) — each button, wheel up/down,
   double-click → a mouse mode. The keyboard half of rebinding landed
   2026-10-14 (Settings → Shortcuts, `shortcut_overrides` in config.ron);
   the mouse half restructures canvas input handling and wants its own pass.
3. **Shader M5: cubemap view** and the **normal-map preview** — the remaining
   shader-pass items (M3 BCn and M4 HDR/tonemap both landed 2026-10-13).
4. **Batch rename + the remaining batch items** (`Roadmap.md` §5) — the
   interactive overwrite dialog (recipe import/export landed 2026-10-13).
5. **Full browser** (`Roadmap.md` §7) with the persistent thumbnail cache.
6. **Texture inspection sidebar** — DEFERRED: the user is designing this.
   Implement only against their design when it's ready.
7. **Property map mode** — when a spec exists.

Done recently (2026-10-12/13): viewing polish (§3), the editing core (§4), the
batch polish (§5), shader **M3 native BCn + the colour-space model** and
**M4 HDR + tonemap** (§5b), and the **§1 registry spine** (one `ImageOp`
vocabulary + the recipe text form + batch recipe import/export) are all
complete; see the Done log. The **§10 chainable CLI + key rebinding** landed
2026-10-14.

---

## Features & behavior gaps

- [ ] **Texture inspection sidebar** — DEFERRED by the user (2026-10-11):
      they are designing it; implement only against that design. Covers mip
      levels, texture-array entries, and cubemap faces in one tree,
      superseding the bottom-bar face buttons. Needs a mip-decode decision:
      the pipeline decodes/uploads mip 0 only
      (`decode_layers_mipmaps_dds(.., 0..1)`), so a mip selector needs either
      per-mip decode on demand or the full chain at load.
- [ ] **Info window layout redesign** — the 2026-10-14 bounding (window width
      cap + truncated value labels) is a stopgap; the user rejected a size cap
      as the solution. The real fix is a layout that adapts — long values wrap,
      the window stays resizable — rather than clamping. Redesign when next
      touched.
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
      shader pass; deferred with it. *(The Image Information window already
      reports whether a profile is present and its size.)*
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
- [x] **Key configuration** — DONE 2026-10-14 (keyboard half, Roadmap §10):
      `shortcut_overrides` deltas in config.ron applied over the defaults, and
      the Settings → Shortcuts section (capture-a-key with Esc to cancel,
      conflicts auto-unbind last-wins, Clear/Reset All). Copy/Paste always
      answer Ctrl+C/V — egui-winit rewrites those keys into command events —
      so rebinding manages the extra raw-key binding. The mouse-mode mapping
      is the remaining half (recommended order item 2).
- [ ] **Layout persistence** (`UI-DESIGN.md` §1) — panel sizes, open windows
      and splitter positions survive restart via egui memory persistence
      (`persistence` + eframe storage); settings currently carry only app
      options, not layout.
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
- [x] **M4: HDR + tonemap** — DONE 2026-10-13. EXR/HDR and float/BC6H DDS decode
      to half-float (`DecodedFormat::Float16`, `Rgba16Float` textures — filterable
      in core WebGPU with no device feature, chosen over `Rgba32Float` +
      `float32_filterable` to avoid a capability branch) so values above 1.0
      survive. `Globals::hdr_source`/`exposure` drive the shader: exposure (EV,
      persisted setting + slider) then Reinhard, then the M3 display encode.
      Info bar shows an `HDR` chip. Copy/save of HDR still re-decode to 8-bit.
- [ ] **M5: cubemap view** — true pan-around-the-sphere navigation (decided
      2026-10-11: face buttons now, 3D view here).
- [ ] **Normal-map preview mode** — reconstruct `B = sqrt(1 - R² - G²)` and/or
      single-channel views; trivial now that the image goes through our own
      shader. (Sample: `mountainslab02_n.dds`.)

## Deferred / needs a spec

- [ ] **UI component kit** (`ui/kit.rs`, `UI-DESIGN.md` §4) — shared helpers so
      dialogs stop hand-assembling widgets: Grid form rows (right-aligned
      labels + colon), dialog scaffolding with live-preview panes, destructive
      buttons, empty states. Introduce when the next dialog (Filter preview,
      batch rename) needs them, and migrate the Settings/Batch windows then.
- [ ] **Property map mode for lightramp setup** — proposed extra mouse/view
      mode; needs a spec before implementation.
- [ ] **Logging console UI** — `egui_logger 0.10.1` widget if we ever want
      in-app log display; the `log`+`env_logger` backend landed 2026-10-11.
- [ ] **Perf: profiling zones** — `profiling` facade + opt-in
      puffin/tracy backends (Sophia's pattern) when the shader pass needs
      frame-time work.

## Verify

- [ ] **Menu + toolbar click-through** (2026-10-14) — menus are now File
      (Open…, Save As…, Save Selection As… | Convert…, Image Information…,
      Settings…), Edit (Undo/Redo | Copy/Paste), Image (Rotate ×3 | Flip ×2,
      Crop | Grayscale, Negative), View (Next/Prev | Fit Image, Actual Size |
      Cycle Mouse Mode, Toggle Sidebar, Fullscreen | Lock Zoom | window-fit
      actions) with section separators. The toolbar is registry-driven too:
      every button dispatches through `run_command` with `command_enabled`
      gating — the toolbar **Save button is now disabled without an image**
      (was always clickable). Shortcuts unchanged. Confirm dispatch and
      enable states on both surfaces.
- [ ] **Image copy/paste across apps on Wayland** — Sophia's notes claim
      arboard is "text-only on Wayland". Evidence update 2026-10-11: Photon's
      app-internal copy→paste works on this Wayland session (buttons), so the
      data-control path functions; still worth one manual cross-app test
      (Photon → GIMP/Krita and back) to confirm other apps accept our image
      MIME offers. Fallbacks if broken: XWayland or wl-clipboard-rs.
      2026-10-14: Ctrl+V with an image clipboard was dead — egui-winit reads the
      clipboard as *text* and returns before emitting any event when that read
      fails, so neither the Paste event nor the key event existed. Fixed by the
      vendored `egui-winit` patch (`VENDOR_NOTES.md`); Ctrl+V → paste now needs
      one more manual confirmation.
- [ ] **Generated fixture set for the Image Information window** (§9) —
      deferred 2026-10-14: manual passes use the user's own images, so this is
      backlog. A generated folder with known header values (plain/palette/16-bit
      PNG, Exif+ICC+XMP JPEG, HDR, DXT5 DDS) so the window's rows can be checked
      against exact expected values. Generator kept at
      `scratch/make_info_fixtures.py` (PIL + numpy); it encodes the gotchas —
      Exif rationals need `PIL.TiffImagePlugin.IFDRational` (`fractions.Fraction`
      silently serialises as UNDEFINED), and a `.hdr` needs new-style RLE with
      literal-run count bytes as-is (a byte above 128 means a repeat run; the
      inverse convention fails with `Wrong length of decoded scanline`). Still
      missing: cubemap/array DDS, which want a throwaway Rust
      example built on `image_dds::ddsfile` (mirroring the in-memory builders in
      the `image_loader.rs` tests); PIL's `iptc=` kwarg did not emit an APP13
      block, so the fixture's IPTC row reads "none".

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
      stops coping). **`egui_extras` adopted 2026-10-14** (TableBuilder for the
      batch window's file and operation tables). Vetted-but-inapplicable: `egui_dock`, `egui-arbor`
      (revisit for the texture sidebar), `egui_plot`, `egui_code_editor`,
      `egui_field_editor`, `ab_glyph`, `mimalloc`, `re_ui 0.33`
      (evaluated 2026-10-14: the one egui-0.34-compatible release, but it is
      Rerun's internal theme/widget crate — drags 7 `re_*` SDK crates incl.
      the entity-db/datastore layer plus ~25 others, ships Rerun's theme
      against our Dracula identity, and is frozen 4 release trains behind an
      internal API that moves at Rerun's train cadence. Its widget *shapes*
      are the catalog to crib into `ui/kit.rs`; the crate itself stays out).
      Sophia layout-crate facts (surveyed 2026-10-14): `egui_taffy 0.12` is
      the **last egui-0.34 line** (0.13→0.35, 0.14→0.36) and `egui_dock
      0.19.1` is the maintained 0.34 docking line — but both needed vendored
      forks at Sophia even on this line (dock: a `render_surface` mode for
      multi-viewport floating windows; taffy: an upstream `request_discard`
      that poisons presented frames), so adoption realistically means the
      vendored copies. `egui-arbor` upstream pins egui 0.31 — adopt via
      Sophia's verified 0.34.3 vendor copy, never from crates.io.
      Layout-crate review (2026-10-14): **`egui_flex 0.6`** is the one worth
      wanting — MIT, single dep (`egui ^0.34`), part of lucasmerlin's
      maintained hello_egui monorepo, ~33k downloads. NOT adopted now: no
      surface fights native layout (toolbar is declarative-native, forms are
      Grid per UI-DESIGN §4). It **supersedes `egui_taffy` as the flex escape
      hatch** if that changes — egui-native, no taffy engine, none of the
      vendored-fork baggage; pin 0.6 (0.7+ needs egui 0.35+).
      **`egui_alignments 0.3.8`** (compatible, MIT, 1 dep) — convenience
      wrappers over egui's own alignment APIs; native copes; crib API shapes
      into `ui/kit.rs` if a need repeats. **`panes 0.21` + `panes-egui
      0.5.1`** (compatible, MIT) — a two-crate foreign layout engine at ~90
      recent downloads and adapter/core version coupling; any dockable-pane
      future is `egui_dock`'s job. **`egui_layout_job_macro 0.1.1`** — hard
      reject: LGPL-3.0-only (conflicts with the MIT/Apache posture under
      static linking), pins `egui==0.34.3` exactly, and Photon has no
      LayoutJob/rich-text usage for it to serve.
- [x] **`egui-winit` vendored fork** (2026-10-14) — `vendor/egui-winit` behind
      `[patch.crates-io]`, one hunk: Ctrl+V with no text on the clipboard falls
      through as a key event so image paste reaches the app. The
      `VENDOR_NOTES.md` pattern; upgrade steps live there.
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

### 2026-10-14 (chainable CLI + key rebinding, Roadmap §10)
- **`--` command line** (`app/cli.rs`): `photon [--switch [arg]]... files...` —
  the op chain applies left-to-right through the same `ImageOp` text form as
  batch recipes (reskinned from `/switch:arg` so both surfaces share one
  serialisation; `to_cli`/`from_cli`/`recipe_text`/`parse_recipe` changed
  skin, not semantics). Ops land as transient viewer edits in one undo step;
  `--save <path>` writes the transformed first image to an explicit output —
  the input is never touched unless named, and parent/extension are
  pre-flighted so bad output paths exit before the GUI. `--settings`,
  `--help`/`-h`; `--` ends switches; `~`/`$VAR` expand (shellexpand). Two or
  more file arguments form a transient playlist (`load_playlist`: argument
  order, no walk/sort/watcher); one path keeps the open-with-folder
  behaviour. Bad invocations exit before the window opens (help → stdout/0,
  errors → stderr/2).
- **Key rebinding**: `shortcut_overrides` deltas in config.ron, applied over
  `default_bindings()` into `PhotonApp::bindings`; Settings → Shortcuts
  captures a key (Esc cancels; the rewritten Ctrl+C/X/V command events are
  mapped back to shortcuts), conflicts auto-unbind last-wins, Clear/Reset
  All. Menus and toolbar tooltips read the live table.
- Deviation from the plan: `--opt=value` also parses in recipe text — the CLI
  funnels both spellings through the one shared parser.

### 2026-10-14 (menu taxonomy + UI design guide + toolbar rewrite)
- **Menu taxonomy** (Roadmap §1): `MenuGroup::Image` takes the eight pixel
  edits, Copy/Paste move to Edit, section separators and the menu bar render
  entirely from the registry (`MenuGroup::ALL`/`title()`,
  `starts_menu_section`, presentation-ordered `Command::all`) with invariant
  tests. `UI-DESIGN.md` is the new interface design authority.
- **Toolbar registry rewrite**: every toolbar button is now a `Command`
  dispatched through one table-driven loop — the five toolbar-only actions
  (Settings, Sidebar, Fit, Actual Size, Save Selection As) became registry
  commands with `None` binding rows, so they also appear in the File/View
  menus. Enable states come from `command_enabled` (toolbar Save is now
  disabled without an image — was always clickable), and tooltips render
  their shortcut hint from the binding table (no more hardcoded "(Tab)"-style
  text that would silently desync when rebinding lands). The mouse-mode
  button stays hand-placed: its label is the live mode name.

### 2026-10-14 (Image Information follow-up: layout, toggle, Ctrl+V, HDR fixture)
- **Info window layout bounded** — `Window::max_width(520)` plus truncated value
  labels (file name, path, Exif values); egui shows the elided full text on
  hover. `Ctrl+I` now **toggles** the window (was open-only) and the toolbar
  gains an info button next to the Settings gear; the toggle has a regression
  test.
- **Ctrl+V on an image clipboard** never reached the app: egui-winit reads the
  clipboard as *text* and `return`s before emitting anything when that read
  fails. Fixed by **vendoring `egui-winit`** (`vendor/`, `[patch.crates-io]`,
  one hunk: fall through to the key event when there is no text) — the
  registry's raw-key fallback then fires and Photon reads the image itself.
  `VENDOR_NOTES.md` documents the hunk and the upgrade steps.
- **Manual-test fixtures**: the generated `.hdr` was invalid — new-style RLE
  literal runs use the count byte as-is (a byte above 128 is a repeat run), not
  `128 + n`; regenerated and verified through the loader's exact decode path.
  The generator stays at `scratch/make_info_fixtures.py`.

### 2026-10-13 (Image Information + Exif panel, Roadmap §9)
- **`app/image_info.rs`**: a headers-only probe — one decoder parse yields
  format, colour depth (incl. sub-8-bit/indexed layouts), alpha, and the raw
  Exif/ICC/XMP/IPTC blocks; `.dds` goes through `image_dds` (the `image` crate
  cannot read BC4/5/7) for dimensions + compression family. `jiff` dates,
  `humansize` sizes.
- **`ui/info_window.rs`**: File/Image/Metadata rows, cubemap/array frame counts,
  colour space + HDR, and the Exif tag table; probes on a background thread and
  re-probes when the target changes. Opened by `Command::ImageInformation`
  (File menu, `Cmd/Ctrl+I`).
- **`app/exif.rs`**: kamadak-exif wrapped into IFD/tag/value rows (IFD named via
  `tag.context()`), capped at 200, malformed blocks yield none. Dep added:
  `kamadak-exif 0.6.1` (packaged as `exif`).
- Tests: 153 → 165. Still open in §9: Exif/IPTC/XMP preservation on save, ICC
  colour management, DPI (unavailable in `image` 0.25).

### 2026-10-13 (registry spine: ImageOp + recipe text, Roadmap §1/§5)
- **One operation vocabulary**: `app/image_op.rs` holds `ImageOp` (directional
  rotates, flips, grayscale/negative, `Resize{w,h,filter}`, convolution filters,
  crop-as-margins), `apply()`, and the moved `ResampleFilter`/`FilterKind`/
  `convolve`. The duplicated `batch::Operation`/`apply_operations` and
  `edit::EditOp` are gone — transforms have the single `edit::Rgba`
  implementation, resize/filter stay on `imageops`/`convolve`. The editor and
  the registry dispatch `ImageOp`s directly (crop-from-selection builds
  margins); batch gains crop at the engine level.
- **The Appendix-B text form**: `to_cli`/`from_cli`/`recipe_text`/`parse_recipe`
  — the serialisation the CLI (§10) and rebinding will reuse. Documented
  superset: `/resize:(W,H[,filter])` (Appendix B has no filter slot); `%` and
  free rotation are rejected with clear errors.
- **Batch recipe import/export** (§5's remaining item): the Convert window shows
  the chain's recipe text (per-step switch tooltips), copies it to the clipboard
  and imports from it. No TextEdit, deliberately — `input.rs` takes Ctrl+C/V at
  the app layer. A failed import reports an error and leaves the chain intact.
- Tests: 145 → 153 (round-trips, Appendix-B parsing, crop/chain application).
- Deviation from the plan: the recipe UI landed with the spine instead of via a
  subagent — it is the parse side's only consumer, and landing it separately
  would have meant a warning-bearing intermediate commit.

### 2026-10-13 (shader M4: HDR + tonemap)
- **Float/HDR path**: EXR/HDR decode via `into_rgba32f()` (was clamped through
  `to_rgba8`) and float/BC6H DDS via `SurfaceRgba32Float`, packed to
  `DecodedFormat::Float16` (LE `f16`) and uploaded as `Rgba16Float` — filterable
  in core WebGPU with no device feature, so no capability branch (chose half
  over `Rgba32Float`+`float32_filterable` for exactly that reason; halves VRAM
  for big cubemaps).
- **Tonemap**: `Globals` gained `hdr_source`/`exposure` (the struct stays 64
  bytes); the shader applies exposure (EV, persisted setting + Settings slider)
  then Reinhard, then the M3 display encode. 8-bit linear sources still get
  gamma-encode only; sRGB sources still pass through.
- Info bar shows `Linear`/`sRGB` plus an `HDR` chip. Smoke-verified: a 4×
  gradient EXR renders tonemapped (not clipped), logged as
  `Float16`/`Linear`. GPU test asserts the tonemapped byte (231) and that +1 EV
  brightens it (242). Tests: 139 → 145.
- Known limit (open): copy/save of an HDR image still re-decode to 8-bit; the
  tonemap operator is fixed to Reinhard.

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
