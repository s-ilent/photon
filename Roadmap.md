# Roadmap — Photon

Forward-looking feature map. `Todo.md` is the near-term, moving worklist; this
file is the long horizon — *our* scope, grouped by capability, with the
concrete behaviours and algorithms worth copying and an explicit list of what
we are **not** building.

**Provenance.** Distilled from a study of a mature, long-running reference
image viewer/toolkit used as a feature benchmark. Clean-room: we copy what the
tool does and how it is wired, never its code, artwork, or translated strings.

**Scope was fixed by a scoping review on 2026-10-11.** The decisions below are
the product's ceiling — a feature that is out of scope stays out unless the
scope itself is revisited. Checkboxes: `[x]` shipped, `[ ]` planned.

---

## 0. What Photon is — and is not

**Photon is:** a fast, zero-latency viewer for huge folders; a texture/HDR
inspector (DDS mips/arrays/faces, EXR); a full thumbnail browser; and a set of
batch tools. It reads far more than it writes.

**Photon is not:** a general image editor, a media/presentation suite, or a
shell-integration layer for the OS it happens to run on.

| Area | Decision |
| --- | --- |
| Core | Viewer + inspector + full browser + batch tools |
| Interactive editing | **Light only** — rotate, flip, crop, grayscale, negative |
| Batch processing | **Rich** — format conversion, resize, filters, and the light edits as chain steps |
| Formats/codecs | **Broad** — all four tiers below, plus legacy raster |
| Browser | **Full** — views, sorting, filters, tree, preview pane, disk-cached thumbnails |
| Metadata | Exif panel, ICC, Image Info. *Not* IPTC/GPS panels |
| Scripting | Chainable CLI **and** per-command/key/mouse rebinding |
| Desktop | `.desktop` file, MIME associations, thumbnailer. *Not* wallpaper setter |
| Presentation | Fullscreen + manual paging only. *No* slide-show tool |
| Platform vestiges | Dropped — see §12 |

---

## 1. The architectural spine: a command registry

The most valuable idea in the reference is a **central command registry**: every
user-visible operation is one entry.

| Field | Meaning |
| --- | --- |
| command id | stable integer (menus, config, batch files) |
| handler | one function that does *everything* for that command |
| enable predicate | when the command is available (image loaded, animation, selection…) |
| name | human label |
| cli name | the switch that names it in the scripting language |

Menus, shortcuts, mouse modes, the CLI and the batch list all dispatch through
this table — which is why one rebinding system can reach every feature and why
CLI and batch recipes are the same thing.

**Handler contract.** `handler(target, command_id, params, flags)`: in
*interactive* mode it opens the command's options dialog, collects params, then
dispatches execute-with-these-params; in *replay* mode (silent flag, params
pre-filled) it applies directly. One handler, two modes, no duplicated code.

This is *the* prerequisite: key-rebinding (§10), the CLI (§10) and batch (§5)
are all just this registry driven from different sources. Build it first.

- [ ] `Command { id, name, cli_name, enable: fn(&AppState) -> bool, run }` + registry table.
- [ ] Route menu, shortcuts, CLI and batch through the registry.
- [ ] Per-command params as a serialisable struct with `to_cli()` / `from_cli()` —
      the one text form shared by config, batch recipes and the CLI.

## 2. Image model: frames over layers, and undo

Photon already models **layers** (DDS mip/array/cubemap). Add the orthogonal
**frame/page** axis (animated GIF/WebP, multipage TIFF, ICO/DCX). Edits walk the
whole frame list — duplicate, process, swap in, push one undo entry — so they
are frame-aware and undoable by construction.

- [ ] Frame/page list per image: frame submenu (play/pause, next/prev, extract
      current/all) and a page navigator strip.
- [ ] **Undo/redo stack** for edits as whole-frame-list swaps (needed at minimum
      for crop and rotate). *Command-history dialog is out of scope.*
- [ ] Pixel-format normalisation step on load so every operation sees a known
      format regardless of source depth/palette.

## 3. Viewing (all extras in scope)

Already in Photon: zoom-at-cursor, pan, next/prev, fullscreen, mouse-mode
cycling, DDS layer/face selection, shader sharp scaling, chunked oversized
images.

- [ ] **Fit modes**: don't-fit / reduce-only-if-larger / always-fit, plus a
      separate *fit bigger image to window* toggle; **fit window per axis**
      (don't / enlarge-if-smaller / always-resize) and *centre window*.
- [ ] **Exif auto-rotate on load** — orient *before* fit/scrollbar math so the
      window sizes correctly; also in thumbnails.
- [ ] **Zoom set**: 100 %, **zoom-to-desktop**, **lock zoom** across navigation.
- [ ] **Tile-repeat ("seamless texture") view** — repeat the texture in a grid.
      *Distinct from Photon's chunked viewing* (which splits one oversized image
      at the GPU limit); this multiplies a small one. A tile-grid uniform.
