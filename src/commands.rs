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
    Copy,
    Paste,
    OpenBatchConvert,
    ImageInformation,
    CycleMouseMode,
    NextImage,
    PrevImage,
    ToggleFullscreen,
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

/// Top-level menu a [`Command`] appears under. The menu bar iterates
/// [`Command::all`] and dispatches by group, so a new command joins a menu just
/// by returning its group here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuGroup {
    File,
    Edit,
    View,
}

impl Command {
    /// The menu bar group this command belongs to.
    pub(crate) fn menu_group(&self) -> MenuGroup {
        match self {
            Command::Open
            | Command::SaveImageAs
            | Command::Copy
            | Command::Paste
            | Command::OpenBatchConvert
            | Command::ImageInformation => MenuGroup::File,
            Command::RotateLeft
            | Command::RotateRight
            | Command::Rotate180
            | Command::FlipHorizontal
            | Command::FlipVertical
            | Command::CropToSelection
            | Command::Grayscale
            | Command::Negative
            | Command::UndoEdit
            | Command::RedoEdit => MenuGroup::Edit,
            Command::CycleMouseMode
            | Command::NextImage
            | Command::PrevImage
            | Command::ToggleFullscreen => MenuGroup::View,
        }
    }
}

impl Command {
    /// All commands, in presentation order. The binding table, and any future
    /// menu or CLI listing, iterate this rather than hardcoding commands.
    pub(crate) fn all() -> &'static [Command] {
        &[
            Command::Open,
            Command::SaveImageAs,
            Command::Copy,
            Command::Paste,
            Command::OpenBatchConvert,
            Command::ImageInformation,
            Command::CycleMouseMode,
            Command::NextImage,
            Command::PrevImage,
            Command::ToggleFullscreen,
            Command::RotateLeft,
            Command::RotateRight,
            Command::Rotate180,
            Command::FlipHorizontal,
            Command::FlipVertical,
            Command::CropToSelection,
            Command::Grayscale,
            Command::Negative,
            Command::UndoEdit,
            Command::RedoEdit,
        ]
    }

    /// Human-readable name (menus, tooltips).
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Command::Open => "Open…",
            Command::SaveImageAs => "Save As…",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::OpenBatchConvert => "Convert…",
            Command::ImageInformation => "Image Information…",
            Command::CycleMouseMode => "Cycle Mouse Mode",
            Command::NextImage => "Next Image",
            Command::PrevImage => "Previous Image",
            Command::ToggleFullscreen => "Toggle Fullscreen",
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

    /// The switch that names this command in the scripting language
    /// (Roadmap §10). Stable, lowercase, hyphenated — the seed of the CLI
    /// grammar, so these must not churn. Reserved for the CLI/rebinding work;
    /// the tests here cover its invariants until that lands.
    #[allow(dead_code)]
    pub(crate) fn cli_name(&self) -> &'static str {
        match self {
            Command::Open => "open",
            Command::SaveImageAs => "save-as",
            Command::Copy => "copy",
            Command::Paste => "paste",
            Command::OpenBatchConvert => "convert",
            Command::ImageInformation => "info",
            Command::CycleMouseMode => "mouse-mode",
            Command::NextImage => "next",
            Command::PrevImage => "prev",
            Command::ToggleFullscreen => "fullscreen",
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

    static BINDINGS: [(Command, Option<KeyboardShortcut>); 23] = [
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
        // Menu-only edits carry a `None` row so the "every command has a row"
        // invariant holds without binding a key.
        (Command::OpenBatchConvert, None),
        (
            Command::ImageInformation,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::I)),
        ),
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
            Command::RotateLeft => self.apply_edit(ImageOp::RotateLeft),
            Command::RotateRight => self.apply_edit(ImageOp::RotateRight),
            Command::Rotate180 => self.apply_edit(ImageOp::Rotate180),
            Command::FlipHorizontal => self.apply_edit(ImageOp::FlipHorizontal),
            Command::FlipVertical => self.apply_edit(ImageOp::FlipVertical),
            Command::CropToSelection => {
                if let Some(op) = self.crop_op_from_selection() {
                    self.apply_edit(op);
                }
            }
            Command::Grayscale => self.apply_edit(ImageOp::Grayscale),
            Command::Negative => self.apply_edit(ImageOp::Negative),
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
}
