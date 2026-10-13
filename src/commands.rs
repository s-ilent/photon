//! Command registry: the single dispatch spine (Roadmap.md §1).
//!
//! Every user-visible operation is one [`Command`]. Shortcuts are scanned
//! through [`default_bindings`] and dispatched through
//! [`PhotonApp::run_command`]; the config rebinding and chainable CLI of
//! Roadmap §10 will hang off the same enum and table later. This module is
//! therefore the *only* place a key is associated with an action.
//!
//! This first step deliberately stops at routing the existing keyboard
//! shortcuts: no params, no enable predicates, no CLI parser yet.

use crate::app::image_op::ImageOp;
use crate::app::PhotonApp;
use eframe::egui;

/// Every user-visible operation Photon can perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Command {
    Open,
    SaveImageAs,
    SaveSelectionAs,
    Copy,
    Paste,
    OpenBatchConvert,
    ImageInformation,
    ToggleSettings,
    CycleMouseMode,
    NextImage,
    PrevImage,
    ToggleFullscreen,
    ToggleSidebar,
    FitImage,
    ZoomActualSize,
    // Interactive edits (Roadmap.md §4).
    RotateLeft,
    RotateRight,
    Rotate180,
    FlipHorizontal,
    FlipVertical,
    CropToSelection,
    Grayscale,
    Negative,
    UndoEdit,
    RedoEdit,
}

/// Top-level menu a [`Command`] appears under. The menu bar renders the groups
/// in [`MenuGroup::ALL`] order; within a group, [`Command::all`] is
/// presentation order and [`Command::starts_menu_section`] places the
/// separators. A new command joins a menu just by returning its group here
/// (placement rules: UI-DESIGN.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MenuGroup {
    File,
    Edit,
    Image,
    View,
}

impl MenuGroup {
    /// Every group, in menu-bar order. Variant order is presentation order.
    pub(crate) const ALL: [MenuGroup; 4] = [
        MenuGroup::File,
        MenuGroup::Edit,
        MenuGroup::Image,
        MenuGroup::View,
    ];

    /// The menu-bar title for this group.
    pub(crate) fn title(self) -> &'static str {
        match self {
            MenuGroup::File => "File",
            MenuGroup::Edit => "Edit",
            MenuGroup::Image => "Image",
            MenuGroup::View => "View",
        }
    }
}

impl Command {
    /// The menu bar group this command belongs to. Taxonomy: files and tool
    /// windows are File, edit history and clipboard are Edit, changes to the
    /// current image's pixels are Image, viewport/app state is View.
    pub(crate) fn menu_group(&self) -> MenuGroup {
        match self {
            Command::Open
            | Command::SaveImageAs
            | Command::SaveSelectionAs
            | Command::OpenBatchConvert
            | Command::ImageInformation
            | Command::ToggleSettings => MenuGroup::File,
            Command::UndoEdit | Command::RedoEdit | Command::Copy | Command::Paste => {
                MenuGroup::Edit
            }
            Command::RotateLeft
            | Command::RotateRight
            | Command::Rotate180
            | Command::FlipHorizontal
            | Command::FlipVertical
            | Command::CropToSelection
            | Command::Grayscale
            | Command::Negative => MenuGroup::Image,
            Command::CycleMouseMode
            | Command::NextImage
            | Command::PrevImage
            | Command::ToggleFullscreen
            | Command::ToggleSidebar
            | Command::FitImage
            | Command::ZoomActualSize => MenuGroup::View,
        }
    }

    /// Whether a separator is drawn before this command inside its menu (it
    /// opens a new visual section). [`Command::all`] is in presentation order;
    /// the first command of a group must not set this (tested).
    pub(crate) fn starts_menu_section(&self) -> bool {
        matches!(
            self,
            Command::OpenBatchConvert
                | Command::Copy
                | Command::FlipHorizontal
                | Command::Grayscale
                | Command::FitImage
                | Command::CycleMouseMode
        )
    }