- [ ] **Status-bar pixel readout** — RGB / alpha / HTML hex (click to copy).
- [ ] **Alpha mask view** + transparency backdrop modes (checkerboard / solid /
      pattern).
- [ ] **Display-while-loading** low-res preview, with no resize on arrival.
- [ ] **Antialiasing toggle** (once the shader pass owns sampling).

## 4. Interactive editing (light)

The registry (§1) and frame model (§2) come first. This is the *entire*
interactive edit set — nothing else gets a menu item.

- [ ] **Rotate**: left / right / 180, plus **free rotate** with a background
      colour for the exposed corners.
- [ ] **Flip** horizontal / vertical.
- [ ] **Crop** — undoable, transparency-aware, multi-frame.
- [ ] **Grayscale** (integer luma) + **negative**.

**Note on the asymmetry.** Resize and the convolution filters are in the
registry (§1) because the batch chain needs them (§5), but they are *not*
surfaced interactively. If that feels wrong later, exposing them is a small
change — flagging it rather than deciding silently. **Colour depth/palette
conversion and dithering are out of scope**: format codecs handle their own
depth internally. **Filters are compute-shader work** — see §5b.

## 5. Batch tools

- [ ] **Batch conversion**: output folder (auto-created), target format + its
      *save options*, per-file **status** rows, overwrite policy
      *ask / yes-to-all / skip*, continue-on-error, live progress, UI pumped
      between files. `rayon` `par_iter` underneath.
- [ ] **Batch operation chain** — the operations the chain can apply before
      saving:
      - format conversion + per-format save options (the core);
      - **resize** (pixels / percent) with the resample-filter set (Appendix A);
      - **rotate / flip / grayscale / negative**;
      - **convolution filters** — blur, soften, sharpen, Gaussian 3×3/5×5,
        emboss, edge (Appendix A). **Compute shader** (§5b), not a CPU loop.
- [ ] **Recipe import/export** as CLI text — falls out of §1/§10.
- [ ] **Batch rename**: mask with insertable tokens — file name/ext/size/date/
      time, image width/height/bpp, and `<Counter.start,step,digits>`
      (zero-padded); search-and-replace; live preview; reorderable list (order
      matters for counters).

## 5b. Image filters are compute-shader work (decision)

**Decision (2026-10-11): image filters are implemented as `wgpu` compute
shaders, not CPU loops.** A compute pass reads the input texture and writes an
output texture — markedly faster than the CPU path for convolution on real
images, and it reuses the device/queue and GPU-resident textures the render
pass already owns. The kernels in Appendix A are the specs for these shaders.

Consequence to plan for: the batch engine (`app/batch.rs`) is deliberately
CPU-only today, so the **filter steps of the batch chain** need a GPU path.
Share one filter module between the batch view and the interactive preview
rather than reimplementing the kernels on the CPU. Non-filter chain steps
(format conversion, resize, rotate/flip, grayscale/negative) stay CPU as they
are. This also applies to any future effect filters (e.g. an interactive
filter preview) — do not add a CPU convolution routine.

## 6. Panorama creator

- [ ] Horizontal / vertical stitch by **concatenation** (no feature matching:
      images are scaled and placed), with width/height methods *fit-to-largest /
      smallest / first / custom*.

## 7. Browser (full)

A real thumbnail/files browser — treat as its own arc.

- [ ] Views: thumbnails, icons, small icons, list, details; sortable,
      drag-rearrangeable columns.
- [ ] Sort by name / extension / **dimensions** / size / date, descending toggle.
- [ ] Filters: supported-only, show folders, show hidden files.
- [ ] **Persistent thumbnail cache** — on/off, max size, custom directory
      (survives sessions); Exif-aware thumbnail rotation.
- [ ] Folder tree, folder history bar, preview pane (with splitter).
- [ ] Archive icons; browse *into* archives (see §8).
- [ ] File ops: new folder, copy, move, delete, rename, properties;
      drag-and-drop everywhere. **Delete is permanent with a confirmation**
      (trash-spec integration is out of scope, §12).

## 8. Codecs & formats (broad)

Photon ships the `image` crate formats, DDS (`image_dds`) and EXR. Add:

- [ ] **Quick wins** (mostly `image` crate already): QOI, Radiance HDR, TGA,
      PCX, ICO, PBM/PGM/PPM.
- [ ] **JXL** decode (`jxl-oxide`); **SVG/SVGZ** raster (`resvg`).
- [ ] **Lossless JPEG transform** — rotate/flip/90/180/270 without re-encode,
      marker policy keep-all / keep-comment / strip (`turbojpeg` transforms);
      also exposed as a shortcut-level *lossless rotate*.
