# Roadmap — Photon

Forward-looking feature map. Where `Todo.md` is the near-term, moving
worklist, this file is the long horizon: the shape of the product we want,
grouped by capability, with the concrete behaviours and algorithms worth
copying. Check items off here as they land and move the durable detail into
`Todo.md` / the code.

**Provenance.** This map is distilled from a study of a mature, long-running
reference image viewer and toolkit that we use as a feature benchmark. It is a
clean-room reimplementation of *behaviour*: we copy what the tool does and how
it is wired, never its code, its toolbar artwork, or its translated string
tables. When a detail below reads like a spec ("÷9", "support 3.0"), treat it
as a verified behaviour of the reference, not a Photon decision — Photon may
diverge where our architecture makes something cleaner.

**How to read the checkboxes.** `[x]` = shipped in Photon. `[ ]` = not yet.
Items that already exist in `Todo.md` are cross-referenced rather than
duplicated.

---

## 1. The architectural spine: everything is a command

The single most valuable idea in the reference is a **central command
registry**. Every user-visible operation is one entry:

| Field | Meaning |
| --- | --- |
| command id | stable integer (used by menus, config, batch files) |
| handler | one function that does *everything* for that command |
| enable predicate | when the command is available (image loaded, animated, selection present, …) |
| name | human label |
| cli name | the switch that names it in the scripting language |

Menus, toolbar buttons, keyboard shortcuts, mouse modes, the command line and
the batch list all dispatch through this same table. That is the whole reason
a single shortcut/rebind system can reach every feature, and why the CLI and
batch recipes are the same thing.

**Handler contract.** `handler(target, command_id, params, flags)`:

- **Interactive** (`flags = 0`): open the command's options dialog, collect
  parameters, then dispatch *execute-with-these-params* to the owner (the
  viewer really edits; a batch dialog merely accepts and records them).
- **Replay** (`flags` has the silent bit, params pre-filled): skip the dialog
  and apply directly.

One handler, two modes, zero duplicated code between the menu action and the
batch action.

**Why this matters for Photon.** This is the design that makes the
"customisable shortcuts" item in `Todo.md` (`Key configuration`) cheap and
makes the `Batch converter` fall out for free: both are just the registry
driven from config and from a queue. Adopt the registry *before* building
batch, rename, or the scripting CLI, so those three share one spine.

- [ ] Define `Command { id, name, cli_name, enable: fn(&AppState) -> bool, run }`
      and a registry table.
- [ ] Route menu, shortcuts, and (later) CLI + batch through the registry.
- [ ] Params are a serialisable struct per command with `to_cli()` /
      `from_cli()`, so a parameter blob can always be written as text and
      read back (`Cmd/Ctrl` config, batch import/export, CLI all agree).

## 2. Image model: frames on top of layers

Photon already models **layers** (DDS mip/array/cubemap entries) — see the
texture sidebar item in `Todo.md`. The reference adds an orthogonal axis:
**frames/pages**. An image is a *list of frames* (animated GIF/WebP, multipage
TIFF, ICO/DCX).

- Edits walk the whole frame list: duplicate each frame, process it, swap the
  new list in, push one **undo entry**. Every edit is therefore frame-aware
  and undoable by construction.
- Undo/redo is a stack of whole-list swaps, with a configurable buffer size
  and a command-history dialog.

- [ ] Frame/page list per image; frame submenu (play/pause, next/prev,
      extract current/all).
- [ ] Undo stack as whole-frame-list swaps; command-history dialog.
- [ ] Page navigator panel (thumbnail strip).

## 3. Viewing — canvas and window

Existing in Photon: zoom-at-cursor, pan, next/prev, fullscreen, mouse-mode
cycling, DDS layer/face selection, shader-based sharp scaling, chunked viewing
of oversized images.

Missing, and worth adding (rough value order):

- [ ] **Fit modes**: *don't fit* / *reduce only if larger than window* /
      *always fit*, plus a separate *fit bigger image to window* toggle.