    /// The tooltip shortcut hint, e.g. " (Ctrl+O)"; empty when unbound. The
    /// first binding wins for multi-key commands.
    pub(crate) fn shortcut_hint(
        &self,
        ctx: &egui::Context,
        bindings: &[(Command, Option<egui::KeyboardShortcut>)],
    ) -> String {
        bindings
            .iter()
            .find_map(|(c, s)| if c == self { *s } else { None })
            .map(|s| format!(" ({})", ctx.format_shortcut(&s)))
            .unwrap_or_default()
    }
}

impl Command {
    /// All commands, in presentation order: grouped by [`MenuGroup`], sections
    /// separated by [`Command::starts_menu_section`]. The binding table, and
    /// any future menu or CLI listing, iterate this rather than hardcoding
    /// commands.
    pub(crate) fn all() -> &'static [Command] {
        &[
            Command::Open,
            Command::SaveImageAs,
            Command::SaveSelectionAs,
            Command::OpenBatchConvert,
            Command::ImageInformation,
            Command::ToggleSettings,
            Command::UndoEdit,
            Command::RedoEdit,
            Command::Copy,
            Command::Paste,
            Command::RotateLeft,
            Command::RotateRight,
            Command::Rotate180,
            Command::FlipHorizontal,
            Command::FlipVertical,
            Command::CropToSelection,
            Command::Grayscale,
            Command::Negative,
            Command::NextImage,
            Command::PrevImage,
            Command::FitImage,
            Command::ZoomActualSize,
            Command::CycleMouseMode,
            Command::ToggleSidebar,
            Command::ToggleFullscreen,
        ]
    }

    /// Human-readable name (menus, tooltips).
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Command::Open => "Open…",
            Command::SaveImageAs => "Save As…",
            Command::SaveSelectionAs => "Save Selection As…",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::OpenBatchConvert => "Convert…",
            Command::ImageInformation => "Image Information…",
            Command::ToggleSettings => "Settings…",
            Command::CycleMouseMode => "Cycle Mouse Mode",
            Command::NextImage => "Next Image",
            Command::PrevImage => "Previous Image",
            Command::ToggleFullscreen => "Toggle Fullscreen",
            Command::ToggleSidebar => "Toggle Sidebar",
            Command::FitImage => "Fit Image",
            Command::ZoomActualSize => "Actual Size",
            Command::RotateLeft => "Rotate Left",
            Command::RotateRight => "Rotate Right",
            Command::Rotate180 => "Rotate 180°",
            Command::FlipHorizontal => "Flip Horizontal",
            Command::FlipVertical => "Flip Vertical",
            Command::CropToSelection => "Crop to Selection",
            Command::Grayscale => "Grayscale",
            Command::Negative => "Negative",
            Command::UndoEdit => "Undo Edit",
            Command::RedoEdit => "Redo Edit",
        }
    }

    /// The name that identifies this command in config shortcut overrides and
    /// the scripting surface (Roadmap §10). Stable, lowercase, hyphenated —
    /// must not churn.
    pub(crate) fn cli_name(&self) -> &'static str {
        match self {
            Command::Open => "open",
            Command::SaveImageAs => "save-as",
            Command::SaveSelectionAs => "save-selection",
            Command::Copy => "copy",
            Command::Paste => "paste",
            Command::OpenBatchConvert => "convert",
            Command::ImageInformation => "info",
            Command::ToggleSettings => "settings",
            Command::CycleMouseMode => "mouse-mode",
            Command::NextImage => "next",
            Command::PrevImage => "prev",
            Command::ToggleFullscreen => "fullscreen",
            Command::ToggleSidebar => "sidebar",
            Command::FitImage => "fit",
            Command::ZoomActualSize => "actual-size",
            Command::RotateLeft => "rotate-left",
            Command::RotateRight => "rotate-right",
            Command::Rotate180 => "rotate-180",
            Command::FlipHorizontal => "flip-horizontal",
            Command::FlipVertical => "flip-vertical",
            Command::CropToSelection => "crop",
            Command::Grayscale => "grayscale",
            Command::Negative => "negative",
            Command::UndoEdit => "undo",
            Command::RedoEdit => "redo",
        }
    }

    /// The command a [`Self::cli_name`] names, if any.
    pub(crate) fn from_cli_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|c| c.cli_name() == name)
    }
}

