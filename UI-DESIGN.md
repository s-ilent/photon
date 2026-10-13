# UI-DESIGN — Photon

The interface design authority. Every UI change — menu, panel, dialog, widget —
follows this document. The command registry (`src/commands.rs`) is the
behavioural spine; this file is the presentation layer above it: where things
appear, what they are called, and how they behave. Rules bind new work
immediately; gaps between this document and the shipped UI are tracked in
`Todo.md`, not silently tolerated.

Guiding principle: a viewer gets out of the way. The canvas is the product;
chrome exists to navigate, inform, and edit — minimally.

## 1. Layout architecture

The classic desktop three-region model:

| Region | Widget | Contents |
| --- | --- | --- |
| Top | `MenuBar` + `TopBottomPanel::top` | Menu bar, toolbar buttons |
| Left | `SidePanel::left` | File list — navigation only |
| Centre | `CentralPanel` | The image canvas — the workspace |
| Bottom | `TopBottomPanel::bottom` | Info bar: index, filename, dimensions, zoom, colour space, pixel readout, status messages |

Rules:

- Never mix navigation and property editing in one pane.
- The canvas stays uncluttered; overlays carry transient state only (loading,
  error), one at a time.
- Panels are toggled, not rebuilt; toggling preserves their state.
- Layout state (panel sizes, open windows) persists across restarts so the app
  looks like the last session. **Not yet wired** — `Todo.md`.

## 2. Menus

The menu bar is generated from the registry: `MenuGroup` places a command,
`Command::all()` orders it, `starts_menu_section()` separates it. A new command
joins a menu by declaring its group — never hand-append a registry-capable
action to a menu. (The runtime-only View tail — Lock Zoom, Fit Window to Image,
Centre Window, Zoom to Desktop — is the one fixed exception.)

Placement rule for a new command — ask what it touches:

| Group | Question | Today's contents |
| --- | --- | --- |
| File | Files, or opens a tool window? | Open…, Save As…, Save Selection As…, Convert…, Image Information…, Settings… |
| Edit | The edit history, or the clipboard? | Undo Edit, Redo Edit, Copy, Paste |
| Image | The current image's pixels? | Rotate ×3, Flip ×2, Crop to Selection, Grayscale, Negative |
| View | The viewport or app state? | Next/Previous, Fit Image, Actual Size, Cycle Mouse Mode, Toggle Sidebar, Toggle Fullscreen (+ zoom/window actions) |

- Sections hold 2–5 related items, separated by the section flag. No more
  separators than content.
- Every command has an enable state. Disable, never hide (`add_enabled`) —
  vanishing items shift the menu.
- Microcopy:
  - Menu items, buttons, window titles: Title Case.
  - Checkbox/radio labels, body text: sentence case.
  - Ellipsis (`…`) only when input stands between the click and the effect
    (`Open…`, `Save As…`, `Convert…`); never on immediate actions.
  - Verbs, no jargon, no internal terms ("unsaved changes", not "dirty").

## 3. Controls

- **Checkbox** — independent binary state, never one-of-many.
- **Radio buttons** — mutually exclusive, 2–5 options, all visible at once.
- **Combo box** — mutually exclusive, 6+ options, or tight space.
- **Button** — triggers an immediate action. Never a state toggle; never
  "… Now" phrasing ("Save", not "Save Now").
- **Table** — tabular data lists (the batch file list, the operation steps)
  via `egui_extras::TableBuilder`: header row on, columns resizable.
  `Grid` is the form idiom (§4), not a data table.

## 4. Forms and dialogs

- Forms use `egui::Grid`: two columns, labels right-aligned with colon, fields
  left-aligned, one spacing vocabulary. No freehand `horizontal` rows. Flex
  lines cannot align a label column across rows, so a form built on a layout
  engine is a worse Grid — engines do not take forms over.
- Layout mechanism by case, one idiom per window: native `Layout` first;
  `egui_flex` is the sanctioned engine for flow layout (wrapping or
  width-pressured rows) when native stops coping (Todo.md). The remaining
  rules in this section are engine-independent — swapping the mechanism never
  rewrites them.
- Spacing comes from `Style`/`Spacing`, not per-call pixel values; independent
  blocks get one `add_space`, related rows get none.
- Tool windows (Preferences, Batch Convert) are **native OS child windows** via
  `show_viewport_immediate` viewports — the WM owns their title bar, resizing
  and close button; never an `egui::Window` pane. Confirmations (none yet) stay
  `egui::Window`: non-collapsible, non-resizable,
  anchored centre.
- Alert anatomy: a bold one-line headline question; a smaller consequence
  body; verb buttons (`Discard`/`Cancel`, not `OK`); the destructive action
  visually separated from the safe pair.
- Progressive disclosure: design the default view for the common workflow;
  advanced parameters go behind a `CollapsingHeader`.
- A parameterised effect shows a live preview: original and result side by
  side, updating as parameters move.

## 5. Immediate-mode discipline

- Never block `update()`. Long work runs on background threads with channel
  results (the loader-worker pattern); `request_repaint()` on completion; a
  spinner or determinate progress meanwhile.
- No layout jitter: prefer disabling widgets or reserving space over making
  them appear and disappear.
- Labels are not selectable (`selectable_labels = false` — viewer rule). A
  `TextEdit` re-enables selectability locally, never globally.

## 6. Status and feedback

- Bottom-bar status messages are plain text — no colour; colour is reserved
  for extreme errors.
- One status line at a time; a new message replaces the old.
- Load failures land in the centre overlay, classified unsupported-format vs
  error. No dialog storms.

## 7. Theme

- Dracula palette via `app/theme.rs`; light mode is egui defaults.
- Widgets take colours from the theme/style — never hardcode a colour.
- Font size changes go through Settings, which re-applies the theme.

## 8. Verification

- Pure presentation helpers (geometry, label forms, section flags) get unit
  tests next to the code; egui-context code is not unit-tested.
- User-visible changes ship with a manual click-through brief (exact values +
  the release binary).
