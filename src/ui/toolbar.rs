use crate::app;
use crate::commands::{Command, MenuGroup};
use eframe::egui;
use egui_phosphor::regular as icons;

/// One registry-backed toolbar button: icon only, purpose on hover; the
/// tooltip gets its shortcut hint from the binding table.
struct ToolButton {
    command: Command,
    icon: &'static str,
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
        tooltip: "Preferences",
    },
    ToolButton {
        command: Command::ImageInformation,
        icon: icons::INFO,
        tooltip: "Image Information",
    },
];

/// Left-hand buttons, in display order.
const TOOLS_LEFT: [ToolSlot; 15] = [
    ToolSlot::Button(ToolButton {
        command: Command::Open,
        icon: icons::FOLDER_OPEN,
        tooltip: "Open a file or folder",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::Copy,
        icon: icons::COPY,
        tooltip: "Copy the image to the clipboard",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::Paste,
        icon: icons::CLIPBOARD,
        tooltip: "Paste an image from the clipboard",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::SaveImageAs,
        icon: icons::FLOPPY_DISK,
        tooltip: "Save the image as a new file",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::OpenBatchConvert,
        icon: icons::ARROWS_CLOCKWISE,
        tooltip: "Batch convert files",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::SaveSelectionAs,
        icon: icons::SCISSORS,
        tooltip: "Save the selected region as a file",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::ToggleSidebar,
        icon: icons::SIDEBAR_SIMPLE,
        tooltip: "Toggle the file list",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::PrevImage,
        icon: icons::CARET_LEFT,
        tooltip: "Previous image",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::NextImage,
        icon: icons::CARET_RIGHT,
        tooltip: "Next image",
    }),
    ToolSlot::Separator,
    ToolSlot::Button(ToolButton {
        command: Command::FitImage,
        icon: icons::ARROWS_OUT_SIMPLE,
        tooltip: "Fit to window",
    }),
    ToolSlot::Button(ToolButton {
        command: Command::ZoomActualSize,
        icon: icons::FRAME_CORNERS,
        tooltip: "Actual size",
    }),
];

impl app::PhotonApp {
    /// One button: enable state from `command_enabled`, dispatch through
    /// `run_command`, tooltip with the binding-table shortcut hint.
    fn toolbar_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, button: &ToolButton) {
        let tooltip = format!(
            "{}{}",
            button.tooltip,
            button.command.shortcut_hint(ctx, &self.bindings)
        );
        if ui
            .add_enabled(
                self.command_enabled(button.command),
                egui::Button::new(button.icon),
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
    ///
    /// Hover switching: egui 0.34 opens a top-level menu as a memory-tracked
    /// popup and switches only on a click. Each title's popup id is collected
    /// here so a hover on another title can move the open menu, as a menu bar
    /// is expected to behave (see [`switch_open_menu_on_hover`]).
    pub(crate) fn ui_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut titles: Vec<(egui::Id, egui::Rect)> = Vec::new();
        egui::MenuBar::new().ui(ui, |ui| {
            for group in MenuGroup::ALL {
                let response = ui.menu_button(group.title(), |ui| {
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
                })
                .response;
                titles.push((
                    egui::Popup::default_response_id(&response),
                    response.rect.expand2(ui.spacing().item_spacing / 2.0),
                ));
            }
        });
        switch_open_menu_on_hover(ctx, &titles);
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
            // The mouse-mode button is icon-only; the live mode name lives in
            // the tooltip, so it stays outside the static table.
            if ui
                .button(icons::MOUSE_SIMPLE)
                .on_hover_text(format!(
                    "Mouse mode: {}{}",
                    self.mode.name(),
                    Command::CycleMouseMode.shortcut_hint(ctx, &self.bindings)
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

    }
}

/// Menu-bar hover switching: while one of the bar's menus is open, a pointer
/// over a different title closes it and opens that title's menu. Egui's own
/// switching lives in `SubMenu` and never runs for the bar's top-level popups.
fn switch_open_menu_on_hover(ctx: &egui::Context, titles: &[(egui::Id, egui::Rect)]) {
    let Some(open) = titles
        .iter()
        .find(|(id, _)| egui::Popup::is_id_open(ctx, *id))
    else {
        return;
    };
    let Some(pointer) = ctx.pointer_hover_pos() else {
        return;
    };
    if let Some((hovered, _)) = titles
        .iter()
        .find(|(id, rect)| *id != open.0 && rect.contains(pointer))
    {
        // One popup is open at a time, so opening the hovered menu closes the open one.
        egui::Popup::open_id(ctx, *hovered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable as _;

    /// Photon's real menu bar in the same top panel the app uses.
    fn menu_bar_harness() -> egui_kittest::Harness<'static> {
        let mut app = app::PhotonApp::new();
        egui_kittest::Harness::new_ui(move |ui| {
            let ctx = ui.ctx().clone();
            egui::Panel::top("menubar").show_inside(ui, |ui| app.ui_menu_bar(ui, &ctx));
        })
    }

    /// Menubar behaviour: hovering a neighbouring title while a menu is open
    /// moves the open menu to the hovered title.
    #[test]
    fn hovering_a_neighbouring_title_switches_the_open_menu() {
        let mut harness = menu_bar_harness();
        harness.run();

        harness.get_by_label("View").click();
        harness.run();
        assert!(
            harness.query_by_label("Zoom to Desktop").is_some(),
            "the View menu opens on click"
        );

        // Two frames: the first sees the hover, the second shows the menu.
        harness.get_by_label("File").hover();
        harness.run();
        harness.run();
        assert!(
            harness.query_by_label("Zoom to Desktop").is_none(),
            "the View menu closes when File is hovered"
        );
        assert!(
            harness.query_by_label("Image Information…").is_some(),
            "File's menu opens on hover"
        );
    }

    /// Hover alone must not open a menu when none is open.
    #[test]
    fn hovering_a_title_with_no_menu_open_opens_nothing() {
        let mut harness = menu_bar_harness();
        harness.run();

        harness.get_by_label("File").hover();
        harness.run();
        harness.run();
        assert!(harness.query_by_label("Open…").is_none());
        assert!(harness.query_by_label("Zoom to Desktop").is_none());
    }
}
