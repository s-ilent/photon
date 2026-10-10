# TODO — Photon

Living worklist, re-prioritized 2026-10-11 after the config/logging/shortcut
pass. Per CLAUDE.md, do not delete pre-existing dead code opportunistically;
list it here and address it deliberately.

---

## Recommended order

What's next, in the order I'd do it:

1. **Shader pass milestone 1** — PaintCallback parity spike (see milestones
   below). The long arc that unblocks ~5 other items.
2. **Imagine-style texture sidebar** — after the shader work decides how mips
   get decoded/uploaded.
3. **Batch converter** — `rayon` `par_iter` convert/save with a real
   "Convert…" dialog. Independent of the shader work; can slot in anytime.
4. **Property map mode** — when a spec exists.

---

## Features & behavior gaps

- [ ] **Imagine-style texture sidebar** — mip levels, texture-array entries,
      and cubemap faces in one tree (user request, modeled on the Imagine
      viewer). Supersedes the bottom-bar face buttons. Needs a mip-decode
      decision: the pipeline decodes/uploads mip 0 only
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
- [ ] **Tiled/chunked viewing of very large images** — the GPU limit is now
      requested at 16384 (clamped per adapter), but e.g. 2048×12288-class
      stacked textures or 32k panoramas still exceed it. True tiling
      (virtual texturing or oversized-texture re-upload) belongs to the
      shader pass; the oversized guard error remains the fallback.
- [ ] **Key configuration** — user-configurable shortcut map (config.ron);
      F11 fullscreen and all others become remappable then.

## Shader pass milestones (the long arc)

Upload textures as their native format, do gamma/tonemap/sharp filtering on
the GPU via `egui_wgpu::CallbackTrait` (feasibility reviewed 2026-10-11 —
viable and architecturally clean). Unblocks: correct sharp scaling, gamma,
HDR, normal-map preview, cubemap 3D view, compressed-VRAM DDS.

- [ ] **M1: parity spike** — render the active RGBA8 texture through a
      PaintCallback with identical visuals. Decides the color-space question
      (Unorm vs UnormSrgb target) against real output.
- [ ] **M2: sampling + gamma** — sharp/pixelated scaling implemented in the
      shader (samplers stay linear per design — the sampler-based approach
      was reverted 2026-10-11), explicit gamma handling in the shader.
- [ ] **M3: native BCn upload** — BC1–BC7 DDS bytes straight to VRAM (no CPU
      decode); per-format pipeline cache. Fallback to CPU decode stays.
- [ ] **M4: HDR + tonemap** — `Rgba32Float` (needs `float32_filterable` or
      nearest sampling), EXR and BC6H float paths, exposure/gamma tonemap.
      BC6H currently uses the RGBA8 path; revisit `imagef32_from_dds`.
- [ ] **M5: cubemap view** — true pan-around-the-sphere navigation (decided
      2026-10-11: face buttons now, 3D view here).
- [ ] **Normal-map preview mode** — reconstruct `B = sqrt(1 - R² - G²)` and/or
      single-channel views; trivial once M2 exists. (Sample:
      `mountainslab02_n.dds`.)

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
  AI scratch and historical design docs. Not part of the product.
- `DDS-investigation.md` — investigation record; its short-term mitigations
  shipped (size guard + per-layer decode). `/tmp/ddstest` is obsolete.
- GUI framework decision: **stay on egui** (reviewed 2026-10-11 against
  iced/Slint/gpui — `PaintCallback` is the sanctioned shader hook; Slint is
  GPL-licensed; iced's Shader widget is the only credible alternative if
  egui's callback path ever proves fighty).
