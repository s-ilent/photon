use crate::commands::Command;
use eframe::egui;

/// Scan this frame's input for commands, driven by the binding table passed in
/// (defaults with config overrides — see [`commands::effective_bindings`]).
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
pub(crate) fn take_commands(
    ctx: &egui::Context,
    bindings: &[(Command, Option<egui::KeyboardShortcut>)],
) -> Vec<Command> {
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

        // Key path, straight off the binding table. The Copy/Paste rows are
        // their raw-key fallback for backends that don't rewrite Ctrl+C/V;
        // `push_once` keeps that from double-firing with the events above.
        for &(command, shortcut) in bindings {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;

    fn with_events(events: Vec<egui::Event>) -> Vec<Command> {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        ctx.begin_pass(raw);
        let commands = take_commands(&ctx, commands::default_bindings());
        let _ = ctx.end_pass();
        commands
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
        let commands = with_events(vec![egui::Event::Copy]);
        assert!(commands.contains(&Command::Copy));
        assert!(!commands.contains(&Command::Paste));

        let commands = with_events(vec![egui::Event::Paste("pixels".into())]);
        assert!(commands.contains(&Command::Paste));
        assert!(!commands.contains(&Command::Copy));
    }

    /// Fallback for backends that deliver the raw key instead of rewriting it.
    #[test]
    fn ctrl_c_and_ctrl_v_key_events_trigger_copy_and_paste() {
        let commands = with_events(vec![key_event(egui::Key::C, egui::Modifiers::COMMAND)]);
        assert!(commands.contains(&Command::Copy));

        let commands = with_events(vec![key_event(egui::Key::V, egui::Modifiers::COMMAND)]);
        assert!(commands.contains(&Command::Paste));
    }

    #[test]
    fn navigation_and_mode_shortcuts_still_work() {
        let commands = with_events(vec![key_event(egui::Key::Space, egui::Modifiers::NONE)]);
        assert!(commands.contains(&Command::NextImage));

        let commands = with_events(vec![key_event(egui::Key::ArrowLeft, egui::Modifiers::NONE)]);
        assert!(commands.contains(&Command::PrevImage));

        let commands = with_events(vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)]);
        assert!(commands.contains(&Command::CycleMouseMode));
    }

    /// A command bound to two keys (Space and →) fires once, not twice.
    #[test]
    fn a_two_key_command_dispatches_once() {
        let commands = with_events(vec![
            key_event(egui::Key::Space, egui::Modifiers::NONE),
            key_event(egui::Key::ArrowRight, egui::Modifiers::NONE),
        ]);
        assert_eq!(
            commands
                .iter()
                .filter(|c| **c == Command::NextImage)
                .count(),
            1
        );
    }

    #[test]
    fn plain_key_press_without_shortcut_is_ignored() {
        let commands = with_events(vec![key_event(egui::Key::C, egui::Modifiers::NONE)]);
        assert!(commands.is_empty());
    }

    #[test]
    fn f11_toggles_fullscreen() {
        let commands = with_events(vec![key_event(egui::Key::F11, egui::Modifiers::NONE)]);
        assert!(commands.contains(&Command::ToggleFullscreen));
    }
}
