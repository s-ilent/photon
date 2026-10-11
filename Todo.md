# TODO — Photon

Living worklist, re-prioritized 2026-10-11 after the config/logging/shortcut
pass. Per CLAUDE.md, do not delete pre-existing dead code opportunistically;
list it here and address it deliberately.

The long horizon — the full forward feature map, grouped by capability — lives
in `Roadmap.md`. This file is the near-term, moving slice of it; when something
on the roadmap comes into focus, add it here with a concrete plan.

---

## Recommended order

What's next, in the order I'd do it:

1. **Shader pass M3: native BCn upload** — compressed DDS bytes straight to
   VRAM (no CPU decode), per-format pipeline variants. The pass machinery
   (M1/M2) landed 2026-10-11; this is the next chunk of the arc.
2. **Command registry + params serialisation** (`Roadmap.md` §1) — the spine
   that makes key-configuration, batch, and a scripting CLI cheap; build it
   before those three so they share one dispatch table.
3. **Batch converter** — `rayon` `par_iter` convert/save with a real
   "Convert…" dialog. Independent of the shader work; can slot in anytime
   (richer design in `Roadmap.md` §7).
4. **Near-term viewing polish** (`Roadmap.md` §3) — fit modes, zoom set
   (100 % / to-desktop / lock), Exif auto-rotate on load, status-bar pixel
   readout, tile-repeat ("seamless texture") view. Small, high-visibility.
5. **Texture inspection sidebar** — DEFERRED: the user is designing this.
   Implement only against their design when it's ready.
6. **Property map mode** — when a spec exists.

---

## Features & behavior gaps

- [ ] **Texture inspection sidebar** — DEFERRED by the user (2026-10-11):
      they are designing it; implement only against that design. Covers mip
      levels, texture-array entries, and cubemap faces in one tree,
      superseding the bottom-bar face buttons. Needs a mip-decode decision:
      the pipeline decodes/uploads mip 0 only
      (`decode_layers_mipmaps_dds(.., 0..1)`), so a mip selector needs either
      per-mip decode on demand or the full chain at load.
- [ ] **Batch converter** (Plan.md) — `rayon` `par_iter` convert/save; fonts
      a real "Convert…" dialog. Depends on nothing; medium-large.
- [ ] **ICC / color profiles** (Plan.md) — read ICC and apply in the shader
      pass; deferred with it.
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
      F11 fullscreen and all others become remappable then. Substrate is the
      command registry (`Roadmap.md` §1).
- [ ] **Exif auto-rotate on load** (`Roadmap.md` §3) — orient before fit and
      scrollbar math; needs `kamadak-exif` (also feeds the metadata panel).
- [ ] **Fit modes + zoom set** (`Roadmap.md` §3) — don't-fit / reduce-only /
      always-fit, fit-window per axis + centre, zoom 100 % / to-desktop / lock.
- [ ] **Status-bar pixel readout** (`Roadmap.md` §3) — RGB / alpha / HTML hex
      under the cursor.
- [ ] **Content-based format detection** (`Roadmap.md` §5) — sniff bytes,
      honour them when the extension lies.
- [ ] **Decode-error enum** (`Roadmap.md` §5) — replaces the `"Failed to
      decode"` string-prefix classification between `image_loader.rs` and
      `app/loader.rs`; lands with the `thiserror` reintroduction below.

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
- [ ] **M3: native BCn upload** — BC1–BC7 DDS bytes straight to VRAM (no CPU
      decode); per-format pipeline variants (sRGB/blocky sample types), tile
      reuse across faces. Fallback to CPU decode stays for exotic formats.
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
      math), `half` (f16/HDR), `rayon` (batch converter), `chrono`/`jiff`
      (human-readable dates), `humansize` (file sizes). Removed as unused
      2026-10-11 except where noted.

---

## Done

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