/// The default key binding table.
///
/// One row per (command, key) pair. A command bound to two keys by default
/// (Tab/Backslash, Space/→, Backspace/←) appears once per key; its rows are
/// adjacent and carry the same [`Command`]. Copy/Paste appear with their
/// Ctrl/Cmd+C/V key as the raw-key fallback — on winit egui rewrites that key
/// into a clipboard *command event* before we see it (see the GOTCHA in
/// `input.rs`), which `input.rs` also honours. Every command in [`Command::all`]
/// has at least one row.
pub(crate) fn default_bindings() -> &'static [(Command, Option<egui::KeyboardShortcut>)] {
    use egui::{Key, KeyboardShortcut, Modifiers};

    static BINDINGS: [(Command, Option<KeyboardShortcut>); 28] = [
        (
            Command::Open,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
        ),
        (
            Command::SaveImageAs,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
        ),
        // Raw-key fallback; the winit path arrives as Event::Copy/Paste.
        (
            Command::Copy,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::C)),
        ),
        (
            Command::Paste,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::V)),
        ),
        (
            Command::CycleMouseMode,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::Backslash)),
        ),
        (
            Command::CycleMouseMode,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::Tab)),
        ),
        (
            Command::NextImage,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::Space)),
        ),
        (
            Command::NextImage,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::ArrowRight)),
        ),
        (
            Command::PrevImage,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::Backspace)),
        ),
        (
            Command::PrevImage,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::ArrowLeft)),
        ),
        (
            Command::ToggleFullscreen,
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::F11)),
        ),
        (
            Command::UndoEdit,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)),
        ),
        (
            Command::RedoEdit,
            Some(KeyboardShortcut::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::Z,
            )),
        ),
        // Menu/toolbar-only commands carry a `None` row so the "every command
        // has a row" invariant holds without binding a key.
        (Command::OpenBatchConvert, None),
        (
            Command::ImageInformation,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::I)),
        ),
        (Command::SaveSelectionAs, None),
        (Command::ToggleSettings, None),
        (Command::ToggleSidebar, None),
        (Command::FitImage, None),
        (Command::ZoomActualSize, None),
        (Command::RotateLeft, None),
        (Command::RotateRight, None),
        (Command::Rotate180, None),
        (Command::FlipHorizontal, None),
        (Command::FlipVertical, None),
        (Command::CropToSelection, None),
        (Command::Grayscale, None),
        (Command::Negative, None),
    ];

    &BINDINGS
}

/// The effective binding table: [`default_bindings`] with the config's
/// per-command overrides applied. An override replaces every default key of
/// its command; unparseable shortcut text is ignored so a corrupt config keeps
/// the defaults (Roadmap §10).
pub(crate) fn effective_bindings(
    overrides: &[(String, Option<String>)],
) -> Vec<(Command, Option<egui::KeyboardShortcut>)> {
    let mut bindings = default_bindings().to_vec();
    for (name, text) in overrides {
        let Some(command) = Command::from_cli_name(name) else {
            continue;
        };
        let shortcut = match text {
            // Explicitly unbound.
            None => None,
            Some(text) => match shortcut_from_text(text) {
                Some(shortcut) => Some(shortcut),
                None => continue,
            },
        };
        let mut applied = false;
        for row in bindings.iter_mut().filter(|(c, _)| *c == command) {
            *row = (command, if applied { None } else { shortcut });
            applied = true;
        }
    }
    bindings
}

/// Apply a binding change: `shortcut == None` clears the command's keys;
/// otherwise the command is rebound to the shortcut and any OTHER command
/// holding that exact key is auto-unbound of it (last wins — the menus show
/// the resulting bindings). A two-key command keeps its non-conflicting key.
pub(crate) fn apply_binding_change(
    bindings: &mut [(Command, Option<egui::KeyboardShortcut>)],
    command: Command,
    shortcut: Option<egui::KeyboardShortcut>,
) {
    for row in bindings.iter_mut().filter(|(c, _)| *c == command) {
        row.1 = None;
    }
    if let Some(shortcut) = shortcut {
        if let Some(row) = bindings.iter_mut().find(|(c, _)| *c == command) {
            row.1 = Some(shortcut);
        }
        for row in bindings.iter_mut().filter(|(c, _)| *c != command) {
            if row.1 == Some(shortcut) {
                row.1 = None;
            }
        }
    }
}

