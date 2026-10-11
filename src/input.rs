use crate::commands::{self, Command};
use eframe::egui;

/// One frame's worth of shortcut actions, scanned off the egui input state.
///
/// Kept as the compatibility surface for `take_shortcut_actions` and its
/// tests; the live dispatch path uses [`take_commands`] directly.
#[allow(dead_code)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShortcutActions {
    pub(crate) open: bool,
    pub(crate) save: bool,
    pub(crate) copy: bool,
    pub(crate) paste: bool,
    pub(crate) mode_switch: bool,
    pub(crate) next: bool,
    pub(crate) prev: bool,
    pub(crate) fullscreen: bool,
}

/// Scan this frame's input for commands, driven by the registry's default
/// binding table ([`commands::default_bindings`]).
///
/// GOTCHA: egui-winit rewrites Ctrl+C / Ctrl+V / Ctrl+X at the input layer
/// into `Event::Copy` / `Event::Paste` / `Event::Cut` (its TextEdit clipboard
/// path) — on winit platforms the corresponding `Event::Key`s never arrive,
/// so copy/paste cannot be matched with `consume_shortcut` alone. We listen
/// for the command events AND keep the key-shortcut checks as a fallback for
/// backends that don't rewrite; on any one platform only one path can fire.
/// App-level handling deliberately takes these events before any widget:
/// Photon has no TextEdits today, but if one is ever added, it will not see
/// Ctrl+C/V and needs its own handling.
pub(crate) fn take_commands(ctx: &egui::Context) -> Vec<Command> {
    let mut commands = Vec::new();

    ctx.input_mut(|i| {
        // The real winit path: egui-winit turned Ctrl+C/V into command events.
        // Consume them so no widget sees them later in the pass.
        if i.events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy))
        {
            push_once(&mut commands, Command::Copy);
        }
        if i.events
            .iter()
            .any(|event| matches!(event, egui::Event::Paste(_)))
        {
            push_once(&mut commands, Command::Paste);
        }
        i.events
            .retain(|event| !matches!(event, egui::Event::Copy | egui::Event::Paste(_)));

        // Copy/Paste ride the command events above, so their table row is
        // `None`; these are the fallbacks for backends that deliver the raw
        // key instead of rewriting it.
        for (command, shortcut) in [
            (
                Command::Copy,
                egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::C),
            ),
            (
                Command::Paste,
                egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::V),
            ),
        ] {
            if i.consume_shortcut(&shortcut) {
                push_once(&mut commands, command);
            }
        }

        // Everything else is a single key read straight off the binding table.
        for &(command, shortcut) in commands::default_bindings() {
            if let Some(shortcut) = shortcut {
                if i.consume_shortcut(&shortcut) {
                    push_once(&mut commands, command);
                }
            }
        }
    });

    commands
}

/// Append `command` unless a binding for it already fired this frame, so a
/// command bound to two keys (e.g. Space and →) still yields one entry.
fn push_once(commands: &mut Vec<Command>, command: Command) {
    if !commands.contains(&command) {
        commands.push(command);
    }
}

/// Compatibility adapter over [`take_commands`] preserving the old
/// `ShortcutActions` shape (and its tests). The live path calls
/// [`take_commands`] and dispatches through `PhotonApp::run_command`.
#[allow(dead_code)]
pub(crate) fn take_shortcut_actions(ctx: &egui::Context) -> ShortcutActions {
    let mut actions = ShortcutActions::default();
    for command in take_commands(ctx) {
        match command {
            Command::Open => actions.open = true,
            Command::SaveImageAs => actions.save = true,
            Command::Copy => actions.copy = true,
            Command::Paste => actions.paste = true,
            Command::CycleMouseMode => actions.mode_switch = true,
            Command::NextImage => actions.next = true,
            Command::PrevImage => actions.prev = true,
            Command::ToggleFullscreen => actions.fullscreen = true,
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_events(events: Vec<egui::Event>) -> ShortcutActions {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        ctx.begin_pass(raw);
        let actions = take_shortcut_actions(&ctx);
        let _ = ctx.end_pass();
        actions
    }

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// The real winit path: egui-winit rewrites Ctrl+C/V into command events.
    #[test]
    fn clipboard_command_events_trigger_copy_and_paste() {
        let actions = with_events(vec![egui::Event::Copy]);
        assert!(actions.copy);
        assert!(!actions.paste);

        let actions = with_events(vec![egui::Event::Paste("pixels".into())]);
        assert!(actions.paste);
        assert!(!actions.copy);
    }

    /// Fallback for backends that deliver the raw key instead of rewriting it.
    #[test]
    fn ctrl_c_and_ctrl_v_key_events_trigger_copy_and_paste() {
        let actions = with_events(vec![key_event(egui::Key::C, egui::Modifiers::COMMAND)]);
        assert!(actions.copy);

        let actions = with_events(vec![key_event(egui::Key::V, egui::Modifiers::COMMAND)]);
        assert!(actions.paste);
    }

    #[test]
    fn navigation_and_mode_shortcuts_still_work() {
        let actions = with_events(vec![key_event(egui::Key::Space, egui::Modifiers::NONE)]);
        assert!(actions.next);

        let actions = with_events(vec![key_event(egui::Key::ArrowLeft, egui::Modifiers::NONE)]);
        assert!(actions.prev);

        let actions = with_events(vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert!(actions.mode_switch);
    }

    #[test]
    fn plain_key_press_without_shortcut_is_ignored() {
        let actions = with_events(vec![key_event(egui::Key::C, egui::Modifiers::NONE)]);
        assert!(!actions.copy);
        assert!(!actions.open);
        assert!(!actions.save);
    }

    #[test]
    fn f11_toggles_fullscreen() {
        let actions = with_events(vec![key_event(egui::Key::F11, egui::Modifiers::NONE)]);
        assert!(actions.fullscreen);
    }
}
