use crate::app;
use crate::commands::{Command, MenuGroup};
use eframe::egui;
use egui_phosphor::regular as icons;

/// One registry-backed toolbar button. `label` (optional) renders after the
/// icon; the tooltip gets its shortcut hint from the binding table.
struct ToolButton {
    command: Command,
    icon: &'static str,
    label: &'static str,
    tooltip: &'static str,
}

/// A toolbar slot: a button or a separator.
enum ToolSlot {
    Button(ToolButton),
    Separator,
}

/// Right-hand buttons, in right-to-left display order.
const TOOLS_RIGHT: [ToolButton; 2] = [
    ToolButton {
        command: Command::ToggleSettings,
        icon: icons::GEAR,
        label: "",
        tooltip: "Settings",
    },
    ToolButton {
        command: Command::ImageInformation,
        icon: icons::INFO,
        label: "",
        tooltip: "Image Information",
    },
];

/// Left-hand buttons, in display order.
const TOOLS_LEFT: [ToolSlot; 15] = [
    ToolSlot::Button(ToolButton {
        command: Command::Open,
        icon: icons::FOLDER_OPEN,
        label: "Open",
        tooltip: "Open",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::Copy,
        icon: icons::COPY,
        label: "Copy",
        tooltip: "Copy",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::Paste,
        icon: icons::CLIPBOARD,
        label: "Paste",
        tooltip: "Paste",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::SaveImageAs,
        icon: icons::FLOPPY_DISK,
        label: "Save",
        tooltip: "Save As",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::OpenBatchConvert,
        icon: icons::ARROWS_CLOCKWISE,
        label: "Convert\u{2026}",
        tooltip: "Batch convert files",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::SaveSelectionAs,
        icon: icons::SCISSORS,
        label: "Save Selection As\u{2026}",
        tooltip: "Save the selected region as a file",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::ToggleSidebar,
        icon: icons::SIDEBAR_SIMPLE,
        label: "Sidebar",
        tooltip: "Toggle the file list",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::PrevImage,
        icon: icons::CARET_LEFT,
        label: "Prev",
        tooltip: "Previous image",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::NextImage,
        icon: icons::CARET_RIGHT,
        label: "Next",
        tooltip: "Next image",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::FitImage,
        icon: icons::ARROWS_OUT_SIMPLE,
        label: "Fit",
        tooltip: "Fit to window",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::ZoomActualSize,
        icon: icons::FRAME_CORNERS,
        label: "100%",
        tooltip: "Actual size",
    }),
];

impl app::PhotonApp {
    /// One button: enable state from `command_enabled`, dispatch through
    /// `run_command`, tooltip with the binding-table shortcut hint.
    fn toolbar_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, button: &ToolButton) {
        let label = if button.label.is_empty() {
            button.icon.to_owned()
        } else {
            format!("{} {}", button.icon, button.label)
        };
        let tooltip = format!("{}{}", button.tooltip, button.command.shortcut_hint(ctx));
        if ui
            .add_enabled(
                self.command_enabled(button.command),
                egui::Button::new(label),
            )
            .on_hover_text(tooltip)
            .clicked()
        {
            self.run_command(button.command, ctx);
        }
    }

    /// Top menu bar (File / Edit / Image / View), driven by the command
    /// registry: the groups come from [`MenuGroup::ALL`], the items and their
    /// enable state from [`Command::all`], so a new command joins a menu just
    /// by declaring its [`MenuGroup`] (Roadmap §1).
    pub(crate) fn ui_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            for group in MenuGroup::ALL {
                ui.menu_button(group.title(), |ui| {
                    for &command in Command::all() {
                        if command.menu_group() != group {
                            continue;
                        }
                        if command.starts_menu_section() {
                            ui.separator();
                        }
                        if ui
                            .add_enabled(
                                self.command_enabled(command),
                                egui::Button::new(command.label()),
                            )
                            .clicked()
                        {
                            ui.close();
                            self.run_command(command, ctx);
                        }
                    }
                    // Lock-zoom and the window-fit/zoom actions are runtime
                    // state, not registry commands, so they are appended to the
                    // View menu directly.
                    if group == MenuGroup::View {
                        ui.separator();
                        let locked = self.zoom_lock.is_locked();
                        if ui.selectable_label(locked, "Lock Zoom").clicked() {
                            ui.close();
                            self.set_zoom_lock(!locked);
                        }
                        ui.separator();
                        let has_image = self.image_dimensions.is_some();
                        let can_window_fit =
                            has_image && self.settings.fit_window != app::view::FitWindow::DontFit;
                        if ui
                            .add_enabled(can_window_fit, egui::Button::new("Fit Window to Image"))
                            .clicked()
                        {
                            ui.close();
                            self.fit_window_to_image(ctx);
                        }
                        if ui
                            .add_enabled(has_image, egui::Button::new("Centre Window"))
                            .clicked()
                        {
                            ui.close();
                            self.center_window(ctx);
                        }
                        if ui
                            .add_enabled(has_image, egui::Button::new("Zoom to Desktop"))
                            .clicked()
                        {
                            ui.close();
                            self.zoom_to_desktop(ctx);
                        }
                    }
                });
            }
        });
    }

    pub(crate) fn ui_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Drain any finished background batch run every frame, regardless of
        // whether the Batch Convert window is currently open.
        self.check_batch_status(ctx);

        // Far-right elements are laid out first (right-to-left)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for button in &TOOLS_RIGHT {
                self.toolbar_button(ui, ctx, button);
            }
            ui.separator();
            // The mouse-mode button shows the live mode name, so it stays
            // outside the static table.
            if ui
                .button(format!("{} {}", icons::MOUSE_SIMPLE, self.mode.name()))
                .on_hover_text(format!(
                    "Mouse mode{}",
                    Command::CycleMouseMode.shortcut_hint(ctx)
                ))
                .clicked()
            {
                self.run_command(Command::CycleMouseMode, ctx);
            }

            // Left-aligned elements fill the remaining space on the left (left-to-right)
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                for slot in &TOOLS_LEFT {
                    match slot {
                        ToolSlot::Separator => {
                            ui.separator();
                        }
                        ToolSlot::Button(button) => self.toolbar_button(ui, ctx, button),
                    }
                }
            });
        });

        // The Batch Convert window is an `egui::Window`; drive it from here so
        // the toolbar is its single entry point.
        if self.batch.open {
            self.ui_batch_window(ctx);
        }
    }
}