- [ ] **Fit window (per axis)**: *don't* / *enlarge window if smaller than
      image* / *always resize window to image*, plus *center window*.
      Fit-image and fit-window interact; pin the exact interaction by use.
- [ ] **Zoom set**: zoom in/out, 100 %, **zoom to desktop**, **lock zoom**
      (survives navigation).
- [ ] **Tiled / tile-repeat view** — draw the texture repeated in a grid to
      check seamless tiling. *Distinct from Photon's "chunked viewing"*, which
      splits one oversized image at the GPU limit; this multiplies a small one.
      Natural fit for the shader pass (a tile-grid uniform).
- [ ] **Exif auto-rotate on load** — orient before fit/scrollbar math so the
      window sizes correctly; also in thumbnails.
- [ ] **Display-while-loading** with no resize after the real image arrives
      (Photon currently keeps the previous image under the spinner; decide
      whether to add the low-res preview).
- [ ] Transparency display: toggle alpha blending, checkerboard / solid /
      pattern backdrop, and a **mask view** of the alpha channel.
- [ ] **Status-bar pixel readout**: RGB, alpha, HTML hex (click to copy) under
      the cursor.
- [ ] Antialiasing toggle (relevant once the shader pass owns sampling).

## 4. Editing & image processing

All of this rides on the registry (§1) and the frame model (§2), so build
those first.

- [ ] **Resize**: units pixels / percent / standard sizes / current desktop
      size; Half and Double buttons; preserve-aspect; DPI-aware; keeps
      transparency. Resample filters in Appendix A.
- [ ] **Effect filters** with a preview pane: Blur, Soften, Sharpen,
      Gaussian 3×3, Gaussian 5×5, Emboss, Edge, Tint (kernels in Appendix A).
      One generic convolution routine; each filter supplies kernel + divisor +
      bias + an *amount* parameter.
- [ ] **Colour depth**: 1, 4, 8, 16 (5:5:5 and 5:6:5), 24, 32-bit, with
      optional dithering.
- [ ] **Palette**: edit, import, export (raw PAL, JASC, Microsoft, Adobe ACT,
      raw RGB).
- [ ] **Transparency**: colour-key set/remove.
- [ ] **Crop** (undoable, transparency-aware, multi-frame) — Photon has
      selection-crop on copy; this is the destructive editor version.
- [ ] **Rotate**: left/right/180 as distinct commands; **rotate free** with an
      angle and a background colour for the exposed corners.
- [ ] Grayscale / negative / flip — trivial once the pixel pipeline exists;
      the luma weights are in Appendix A.
- [ ] Pixel-format normalisation step on load, so every operation works on a
      known format regardless of the source's depth/palette.

## 5. Codecs & format strategy

Photon ships the `image` crate formats plus DDS (via `image_dds`) and EXR.
The reference's breadth is a menu of candidates, not a to-do — pick by value.

**Cheap wins (pure-Rust crates exist):**

- [ ] QOI (`image` crate), RADIANCE HDR (`.hdr`, `image` crate), TGA/PCX/ICO
      where `image` covers them, PBM/PGM/PPM.
- [ ] **JXL** decode via `jxl-oxide`; **SVG/SVGZ** via `resvg`.
- [ ] **Lossless JPEG transform** (rotate/flip/90/180/270 without re-encode;
      marker policy keep-all / keep-comment / strip) via `turbojpeg` transforms
      — also exposed as a shortcut-level *lossless rotate*.
- [ ] **Archive browsing**: read images inside CBZ/zip (`zip`), CBR/rar
      (`unrar`), CB7/7z (`sevenz-rust`); treat an archive as a folder to page
      through.
- [ ] RAW via `rawler` / `rawloader` (or `libraw` bindings) — heavy, do later.