/// The config deltas: rows of `bindings` that differ from [`default_bindings`]
/// as `(cli_name, shortcut text | None)`. One entry per changed command —
/// `None` text when the effective table leaves it without keys; commands
/// matching the defaults are skipped. Order follows `Command::all` so the
/// config output is stable.
pub(crate) fn binding_overrides(
    bindings: &[(Command, Option<egui::KeyboardShortcut>)],
) -> Vec<(String, Option<String>)> {
    Command::all()
        .iter()
        .filter_map(|command| {
            let keys = |table: &[(Command, Option<egui::KeyboardShortcut>)]| {
                table
                    .iter()
                    .filter(|(c, _)| c == command)
                    .filter_map(|(_, s)| *s)
                    .collect::<Vec<_>>()
            };
            if keys(default_bindings()) == keys(bindings) {
                return None;
            }
            let text = keys(bindings).first().map(|s| shortcut_to_text(*s));
            Some((command.cli_name().to_owned(), text))
        })
        .collect()
}

/// The config-file text form of a shortcut, e.g. `"Ctrl+Shift+Z"` —
/// [`shortcut_from_text`] parses this back. Always the non-Mac spelling, so a
/// config is portable across platforms (`Ctrl` means Cmd there).
pub(crate) fn shortcut_to_text(shortcut: egui::KeyboardShortcut) -> String {
    shortcut.format(&egui::ModifierNames::NAMES, false)
}

/// Parse the text form [`shortcut_to_text`] writes, plus egui's Mac modifier
/// names ("Cmd", "Option") for hand-edited configs. `None` when any part is
/// neither a modifier nor a key name.
pub(crate) fn shortcut_from_text(text: &str) -> Option<egui::KeyboardShortcut> {
    use egui::ModifierNames;
    let names = ModifierNames::NAMES;
    let mut modifiers = egui::Modifiers::NONE;
    let mut key = None;
    for part in text.split(names.concat) {
        let part = part.trim();
        // "Ctrl" is the printed form of the platform Command modifier, so it
        // parses back to `command` (which is Ctrl on non-Mac, Cmd on Mac).
        if part == names.ctrl {
            modifiers.command = true;
        } else if part == names.alt || part == names.mac_alt {
            modifiers.alt = true;
        } else if part == names.shift {
            modifiers.shift = true;
        } else if part == names.mac_cmd {
            modifiers.mac_cmd = true;
        } else {
            key = egui::Key::from_name(part);
        }
    }
    Some(egui::KeyboardShortcut {
        modifiers,
        logical_key: key?,
    })
}

