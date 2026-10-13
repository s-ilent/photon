use crate::app;
use eframe::egui;
use egui_phosphor::regular as icons;

impl app::PhotonApp {
    pub(crate) fn ui_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Sort:");
            egui::ComboBox::from_id_salt("sort_mode")
                .selected_text(self.settings.sort_mode.label())
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    for mode in [
                        app::SortMode::Alphabetical,
                        app::SortMode::AlphabeticalDesc,
                        app::SortMode::DateModified,
                        app::SortMode::DateModifiedOldest,
                        app::SortMode::FileSize,
                        app::SortMode::FileSizeSmallest,
                    ] {
                        if ui
                            .selectable_value(&mut self.settings.sort_mode, mode, mode.label())
                            .clicked()
                        {
                            changed = true;
                        }
                    }
                    if changed {
                        self.sort_images(self.settings.sort_mode);
                    }
                });
        });

        ui.separator();

        let num_images = self.images.len();
        let row_height = 28.0;

        let mut scroll_area = egui::ScrollArea::vertical();
        if self.scroll_to_target {
            let view_height = ui.available_height();
            let target_top = self.current_index as f32 * row_height;
            let scroll_offset = target_top - (view_height / 2.0) + (row_height / 2.0);
            scroll_area = scroll_area.vertical_scroll_offset(scroll_offset.max(0.0));
            self.scroll_to_target = false;
        }

        // Force zero spacing for perfect predictability
        ui.spacing_mut().item_spacing.y = 0.0;

        scroll_area.show_rows(ui, row_height, num_images, |ui, row_range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let align = self.settings.sidebar_align;
            for row in row_range {
                if sidebar_row(ui, &self.images[row], row == self.current_index, row_height, align)
                {
                    self.current_index = row;
                    self.request_current();
                }
            }
        });
    }
}

/// One sidebar entry: a full-width hit area with the filetype icon and a
/// middle-elided file name, aligned to the configured side. Returns true when
/// the entry was clicked.
fn sidebar_row(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    is_selected: bool,
    row_height: f32,
    align: app::Side,
) -> bool {
    let response = ui
        .allocate_response(egui::vec2(ui.available_width(), row_height), egui::Sense::click())
        .on_hover_text(path.display().to_string());

    let fill = if is_selected {
        Some(app::theme::PURPLE)
    } else if response.hovered() {
        Some(app::theme::SURFACE_HOVER)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(response.rect, 0.0, fill);
    }

    // Dark text reads on the accent selection fill.
    let text_color = if is_selected {
        app::theme::BG
    } else {
        app::theme::FG
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let max_width = (response.rect.width() - 16.0).max(0.0);
    let prefix = format!("{}  ", file_type_icon(path));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let text = middle_elide(&name, max_width, &prefix, |candidate| {
        ui.painter()
            .layout_no_wrap(candidate, font.clone(), text_color)
            .size()
            .x
    });

    let galley = ui.painter().layout_no_wrap(format!("{prefix}{text}"), font, text_color);
    let x = match align {
        app::Side::Right => response.rect.right() - 8.0 - galley.size().x,
        app::Side::Left => response.rect.left() + 8.0,
    };
    let pos = egui::pos2(x, response.rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(pos, galley, egui::Color32::TRANSPARENT);

    response.clicked()
}

/// macOS-style middle elision: keep the head and tail of `name` with an
/// ellipsis between until the composed `prefix + name` line fits `max_width`
/// (measured by `width_of`). Binary search over the kept-character count, so
/// a pathological name costs a handful of layout calls.
fn middle_elide(
    name: &str,
    max_width: f32,
    prefix: &str,
    mut width_of: impl FnMut(String) -> f32,
) -> String {
    if width_of(format!("{prefix}{name}")) <= max_width {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    let build = |keep: usize| -> String {
        let head = keep / 2;
        let tail = keep - head;
        let mut out: String = chars[..head].iter().collect();
        out.push('\u{2026}');
        out.extend(chars[chars.len() - tail..].iter());
        out
    };

    let mut lo = 0usize;
    let mut hi = chars.len();
    let mut best = String::from("\u{2026}");
    while lo <= hi {
        let keep = (lo + hi) / 2;
        let candidate = build(keep);
        if width_of(format!("{prefix}{candidate}")) <= max_width {
            best = candidate;
            lo = keep + 1;
        } else if keep == 0 {
            break;
        } else {
            hi = keep - 1;
        }
    }
    best
}

/// Phosphor's filetype glyphs where they exist, a generic file otherwise.
fn file_type_icon(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => icons::FILE_PNG,
        Some("jpg") | Some("jpeg") => icons::FILE_JPG,
        _ => icons::FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A monospace-ish measurer: every char is 10 wide, the prefix 20.
    fn measurer() -> impl FnMut(String) -> f32 {
        |text: String| text.chars().count() as f32 * 10.0 + 10.0 // + padding
    }

    #[test]
    fn short_names_pass_through() {
        let mut width_of = measurer();
        // prefix + name is 8 chars: 80 + 10 padding = 90 wide, fits 100.
        assert_eq!(middle_elide("a.png", 100.0, "\u{f115}  ", &mut width_of), "a.png");
    }

    #[test]
    fn long_names_keep_head_and_tail_with_an_ellipsis() {
        let mut width_of = measurer();
        let name = "abcdefghij".repeat(4); // 40 chars: 400 wide + prefix/padding
        let elided = middle_elide(&name, 120.0, "P  ", &mut width_of);
        assert!(elided.contains('\u{2026}'));
        assert!(elided.starts_with('a'), "keeps the head: {elided}");
        assert!(elided.ends_with('j'), "keeps the tail: {elided}");
        // The result actually fits: prefix (3) + elided + padding <= 12 chars.
        assert!(format!("P  {elided}").chars().count() * 10 + 10 <= 120);
    }

    #[test]
    fn a_hopeless_name_elides_to_the_ellipsis() {
        let mut width_of = measurer();
        let elided = middle_elide("abcdef", 15.0, "", &mut width_of);
        assert_eq!(elided, "\u{2026}");
    }
}
