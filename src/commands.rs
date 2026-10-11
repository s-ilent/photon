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

use crate::app::PhotonApp;
use eframe::egui;

/// Every user-visible operation Photon can perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Command {
    Open,
    SaveImageAs,
    Copy,
    Paste,
    CycleMouseMode,
    NextImage,
    PrevImage,
    ToggleFullscreen,
}

// The registry's public surface (names/CLI keys) is exercised by the tests
// here and consumed by the menu/CLI layering of Roadmap §1/§10, which lands
// after this first dispatch step. Temporary `allow`, as with `app::view`.
#[allow(dead_code)]
impl Command {
    /// All commands, in presentation order. The binding table, and any future
    /// menu or CLI listing, iterate this rather than hardcoding commands.
    pub(crate) fn all() -> &'static [Command] {
        &[
            Command::Open,
            Command::SaveImageAs,
            Command::Copy,
            Command::Paste,
            Command::CycleMouseMode,
            Command::NextImage,
            Command::PrevImage,
            Command::ToggleFullscreen,
        ]
    }

    /// Human-readable name (menus, tooltips).
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Command::Open => "Open…",
            Command::SaveImageAs => "Save As…",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::CycleMouseMode => "Cycle Mouse Mode",
            Command::NextImage => "Next Image",
            Command::PrevImage => "Previous Image",
            Command::ToggleFullscreen => "Toggle Fullscreen",
        }
    }

    /// The switch that names this command in the scripting language
    /// (Roadmap §10). Stable, lowercase, hyphenated — the seed of the CLI
    /// grammar, so these must not churn.
    pub(crate) fn cli_name(&self) -> &'static str {
        match self {
            Command::Open => "open",
            Command::SaveImageAs => "save-as",
            Command::Copy => "copy",
            Command::Paste => "paste",
            Command::CycleMouseMode => "mouse-mode",
            Command::NextImage => "next",
            Command::PrevImage => "prev",
            Command::ToggleFullscreen => "fullscreen",
        }
    }
}

/// The default key binding table.
///
/// One row per (command, key) pair. A command bound to two keys by default
/// (Tab/Backslash, Space/→, Backspace/←) appears once per key; its rows are
/// adjacent and carry the same [`Command`]. A command whose real trigger is a
/// clipboard *command event* rather than a key (Copy/Paste — see the GOTCHA in
/// `input.rs`) appears once with `None`. Every command in [`Command::all`] has
/// at least one row.
pub(crate) fn default_bindings() -> &'static [(Command, Option<egui::KeyboardShortcut>)] {
    use egui::{Key, KeyboardShortcut, Modifiers};

    static BINDINGS: [(Command, Option<KeyboardShortcut>); 11] = [
        (
            Command::Open,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
        ),
        (
            Command::SaveImageAs,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
        ),
        // Copy/Paste: delivered as egui::Event::Copy / Event::Paste on winit.
        (Command::Copy, None),
        (Command::Paste, None),
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
    fn copy_and_paste_have_no_key_binding() {
        // Their real trigger is egui::Event::Copy / Event::Paste (input.rs).
        assert!(default_bindings()
            .iter()
            .any(|(c, s)| *c == Command::Copy && s.is_none()));
        assert!(default_bindings()
            .iter()
            .any(|(c, s)| *c == Command::Paste && s.is_none()));
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
}