**Heavier / platform-bound (lower priority):** HEIF/HEIC (`libheif-rs`),
AVIF (dav1d bindings), JPEG2000 (`jpeg2k`), WebP animation (`webp` /
`webp-animation`), HD Photo/JPEG-XR, BPG, JBIG, multi-page TIFF writing.
Windows metafiles (WMF/EMF) — skip; low value on Linux.

**Cross-cutting codec work:**

- [ ] A fixed **decode-error vocabulary** enum (no error, unsupported type,
      unknown, out of memory, colour not supported, read error, write error,
      invalid data, file not found, aborted, grayscale only, other components
      required, size not supported, can't get information). This is the
      principled replacement for the brittle `"Failed to decode"` string-prefix
      classification noted in `Todo.md` (pair it with the `thiserror`
      reintroduction listed there).
- [ ] **Content-based format detection** — sniff the bytes and honour them
      when the extension lies; offer to rename the incorrect extension.
- [ ] Animated-source → static-output rule for batch (first frame only) is
      already the stated Photon behaviour; keep it and surface it in the
      batch UI.

## 6. Browser & thumbnails

Photon is currently a viewer with a file list; the reference's browser is a
full thumbnail/file manager. Large and self-contained — treat as its own arc.

- [ ] Views: thumbnails, icons, small icons, list, details; sortable, drag-
      rearrangeable columns.
- [ ] Sort by name / extension / dimensions / size / date, descending toggle.
- [ ] Filters: supported-only, show folders, show hidden files.
- [ ] **Persistent thumbnail cache**: on/off, max size, custom directory
      (survives sessions).
- [ ] Optional folder tree, folder history bar, preview pane with splitter.
- [ ] Exif-aware thumbnail rotation; archive icons; browse *into* archives.
- [ ] File ops: new folder, copy/move/delete/rename/properties, drag-and-drop
      everywhere.

## 7. Tool windows

Each is a list-of-files × options pattern; all reuse the command registry (§1)
for their operation chains.

- [ ] **Batch conversion** (already in `Todo.md`): output folder (auto-created),
      target format + per-format *Options*, resize, an **operation chain**, and
      **per-file status** rows. Overwrite policy *ask / yes-to-all / skip*;
      continue-on-error; live progress; UI pumped between files. Recipe
      import/export as CLI text (falls out of §1 / §11).
- [ ] **Batch rename**: rename mask with insertable tokens — file
      name/ext/size/date/time, image width/height/bpp, Exif/IPTC tags, and
      `<Counter.start,step,digits>` (zero-padded); search-and-replace; live
      preview; reorderable list (order matters for counters).
- [ ] **Slide show**: file list (add/remove/up/down/add-from-folder/open/save
      the list), background colour, auto-advance after N seconds or
      mouse/keyboard, loop, randomise, transition effects (fade, horizontal
      line, vertical line, overlap, none, random), full-screen.
      *Skip EXE/SCR export* — not meaningful on Linux.
- [ ] **Screen capture**: whole desktop / active window / client-area-only,
      include cursor, hotkey or automatic-after-N-seconds, output to viewer or
      to a file with auto-created folders. (Linux: XDG portal / `wlr-screencopy`.)
- [ ] **Animation editor**: insert/delete/select frames, per-frame delay,
      loop/play-count, frame properties, save-frame-as, play. Constraint: all
      frames share size and colour depth.
- [ ] **Multiple-page creator**: combine files into one multipage image.
- [ ] **Panorama**: horizontal/vertical stitch by concatenation (no
      feature matching), width/height methods *fit-to-largest / smallest /
      first / custom*.
- [ ] **Print**: printer, orientation, copies, fit-to-page/stretch/custom,
      page range, multipage images.

## 8. Metadata & colour management

- [ ] **Exif** panel (~90 tags: camera, exposure, flash, white balance,
      metering, …) and **preserve Exif on JPEG save**.
- [ ] **IPTC** and **GPS** panels; GPS → user-defined map URL.
- [ ] Image Information: dimensions, colour depth/palette, frames, pages,
      compression, transparency method, file size/date, DPI (+ original size
      from DPI), progressive/interlaced, ICC profile button.
- [ ] **ICC / colour management** (already in `Todo.md`, hooked to the shader
      pass): enable, input profile for untagged images, output profile
      (display / sRGB / custom), rendering intent (perceptual, relative
      colorimetric, saturation, absolute).

## 9. Configuration & input mapping

Photon persists `config.ron` (serde + RON). Extend it toward the reference's
key inventory, grouped roughly:

- **View**: fit modes (image + window), antialiasing, display-while-loading
  (+load-time/frequency readouts), centre image/window, zoom, page navigator.
- **Scrolling**: smooth scroll, scroll step.
- **Misc**: use-undo, ignore-errors, ask-to-overwrite, ask-to-save-at-exit,
  loop directory + notify-at-end, single-instance, save/restore position,
  add-extension-in-capitals, rename-incorrect-extension.
- **Colour management**, **Print**, **Full screen** (hide cursor/scrollbars,
  show filename), **Toolbar** (button size, wrap, flat style).
- [ ] **Per-command shortcut table** and **mouse-mode mapping** (each button,
      wheel up/down, double-click → a mode). This is the `Key configuration`
      item in `Todo.md`; the registry (§1) is its substrate.

## 10. Command-line / scripting

A chainable switch language, executed left-to-right, that is the *same*
serialisation as batch recipes and (later) config:

`photon [/switch[:arg]]… files…`

Per-image pipeline switches: `/crop /depth /filter /flip /grayscale /mirror
/negative /palimp /paste /resize /rotate /swapcol /top`. Application switches:
`/about /browse /capture /help /pref /print /save /slide /wallpaper …`.
Sub-parameters scope to a command, e.g.
`/resize:(W,H[%])`, `/jpegtran --tran --optimize --keeptime`, `/panorama
--direction --widthmethod`, `/save --quality:90 --progressive`.

Example shape: `photon /negative /filter:emboss /grayscale /save:*.gif *.jpg`
— load, chain, convert every match. Environment variables expand.

- [ ] Adopt the switch grammar (Appendix B) once the registry (§1) exists;
      it is the CLI face of the registry, not a second implementation.

## 11. Platform scoping (Linux)

What to drop or replace from the reference's Windows-centric surface:

| Reference feature | Linux approach |
| --- | --- |
| TWAIN scanner acquisition | SANE (via `sane` bindings) if ever wanted; else drop |
| Shell extension / context-menu handler | `.desktop` file + MIME associations + a thumbnailer |
| Set wallpaper | per-DE tools (`gsettings`, `plasma-org.kde…`, `swaybg`) |
| Recycle bin delete | freedesktop trash spec via the `trash` crate |
| Total Commander Lister/Packer plugins | drop |
| Slide-show EXE/SCR export | drop |
| WMF/EMF, registration/donation code | drop |

## 12. Suggested build order

Mapped onto the existing arcs (`Todo.md`: shader milestones M1–M5, and the
near-term list).

1. **Command registry + params serialisation** (§1). Do this before batch,
   rename, or CLI — they all depend on it.
2. **Near-term viewing polish** (§3): fit modes, zoom set, Exif auto-rotate,
   status-bar pixel readout, tiled/tile-repeat view. Small, high-visibility.
3. **Batch conversion** (§7) on the registry — already queued.
4. **Editing core** (§4): resize + resample filters, convolution filters,
   colour depth, undo/command history — the first real editor features.
5. **Metadata** (§8) with `kamadak-exif`; ICC when the shader pass is ready.
6. **Browser** (§6) with the persistent thumbnail cache.
7. **Remaining tool windows** (§7) — they reuse the registry and, for batch
   rename, the CLI grammar.
8. **CLI + scripting** (§10), which by then is mostly plumbing over §1.
9. **Broad codec expansion** (§5) — JXL, SVG, archives, lossless JPEG, RAW.

The shader-pass arc (`M3` BCn, `M4` HDR/tonemap, `M5` cubemap view) proceeds
in parallel and is orthogonal to the above; it just needs to be *kept* rather
than bolted on.

---

## Appendix A — algorithm & pixel-behaviour reference

Verified from the reference; standard image-processing definitions.

**Grayscale luma (integer):** `gray = (77·R + 150·G + 29·B) >> 8`
(weights sum to 256).

**Convolution filters** (one generic routine; each supplies kernel, divisor,
bias, amount):

| Filter | Kernel (3×3 unless noted) | Divisor / bias |
| --- | --- | --- |
| Blur | all 1s | ÷9 |
| Soften | 1 1 1 / 1 **8** 1 / 1 1 1 | ÷16 |
| Sharpen | −1 −1 −1 / −1 **15** −1 / −1 −1 −1 | ÷7 |
| Gaussian 3×3 | 1 2 1 / 2 4 2 / 1 2 1 | ÷16 |
| Gaussian 5×5 | 0 1 2 1 0 / 1 3 4 3 1 / 2 4 8 4 2 / 1 3 4 3 1 / 0 1 2 1 0 | ÷52 |
| Emboss | 0 0 −1 / 0 0 0 / 1 0 0 | bias +127 |
| Edge | −1 −1 −1 / −1 **8** −1 / −1 −1 −1 | bias per record (value not confirmed) |
| Tint | not a kernel filter | *unconfirmed* |

**Resample filters** (kernel function + support radius):

| Filter | Definition | Support |
| --- | --- | --- |
| Box | box | 1.0 |
| Bilinear | triangle | 1.0 |
| Bicubic | Mitchell–Netravali, B = C = ⅓ | 2.0 |
| B-Spline | cubic B-spline | 2.0 |
| Catmull-Rom | Catmull-Rom | 2.0 |
| Lanczos3 | sinc, a = 3 | 3.0 |
| Blackman | windowed, constants 0.42 / 0.5 / 0.08 | — |

**Colour depth:** 1, 4, 8, 16 (5:5:5 and 5:6:5), 24, 32-bit; optional
dithering on reduction.

**Retry/overwrite vocabulary** used by batch tools: *Success / Skipped /
Aborted / Failure*, with overwrite policy *Yes / Yes-to-all / No / Cancel*.

## Appendix B — parameter text grammar

Text forms decoded from the reference's parsers (the serialisation shared by
CLI, batch import/export and config):