- [ ] **Archive browsing** — images inside CBZ/zip (`zip`), CBR/rar (`unrar`),
      CB7/7z (`sevenz-rust`); treat an archive as a folder to page through.
- [ ] **Heavy formats** (larger deps): HEIF/HEIC (`libheif-rs`), AVIF, JPEG2000
      (`jpeg2k`), RAW (`rawler`/`rawloader`).
- [ ] **Legacy raster kept**: TGA, SGI, PIC/CEL, PDN, PSP, CDR, MBM, WPG, LBM,
      MAC, SPR, ANI, XCF, XPM… (carry what the `image`/dedicated crates give us;
      no bespoke work).
- [x] **Dropped**: WMF/EMF vector metafiles (Windows-only).

Cross-cutting:

- [ ] **Decode-error enum** — the fixed vocabulary (no error, unsupported type,
      unknown, out of memory, colour not supported, read error, write error,
      invalid data, file not found, aborted, grayscale only, other components
      required, size not supported, can't get information). Replaces the brittle
      `"Failed to decode"` string-prefix classification; pair with the
      `thiserror` reintroduction in `Todo.md`.
- [ ] **Content-based format detection** — sniff bytes, honour them when the
      extension lies; offer to rename the incorrect extension.

## 9. Metadata & colour

- [ ] **Exif panel** (~90 tags: camera, exposure, flash, WB, metering) and
      **preserve Exif/IPTC/XMP on JPEG save**.
- [ ] **Image Information** — dimensions, colour depth/palette, frames, pages,
      compression, transparency method, file size/date, DPI (+ original size
      from DPI), progressive/interlaced, ICC profile button.
- [ ] **ICC colour management** — enable, input profile for untagged images,
      output profile (display / sRGB / custom), rendering intent (perceptual,
      relative colorimetric, saturation, absolute); applied in the shader pass.
- **Out of scope:** IPTC and GPS *editing panels* and GPS→map URLs. (IPTC/XMP
  are still *preserved* on save, per the line above.)

## 10. Config, rebinding & scripting

Photon persists `config.ron` (serde + RON). Extend it, and build both faces of
the registry on top.

- [ ] **Per-command rebinding**: shortcut table and mouse-mode mapping (each
      button, wheel up/down, double-click → a mode). This is the
      `Key configuration` item in `Todo.md`.
- [ ] **Chainable CLI**: `photon [/switch[:arg]]… files…`, executed
      left-to-right, the same serialisation as batch recipes (Appendix B). Per-
      image pipeline switches (`/crop /filter /flip /grayscale /negative /resize
      /rotate /swapcol …`) and application switches (`/help /pref /save /browse
      …`). Environment variables expand. **`/slide`, `/wallpaper`, `/print` are
      out.**
- [ ] Config key inventory to grow: **View** (fit modes, AA, display-while-
      loading, centre, zoom, page navigator); **Scrolling** (smooth scroll,
      step); **Misc** (use-undo, ignore-errors, ask-to-overwrite,
      ask-to-save-at-exit, loop-dir + notify, single-instance,
      save/restore-position, rename-incorrect-extension); **Full screen**
      (hide cursor/scrollbars, show filename); **Toolbar** (button size, wrap,
      flat style).

## 11. Desktop integration

- [ ] A **`.desktop` file**, **MIME associations**, and a **thumbnailer**, so
      Photon slots into the file manager and "Open with".
- **Out of scope:** "set as desktop wallpaper" (per-DE tools) and freedesktop
  trash-spec integration.

## 12. Out of scope (decided — do not re-add without revisiting scope)

Vestiges of the reference's Windows-era surface, plus a few features that don't
fit a read-mostly viewer:

- **TWAIN / scanner acquisition** — ancient Windows API; SANE not wanted either.
- **Shell extension / context-menu handler** — replaced by §11.
- **Total Commander Lister/Packer plugins** — drop.
- **Slide-show EXE/SCR export; registration/donation code** — drop.
- **WMF/EMF** metafiles — Windows-only.
- **Slide-show tool** (file list, transitions, auto-advance, randomise) — full
  screen stays manual paging.
- **Screen-capture tool**; **animation editor**; **multipage-image creator**;
  **print**.
- **IPTC / GPS editing panels**; GPS→map URL.
- **Colour-depth / palette conversion, palette edit/import/export, dithering.**
- **Set-as-wallpaper**; **move-to-trash**.
- **Command-history dialog.**

## 13. Suggested build order

Mapped onto the existing arcs (`Todo.md`: shader milestones M1–M5).

1. **Command registry + params serialisation** (§1) — prerequisite for §5, §10.
2. **Near-term viewing polish** (§3): fit modes, zoom set, Exif auto-rotate,
   pixel readout, tile-repeat view. Small, high-visibility.
3. **Batch conversion + chain**, and **batch rename** (§5). The filter steps of
   the chain wait on the **compute-shader filter pass** (§5b).
4. **Editing core** (§4): rotate / flip / crop, grayscale / negative, undo.
5. **Metadata** (§9): Exif panel + Image Info; ICC when the shader pass is ready.
6. **Browser** (§7) with the persistent thumbnail cache; then **desktop
   integration** (§11).
7. **Panorama** (§6).
8. **CLI + rebinding** (§10) — near-free once §1 exists.
9. **Broad codec expansion** (§8) — JXL, SVG, archives, lossless JPEG, RAW.

The shader arc (`M3` BCn, `M4` HDR/tonemap, `M5` cubemap view, plus the
**compute-shader filter pass** of §5b) runs in parallel and is orthogonal; it
just has to be kept, not bolted on.

---

## Appendix A — algorithm & pixel-behaviour reference

Standard definitions; verified from the reference.

**Grayscale luma (integer):** `gray = (77·R + 150·G + 29·B) >> 8`
(weights sum to 256).

**Convolution filters** (one generic routine; each supplies kernel, divisor,
bias, amount). In scope for the batch chain (§5):

| Filter | Kernel (3×3 unless noted) | Divisor / bias |
| --- | --- | --- |
| Blur | all 1s | ÷9 |
| Soften | 1 1 1 / 1 **8** 1 / 1 1 1 | ÷16 |
| Sharpen | −1 −1 −1 / −1 **15** −1 / −1 −1 −1 | ÷7 |
| Gaussian 3×3 | 1 2 1 / 2 4 2 / 1 2 1 | ÷16 |
| Gaussian 5×5 | 0 1 2 1 0 / 1 3 4 3 1 / 2 4 8 4 2 / 1 3 4 3 1 / 0 1 2 1 0 | ÷52 |
| Emboss | 0 0 −1 / 0 0 0 / 1 0 0 | bias +127 |
| Edge | −1 −1 −1 / −1 **8** −1 / −1 −1 −1 | bias per record (unconfirmed) |
| Tint | not a kernel filter | *unconfirmed* — likely out |

**Resample filters** (kernel + support radius). In scope for batch resize (§5):

| Filter | Definition | Support |
| --- | --- | --- |
| Box | box | 1.0 |
| Bilinear | triangle | 1.0 |
| Bicubic | Mitchell–Netravali, B = C = ⅓ | 2.0 |
| B-Spline | cubic B-spline | 2.0 |
| Catmull-Rom | Catmull-Rom | 2.0 |
| Lanczos3 | sinc, a = 3 | 3.0 |
| Blackman | windowed, 0.42 / 0.5 / 0.08 | — |

**Batch status vocabulary:** *Success / Skipped / Aborted / Failure*, with
overwrite policy *Yes / Yes-to-all / No / Cancel*.

## Appendix B — parameter text grammar (in-scope switches)

Text forms decoded from the reference's parsers — the serialisation shared by
CLI, batch import/export and config.

- `/depth:(bits,dither)` — dither defaults on. *(Out of scope for ops, but keep
  the grammar if a format option needs it.)*
- `/resize:(W,H)` — trailing `%` flips the percent flag; filter defaults to the
  configured resample filter.
- `/rotate:Left | Right | 180 | <degrees>` — the three words are the distinct
  Rotate Left/Right/180 commands; a number is Rotate-Free with an angle and a
  background-colour parameter.
- `/swapcol:BGR | RBG | GRB | BRG | GBR`.
- `/filter:<name>` — the convolution filters above.
- `/crop:(L,T,R,B)`; `/flip`; `/mirror`; `/grayscale`; `/negative`.
- `/jpegtran --tran --extra --keeptime --optimize --trim`; transform values
  `none, flip, mirror, 90, 180, 270`; marker policy `keepall, keepcomment,
  removeall`.
- `/panorama --direction --widthmethod/--customwidth
  --heightmethod/--customheight` with values `fittolargest / fittosmallest /
  fittofirstimage / custom`.
- `/save --quality:N --progressive --compression:lzw --showprogress`;
  `/?` aliases `/help`.

## Open questions (resolve by using the reference, not by guessing)

- Default keyboard / mouse bindings (read from a running copy's settings).
- Exact `Edge` bias; whether `Tint` is wanted at all.
- Format-detection order; thumbnail-cache file format.
- How Fit-image and Fit-window interact at the edges.
- Exact parameter-blob layout for commands not covered by Appendix B.