impl PhotonApp {
    /// Dispatch one command. This is the single entry point shared by the
    /// keyboard path today and, later, menus, the CLI, batch recipes and
    /// rebinding (Roadmap §1/§10).
    pub(crate) fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Open => self.open_file(),
            Command::SaveImageAs => {
                log::debug!("Save triggered");
                self.save_image_as();
            }
            Command::Copy => {
                log::debug!("Stateful Copy Triggered");
                self.copy_to_clipboard();
            }
            Command::Paste => {
                log::debug!("Stateful Paste Triggered");
                self.paste_from_clipboard(ctx);
            }
            Command::CycleMouseMode => self.cycle_mode(),
            Command::NextImage => self.next_image(),
            Command::PrevImage => self.prev_image(),
            Command::ToggleFullscreen => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            Command::OpenBatchConvert => self.batch.open = true,
            Command::ImageInformation => self.info_open = !self.info_open,
            Command::ToggleSettings => self.settings_open = !self.settings_open,
            Command::ToggleSidebar => {
                self.sidebar_open = !self.sidebar_open;
                if self.sidebar_open {
                    self.scroll_to_target = true;
                }
            }
            Command::FitImage => self.needs_fit = true,
            Command::ZoomActualSize => {
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
            }
            Command::SaveSelectionAs => self.save_selection_as(),
            Command::RotateLeft => self.apply_edit(&[ImageOp::RotateLeft]),
            Command::RotateRight => self.apply_edit(&[ImageOp::RotateRight]),
            Command::Rotate180 => self.apply_edit(&[ImageOp::Rotate180]),
            Command::FlipHorizontal => self.apply_edit(&[ImageOp::FlipHorizontal]),
            Command::FlipVertical => self.apply_edit(&[ImageOp::FlipVertical]),
            Command::CropToSelection => {
                if let Some(op) = self.crop_op_from_selection() {
                    self.apply_edit(&[op]);
                }
            }
            Command::Grayscale => self.apply_edit(&[ImageOp::Grayscale]),
            Command::Negative => self.apply_edit(&[ImageOp::Negative]),
            Command::UndoEdit => self.undo_edit(),
            Command::RedoEdit => self.redo_edit(),
        }
    }

    /// Whether `command` can run right now — the menu-bar enable state. A
    /// missing image (or an error overlay) disables the ops that need pixels;
    /// undo/redo and crop track their own preconditions.
    pub(crate) fn command_enabled(&self, command: Command) -> bool {
        let has_image = self.displayed_image.is_some() && self.error_overlay.is_none();
        match command {
            Command::UndoEdit => self.edit_undo.can_undo(),
            Command::RedoEdit => self.edit_undo.can_redo(),
            Command::CropToSelection => has_image && self.selection_image_rect.is_some(),
            Command::SaveSelectionAs => {
                self.mode == crate::app::MouseMode::Select && self.selection_image_rect.is_some()
            }
            Command::FitImage | Command::ZoomActualSize => has_image,
            Command::ImageInformation => self.target_path.is_some(),
            Command::SaveImageAs
            | Command::RotateLeft
            | Command::RotateRight
            | Command::Rotate180
            | Command::FlipHorizontal
            | Command::FlipVertical
            | Command::Grayscale
            | Command::Negative => has_image,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_command_has_a_binding_row() {
        for command in Command::all() {
            assert!(
                default_bindings().iter().any(|(c, _)| c == command),
                "{command:?} has no row in default_bindings()"
            );
        }
    }

    #[test]
    fn binding_table_names_only_known_commands() {
        for (command, _) in default_bindings() {
            assert!(
                Command::all().contains(command),
                "unknown command {command:?}"
            );
        }
    }

    #[test]
    fn cli_names_are_unique_and_nonempty() {
        let mut seen = HashSet::new();
        for command in Command::all() {
            let name = command.cli_name();
            assert!(!name.is_empty(), "{command:?} has an empty cli_name");
            assert!(seen.insert(name), "duplicate cli_name {name:?}");
        }
    }

    #[test]
    fn copy_and_paste_have_key_fallbacks() {
        // Their primary trigger is egui::Event::Copy / Event::Paste (input.rs);
        // the table carries the raw-key fallback for backends that don't rewrite.
        use egui::{Key, KeyboardShortcut, Modifiers};
        assert!(default_bindings().contains(&(
            Command::Copy,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::C))
        )));
        assert!(default_bindings().contains(&(
            Command::Paste,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::V))
        )));
    }

    #[test]
    fn no_duplicate_key_binding() {
        let mut seen = HashSet::new();
        for (_, shortcut) in default_bindings() {
            if let Some(shortcut) = shortcut {
                assert!(seen.insert(*shortcut), "duplicate key binding {shortcut:?}");
            }
        }
    }

    #[test]
    fn undo_redo_and_crop_disabled_without_preconditions() {
        let app = PhotonApp::new();
        assert!(!app.command_enabled(Command::UndoEdit));
        assert!(!app.command_enabled(Command::RedoEdit));
        assert!(!app.command_enabled(Command::CropToSelection));
    }

    #[test]
    fn pixel_edits_disabled_without_an_image() {
        let app = PhotonApp::new();
        for command in [
            Command::ImageInformation,
            Command::RotateLeft,
            Command::RotateRight,
            Command::Rotate180,
            Command::FlipHorizontal,
            Command::FlipVertical,
            Command::Grayscale,
            Command::Negative,
            Command::SaveImageAs,
            Command::FitImage,
            Command::ZoomActualSize,
        ] {
            assert!(
                !app.command_enabled(command),
                "{command:?} should be disabled"
            );
        }
        // Commands that do not need a loaded image stay enabled.
        assert!(app.command_enabled(Command::Open));
        assert!(app.command_enabled(Command::Paste));
        assert!(app.command_enabled(Command::OpenBatchConvert));
        assert!(app.command_enabled(Command::ToggleSettings));
        assert!(app.command_enabled(Command::ToggleSidebar));
    }

    #[test]
    fn save_selection_needs_a_selection() {
        let app = PhotonApp::new();
        assert!(!app.command_enabled(Command::SaveSelectionAs));
    }

    #[test]
    fn toolbar_window_toggles_flip_their_state() {
        let mut app = PhotonApp::new();
        let ctx = egui::Context::default();
        app.run_command(Command::ToggleSettings, &ctx);
        assert!(app.settings_open);
        let sidebar = app.sidebar_open;
        app.run_command(Command::ToggleSidebar, &ctx);
        assert_eq!(app.sidebar_open, !sidebar);
    }

    #[test]
    fn shortcut_hints_follow_the_binding_table() {
        let ctx = egui::Context::default();
        assert!(Command::Open.shortcut_hint(&ctx, default_bindings()).contains('O'));
        assert!(!Command::SaveImageAs.shortcut_hint(&ctx, default_bindings()).is_empty());
        assert!(Command::OpenBatchConvert.shortcut_hint(&ctx, default_bindings()).is_empty());
        assert!(Command::ToggleSidebar.shortcut_hint(&ctx, default_bindings()).is_empty());
    }

    #[test]
    fn shortcut_hint_follows_an_overridden_table() {
        let ctx = egui::Context::default();
        let bindings = effective_bindings(&[
            ("open".into(), Some("Ctrl+Shift+O".into())),
            ("fullscreen".into(), None),
        ]);
        assert!(Command::Open.shortcut_hint(&ctx, &bindings).contains("Shift"));
        assert!(Command::ToggleFullscreen.shortcut_hint(&ctx, &bindings).is_empty());
    }

    #[test]
    fn rebinding_a_two_key_command_replaces_both_keys() {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let mut bindings = default_bindings().to_vec();
        let rebinding = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
        apply_binding_change(&mut bindings, Command::NextImage, Some(rebinding));
        let next: Vec<_> = bindings
            .iter()
            .filter(|(c, _)| *c == Command::NextImage)
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(next, vec![Some(rebinding), None]);
    }

    #[test]
    fn rebinding_takes_the_key_from_the_previous_holder() {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let mut bindings = default_bindings().to_vec();
        // Space is NextImage's default key.
        let space = KeyboardShortcut::new(Modifiers::NONE, Key::Space);
        apply_binding_change(&mut bindings, Command::PrevImage, Some(space));
        let next: Vec<_> = bindings
            .iter()
            .filter(|(c, _)| *c == Command::NextImage)
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(
            next.iter().filter(|s| s.is_some()).count(),
            1,
            "NextImage keeps its non-conflicting key"
        );
        // PrevImage's own row carries the rebound key.
        assert!(bindings
            .iter()
            .any(|&(c, s)| c == Command::PrevImage && s == Some(space)));
    }

    #[test]
    fn unbinding_clears_every_key() {
        let mut bindings = default_bindings().to_vec();
        apply_binding_change(&mut bindings, Command::CycleMouseMode, None);
        assert!(bindings
            .iter()
            .all(|&(c, s)| c != Command::CycleMouseMode || s.is_none()));
    }

    #[test]
    fn rebinding_a_command_without_keys() {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let mut bindings = default_bindings().to_vec();
        let binding = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
        apply_binding_change(&mut bindings, Command::OpenBatchConvert, Some(binding));
        assert!(bindings.contains(&(Command::OpenBatchConvert, Some(binding))));
    }

    #[test]
    fn binding_overrides_round_trip_through_effective_bindings() {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let mut table = default_bindings().to_vec();
        // A rebind, an unbind, and a two-key command that lost one key.
        apply_binding_change(
            &mut table,
            Command::Open,
            Some(KeyboardShortcut::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::O,
            )),
        );
        apply_binding_change(&mut table, Command::ToggleFullscreen, None);
        apply_binding_change(
            &mut table,
            Command::NextImage,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::N)),
        );

        let overrides = binding_overrides(&table);
        // Command::all() order, not application order.
        assert_eq!(
            overrides,
            vec![
                ("open".into(), Some("Ctrl+Shift+O".into())),
                ("next".into(), Some("Ctrl+N".into())),
                ("fullscreen".into(), None),
            ]
        );
        assert_eq!(effective_bindings(&overrides), table);
    }

    #[test]
    fn binding_overrides_skip_default_commands() {
        assert!(binding_overrides(default_bindings()).is_empty());
    }

    /// Ctrl+I toggles the window: invoking it again closes what it opened.
    #[test]
    fn image_information_command_toggles_the_window() {
        let mut app = PhotonApp::new();
        let ctx = egui::Context::default();
        assert!(!app.info_open);
        app.run_command(Command::ImageInformation, &ctx);
        assert!(app.info_open);
        app.run_command(Command::ImageInformation, &ctx);
        assert!(!app.info_open);
    }

    #[test]
    fn menu_groups_are_contiguous_in_presentation_order() {
        let groups: Vec<MenuGroup> = Command::all().iter().map(|c| c.menu_group()).collect();
        for pair in groups.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "{:?} appears again after {:?} in Command::all()",
                pair[1],
                pair[0]
            );
        }
    }

    #[test]
    fn every_menu_group_has_commands() {
        for group in MenuGroup::ALL {
            assert!(
                Command::all().iter().any(|c| c.menu_group() == group),
                "{group:?} has no commands"
            );
        }
    }

    #[test]
    fn a_section_separator_never_leads_a_menu() {
        let mut prev_group = None;
        for command in Command::all() {
            let group = command.menu_group();
            if prev_group != Some(group) {
                assert!(
                    !command.starts_menu_section(),
                    "{command:?} would open the {group:?} menu with a separator"
                );
            }
            prev_group = Some(group);
        }
    }

    #[test]
    fn shortcut_text_round_trips() {
        use egui::{Key, KeyboardShortcut, Modifiers};
        for shortcut in [
            KeyboardShortcut::new(Modifiers::NONE, Key::Space),
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z),
            KeyboardShortcut::new(Modifiers::NONE, Key::F11),
            KeyboardShortcut::new(Modifiers::NONE, Key::Minus),
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::ArrowRight),
        ] {
            let text = shortcut_to_text(shortcut);
            assert_eq!(
                shortcut_from_text(&text),
                Some(shortcut),
                "round-trip of {text:?}"
            );
        }
    }

    #[test]
    fn shortcut_text_parses_human_forms() {
        let shortcut = shortcut_from_text("Ctrl+Shift+Z").expect("parse Ctrl+Shift+Z");
        assert_eq!(shortcut.logical_key, egui::Key::Z);
        assert!(shortcut.modifiers.command);
        assert!(shortcut.modifiers.shift);
        assert!(shortcut_from_text("gibberish").is_none());
        assert!(shortcut_from_text("Ctrl+NotAKey").is_none());
    }

    #[test]
    fn effective_bindings_apply_overrides() {
        // An override replaces both keys of a two-key command with one.
        let bindings = effective_bindings(&[("next".into(), Some("Ctrl+N".into()))]);
        let next: Vec<_> = bindings
            .iter()
            .filter(|(c, _)| *c == Command::NextImage)
            .collect();
        assert_eq!(next.len(), 2);
        assert_eq!(next.iter().filter(|(_, s)| s.is_some()).count(), 1);

        // An explicit unbind clears every key of the command.
        let bindings = effective_bindings(&[("fullscreen".into(), None)]);
        assert!(
            bindings
                .iter()
                .all(|(c, s)| c != &Command::ToggleFullscreen || s.is_none())
        );

        // Unknown names and unparseable text leave the defaults in place.
        let bindings = effective_bindings(&[
            ("nope".into(), Some("Ctrl+N".into())),
            ("info".into(), Some("nonsense".into())),
        ]);
        assert_eq!(bindings, default_bindings());
    }
}