- `/depth:(bits,dither)` — dither defaults to **on** when omitted.
- `/resize:(W,H)` — trailing `%` flips the percent flag; filter defaults to the
  configured resample filter.
- `/rotate:Left | Right | 180 | <degrees>` — the three words are the distinct
  Rotate Left/Right/180 commands; a number is Rotate-Free with an angle and a
  background-colour parameter.
- `/swapcol:BGR | RBG | GRB | BRG | GBR`.
- `/wallpaper:Centered | Tiled | Stretched`.
- `/jpegtran --tran --extra --keeptime --optimize --trim`; transform values
  `none, flip, mirror, 90, 180, 270`; marker policy `keepall, keepcomment,
  removeall`.
- `/panorama --direction --widthmethod/--customwidth
  --heightmethod/--customheight` with values `fittolargest / fittosmallest /
  fittofirstimage / custom`.
- `/save --quality:N --progressive --compression:lzw --showprogress`;
  `/p` aliases `/print`; `/?` aliases `/help`.
- Flag semantics: chainable per-image op vs application-level action; sub-param
  taking; save-related.

## Open questions (resolve by using the reference, not by guessing)

- Default keyboard and mouse bindings (read from a running copy's settings).
- Exact `Edge` bias and the `Tint` algorithm.
- Format-detection order; thumbnail-cache file format.
- How Fit-image and Fit-window interact at the edges.
- Dithering algorithm; free-rotation interpolation; tiled-view rendering;
  slide-show transition drawing.
- Exact parameter-blob layout for the commands not covered by Appendix B.
