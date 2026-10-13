//! Batch Convert window (Roadmap.md §5): the interactive front end for the
//! `crate::app::batch` engine. Mirrors `ui_settings_window` in style, keeps the
//! window body in a free function over `&mut BatchState` (so it never borrows
//! the whole `PhotonApp`), and runs conversions on a background thread —
//! reporting through the `BatchMessage` channel drained in `check_batch_status`.

use crate::app::batch::{
    self, ConversionJob, FileStatus, OutputFormat, OverwritePolicy, SaveOptions,
};
use crate::app::image_op::{self, FilterKind, ImageOp, ResampleFilter};
use crate::app::theme;
use crate::app::{BatchMessage, BatchState, PhotonApp};
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icons;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// One entry of the "add step" combo. Kept as a small ordinal list so the
/// selection can live in [`BatchState`] as a plain `usize`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OperationChoice {
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
    Grayscale,
    Negative,
    Resize,
    Filter(FilterKind),
}

impl OperationChoice {
    const ALL: [OperationChoice; 15] = [
        OperationChoice::Rotate90,
        OperationChoice::Rotate180,
        OperationChoice::Rotate270,
        OperationChoice::FlipHorizontal,
        OperationChoice::FlipVertical,
        OperationChoice::Grayscale,
        OperationChoice::Negative,
        OperationChoice::Resize,
        OperationChoice::Filter(FilterKind::Blur),
        OperationChoice::Filter(FilterKind::Soften),
        OperationChoice::Filter(FilterKind::Sharpen),
        OperationChoice::Filter(FilterKind::Gaussian3),
        OperationChoice::Filter(FilterKind::Gaussian5),
        OperationChoice::Filter(FilterKind::Emboss),
        OperationChoice::Filter(FilterKind::Edge),
    ];

    fn label(self) -> &'static str {
        match self {
            OperationChoice::Rotate90 => "Rotate 90",
            OperationChoice::Rotate180 => "Rotate 180",
            OperationChoice::Rotate270 => "Rotate 270",
            OperationChoice::FlipHorizontal => "Flip horizontal",
            OperationChoice::FlipVertical => "Flip vertical",
            OperationChoice::Grayscale => "Grayscale",
            OperationChoice::Negative => "Negative",
            OperationChoice::Resize => "Resize",
            OperationChoice::Filter(kind) => kind.label(),
        }
    }

    /// Build the engine operation for this choice. `width`/`height` of `0` mean
    /// "auto" (keep aspect ratio); a resize with both unset is a no-op and
    /// returns `None` so the UI doesn't queue a step that does nothing.
    fn to_operation(self, width: u32, height: u32, filter: ResampleFilter) -> Option<ImageOp> {
        Some(match self {
            OperationChoice::Rotate90 => ImageOp::RotateRight,
            OperationChoice::Rotate180 => ImageOp::Rotate180,
            OperationChoice::Rotate270 => ImageOp::RotateLeft,
            OperationChoice::FlipHorizontal => ImageOp::FlipHorizontal,
            OperationChoice::FlipVertical => ImageOp::FlipVertical,
            OperationChoice::Grayscale => ImageOp::Grayscale,
            OperationChoice::Negative => ImageOp::Negative,
            OperationChoice::Filter(kind) => ImageOp::Filter(kind),
            OperationChoice::Resize => {
                if width == 0 && height == 0 {
                    return None;
                }
                ImageOp::Resize {
                    width: (width != 0).then_some(width),
                    height: (height != 0).then_some(height),
                    filter,
                }
            }
        })
    }
}

fn filter_label(filter: ResampleFilter) -> &'static str {
    match filter {
        ResampleFilter::Nearest => "Nearest",
        ResampleFilter::Triangle => "Triangle",
        ResampleFilter::CatmullRom => "Catmull-Rom",
        ResampleFilter::Gaussian => "Gaussian",
        ResampleFilter::Lanczos3 => "Lanczos3",
    }
}

fn operation_label(op: &ImageOp) -> String {
    match op {
        ImageOp::RotateRight => "Rotate 90".to_string(),
        ImageOp::Rotate180 => "Rotate 180".to_string(),
        ImageOp::RotateLeft => "Rotate 270".to_string(),
        ImageOp::FlipHorizontal => "Flip horizontal".to_string(),
        ImageOp::FlipVertical => "Flip vertical".to_string(),
        ImageOp::Grayscale => "Grayscale".to_string(),
        ImageOp::Negative => "Negative".to_string(),
        ImageOp::Filter(kind) => kind.label().to_string(),
        ImageOp::Crop {
            left,
            top,
            right,
            bottom,
        } => format!("Crop {left},{top},{right},{bottom}"),
        ImageOp::Resize {
            width,
            height,
            filter,
        } => {
            let w = width
                .map(|v| v.to_string())
                .unwrap_or_else(|| "auto".to_string());
            let h = height
                .map(|v| v.to_string())
                .unwrap_or_else(|| "auto".to_string());
            format!("Resize {}x{} ({})", w, h, filter_label(*filter))
        }
    }
}

/// Plain-text status for one row: Success / Skipped / Failed, uncoloured, as
/// the rest of the project keeps status text.
fn status_label(status: &Option<FileStatus>) -> String {
    match status {
        None => "-".to_string(),
        Some(FileStatus::Success) => "Success".to_string(),
        Some(FileStatus::Skipped(reason)) => format!("Skipped ({})", reason),
        Some(FileStatus::Failed(error)) => format!("Failed: {}", error),
    }
}

/// Count `(success, skipped, failed)` among the reported statuses. `None`
/// (not yet reported) is ignored so the summary can be shown mid-run too.
fn summarize(statuses: &[Option<FileStatus>]) -> (usize, usize, usize) {
    let mut success = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for status in statuses.iter().flatten() {
        match status {
            FileStatus::Success => success += 1,
            FileStatus::Skipped(_) => skipped += 1,
            FileStatus::Failed(_) => failed += 1,
        }
    }
    (success, skipped, failed)
}

/// Build one [`ConversionJob`] per source file, all sharing the run's format
/// and operation chain. The output folder is the chosen `target_dir`, or each
/// file's own directory when none was chosen. Encoder settings travel
/// separately as [`SaveOptions`]. Pure — no I/O.
fn build_jobs(
    files: &[PathBuf],
    target_dir: Option<&Path>,
    format: OutputFormat,
    operations: &[ImageOp],
) -> Vec<ConversionJob> {
    files
        .iter()
        .map(|source| ConversionJob {
            source: source.clone(),
            target_dir: target_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| source.parent().unwrap_or(Path::new(".")).to_path_buf()),
            format,
            operations: operations.to_vec(),
        })
        .collect()
}

/// Append `paths` to the queue, skipping any already present (order preserved).
fn add_files(batch: &mut BatchState, paths: Vec<PathBuf>) {
    for path in paths {
        if !batch.files.contains(&path) {
            batch.files.push(path);
        }
    }
}

/// Convert `jobs` on a background thread and report progress plus the per-file
/// statuses back over the returned channel. `run_batch_with_progress` fans the
/// work out across `rayon`'s pool and reports `(done, total)` as files finish,
/// so the progress bar is determinate; the final `Finished` message arrives once
/// every file is done.
fn spawn_batch(
    jobs: Vec<ConversionJob>,
    policy: OverwritePolicy,
    options: SaveOptions,
) -> Receiver<BatchMessage> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let statuses =
            batch::run_batch_with_progress(&jobs, policy, &options, move |done, total| {
                let _ = progress_tx.send(BatchMessage::Progress { done, total });
            });
        let _ = tx.send(BatchMessage::Finished(statuses));
    });
    rx
}

/// Validate the current state and kick off a run. No-op when preconditions
/// are unmet (no files); the Run button is disabled in that case anyway.
/// Re-derive the recipe field's text from the chain (after a button changed
/// it). Typed edits flow the other way: text parses into `operations`.
fn sync_recipe_text(batch: &mut BatchState) {
    batch.recipe_text = image_op::recipe_text(&batch.operations);
}

fn start_batch(batch: &mut BatchState) {
    if batch.files.is_empty() {
        return;
    }
    let jobs = build_jobs(
        &batch.files,
        batch.output_dir.as_deref(),
        batch.format,
        &batch.operations,
    );
    batch.statuses = vec![None; jobs.len()];
    batch.progress = (0, jobs.len());
    batch.running = true;
    batch.receiver = Some(spawn_batch(jobs, batch.overwrite, batch.save_options));
}

impl PhotonApp {
    /// Poll the background batch run and swap in its statuses. Called every
    /// frame (like `check_save_status`), so a run finishes even if the window
    /// has been closed; `request_repaint` keeps the spinner and progress bar
    /// animating while it works.
    pub(crate) fn check_batch_status(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = self.batch.receiver.take() {
            match receiver.try_recv() {
                Ok(BatchMessage::Progress { done, total }) => {
                    self.batch.progress = (done, total);
                    self.batch.receiver = Some(receiver);
                    ctx.request_repaint();
                }
                Ok(BatchMessage::Finished(statuses)) => {
                    self.batch.statuses = statuses.into_iter().map(Some).collect();
                    self.batch.running = false;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    self.batch.receiver = Some(receiver);
                    ctx.request_repaint();
                }
                Err(TryRecvError::Disconnected) => {
                    self.batch.running = false;
                    ctx.request_repaint();
                }
            }
        }
    }
}

/// Render the Batch Convert surface while it is open: a native OS child
/// window (viewport), not an in-app pane — its title bar, resizing and close
/// button belong to the window manager. Called from `main` each frame, after
/// the panels, so the body's state edits land as complete passes.
pub(crate) fn show(ctx: &egui::Context, batch: &mut BatchState, images: &[PathBuf]) {
    if !batch.open {
        return;
    }
    let mut builder = egui::ViewportBuilder::default()
        .with_title("Batch Convert")
        .with_inner_size([560.0, 480.0])
        .with_min_inner_size([380.0, 260.0])
        .with_active(true);
    if ctx.embed_viewports() {
        // No native child windows on this platform: drop the OS-window
        // contract so the embedded fallback follows its host canvas.
        builder.min_inner_size = None;
    }
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("batch_convert"),
        builder,
        |ui, class| {
            if class == egui::ViewportClass::Immediate {
                if ui.input(|i| i.viewport().close_requested()) {
                    batch.open = false;
                    return;
                }
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    batch_window_body(ui, batch, images);
                });
            } else {
                // Embedded fallback: the surface is a child Ui of the host.
                batch_window_body(ui, batch, images);
            }
        },
    );
}

/// The window body. A free function over the batch state and the app's current
/// file list, so the viewport closure captures disjoint fields of `PhotonApp`
/// rather than the whole app.
fn batch_window_body(ui: &mut egui::Ui, batch: &mut BatchState, images: &[PathBuf]) {
    file_list_section(ui, batch, images);
    ui.separator();
    output_section(ui, batch);
    ui.separator();
    operations_section(ui, batch);
    ui.separator();
    recipe_section(ui, batch);
    run_section(ui, batch);
}

/// The file list table, with the add/clear actions and the count below it.
fn file_list_section(ui: &mut egui::Ui, batch: &mut BatchState, images: &[PathBuf]) {
    let width = ui.available_width();
    let mut remove: Option<usize> = None;
    TableBuilder::new(ui)
        .id_salt("batch_file_list")
        .striped(true)
        .resizable(true)
        .column(Column::initial(width * 0.28).resizable(true).clip(true))
        .column(Column::remainder().clip(true))
        .column(Column::exact(26.0).resizable(false).clip(true))
        .column(Column::exact(26.0).resizable(false).clip(true))
        .max_scroll_height(140.0)
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.strong("Filename");
            });
            header.col(|ui| {
                ui.strong("Path");
            });
            header.col(|_ui| {});
            header.col(|_ui| {});
        })
        .body(|body| {
            body.rows(22.0, batch.files.len(), |mut row| {
                let index = row.index();
                let path = &batch.files[index];
                row.col(|ui| {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    ui.label(name);
                });
                row.col(|ui| {
                    let parent = path
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ui.label(parent).on_hover_text(path.display().to_string());
                });
                row.col(|ui| {
                    let status = batch.statuses.get(index).unwrap_or(&None);
                    let (icon, color) = status_icon(status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(icon).color(color))
                            .on_hover_text(status_label(status));
                    });
                });
                row.col(|ui| {
                    if ui.small_button(icons::X).clicked() {
                        remove = Some(index);
                    }
                });
            });
        });
    if let Some(index) = remove {
        batch.files.remove(index);
        if index < batch.statuses.len() {
            batch.statuses.remove(index);
        }
    }

    ui.horizontal(|ui| {
        if ui.button("Add files\u{2026}").clicked() {
            let picked = rfd::FileDialog::new()
                .add_filter("Images", crate::dir_walker::SUPPORTED_EXTENSIONS)
                .pick_files();
            if let Some(paths) = picked {
                add_files(batch, paths);
            }
        }
        if ui.button("Add current folder").clicked() {
            add_files(batch, images.to_vec());
        }
        if ui.button("Clear").clicked() {
            batch.files.clear();
            batch.statuses.clear();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("{} file(s)", batch.files.len()));
        });
    });
}
fn output_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    egui::Grid::new("batch_output")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            grid_label(ui, "Format:");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("batch_format")
                    .selected_text(batch.format.extension().to_uppercase())
                    .show_ui(ui, |ui| {
                        for format in [
                            OutputFormat::Png,
                            OutputFormat::Jpeg,
                            OutputFormat::Gif,
                            OutputFormat::Bmp,
                            OutputFormat::Tiff,
                            OutputFormat::Webp,
                            OutputFormat::Tga,
                        ] {
                            ui.selectable_value(
                                &mut batch.format,
                                format,
                                format.extension().to_uppercase(),
                            );
                        }
                    });
                if batch.format == OutputFormat::Jpeg {
                    ui.label("Quality:");
                    ui.add(
                        egui::DragValue::new(&mut batch.save_options.jpeg_quality).range(1..=100),
                    );
                }
            });
            ui.end_row();

            grid_label(ui, "If target exists:");
            egui::ComboBox::from_id_salt("batch_overwrite")
                .selected_text(match batch.overwrite {
                    OverwritePolicy::Overwrite => "Overwrite",
                    OverwritePolicy::Skip => "Skip",
                    // Headlessly there is nobody to ask, so `Ask` behaves as skip.
                    OverwritePolicy::Ask => "Ask (skips)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut batch.overwrite,
                        OverwritePolicy::Overwrite,
                        "Overwrite",
                    );
                    ui.selectable_value(&mut batch.overwrite, OverwritePolicy::Skip, "Skip");
                    ui.selectable_value(&mut batch.overwrite, OverwritePolicy::Ask, "Ask (skips)");
                });
            ui.end_row();

            grid_label(ui, "Output folder:");
            ui.horizontal(|ui| {
                if ui.button("Pick folder\u{2026}").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        batch.output_dir = Some(dir);
                    }
                }
                match &batch.output_dir {
                    Some(dir) => {
                        ui.add(egui::Label::new(dir.display().to_string()).truncate());
                    }
                    None => {
                        ui.label("(none)");
                    }
                }
            });
            ui.end_row();
        });
}

/// A right-aligned form label with its colon (UI-DESIGN §4).
fn grid_label(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(text);
    });
}

fn operations_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    ui.label("Operations");
    sync_recipe_text(batch);

    let width = ui.available_width();
    let mut remove_op: Option<usize> = None;
    TableBuilder::new(ui)
        .id_salt("batch_operations")
        .striped(true)
        .resizable(true)
        .column(Column::exact(26.0).resizable(false).clip(true))
        .column(Column::initial(width * 0.30).resizable(true).clip(true))
        .column(Column::remainder().clip(true))
        .column(Column::exact(26.0).resizable(false).clip(true))
        .max_scroll_height(120.0)
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.strong("#");
            });
            header.col(|ui| {
                ui.strong("Operation");
            });
            header.col(|ui| {
                ui.strong("Switch");
            });
            header.col(|_ui| {});
        })
        .body(|body| {
            body.rows(22.0, batch.operations.len(), |mut row| {
                let index = row.index();
                let op = &batch.operations[index];
                row.col(|ui| {
                    ui.label(format!("{}.", index + 1));
                });
                row.col(|ui| {
                    ui.label(operation_label(op));
                });
                row.col(|ui| {
                    ui.monospace(op.to_cli());
                });
                row.col(|ui| {
                    if ui.small_button(icons::X).clicked() {
                        remove_op = Some(index);
                    }
                });
            });
        });
    if let Some(index) = remove_op {
        batch.operations.remove(index);
        sync_recipe_text(batch);
    }

    // Fixed rows: the parameters stay visible but disabled unless the chosen
    // step uses them, so the layout never jumps (UI-DESIGN §5).
    let choice = OperationChoice::ALL[batch.pending_op.min(OperationChoice::ALL.len() - 1)];
    let resizing = choice == OperationChoice::Resize;
    egui::Grid::new("batch_add_step")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            grid_label(ui, "Add step:");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("batch_op")
                    .selected_text(choice.label())
                    .show_ui(ui, |ui| {
                        for (index, option) in OperationChoice::ALL.iter().enumerate() {
                            ui.selectable_value(&mut batch.pending_op, index, option.label());
                        }
                    });
                if ui.add_enabled(
                    choice
                        .to_operation(batch.resize_width, batch.resize_height, batch.resize_filter)
                        .is_some(),
                    egui::Button::new("Add step"),
                )
                .clicked()
                {
                    if let Some(op) = choice
                        .to_operation(batch.resize_width, batch.resize_height, batch.resize_filter)
                    {
                        batch.operations.push(op);
                        sync_recipe_text(batch);
                    }
                }
            });
            ui.end_row();

            grid_label(ui, "Parameters:");
            ui.horizontal(|ui| {
                ui.add_enabled(
                    resizing,
                    egui::DragValue::new(&mut batch.resize_width).range(0..=100_000),
                );
                ui.label("\u{d7}");
                ui.add_enabled(
                    resizing,
                    egui::DragValue::new(&mut batch.resize_height).range(0..=100_000),
                );
                ui.add_enabled_ui(resizing, |ui| {
                    egui::ComboBox::from_id_salt("batch_filter")
                        .selected_text(filter_label(batch.resize_filter))
                        .show_ui(ui, |ui| {
                            for filter in [
                                ResampleFilter::Nearest,
                                ResampleFilter::Triangle,
                                ResampleFilter::CatmullRom,
                                ResampleFilter::Gaussian,
                                ResampleFilter::Lanczos3,
                            ] {
                                ui.selectable_value(
                                    &mut batch.resize_filter,
                                    filter,
                                    filter_label(filter),
                                );
                            }
                        });
                });
            });
            ui.end_row();
        });
}
/// The per-file outcome as a status-bar icon (text on hover).
fn status_icon(status: &Option<FileStatus>) -> (&'static str, egui::Color32) {
    match status {
        None => (icons::CIRCLE_NOTCH, theme::SUBDUED),
        Some(FileStatus::Success) => (icons::CHECK, theme::FG),
        Some(FileStatus::Skipped(_)) => (icons::MINUS, theme::SUBDUED),
        Some(FileStatus::Failed(_)) => (icons::X_CIRCLE, theme::RED),
    }
}

/// The operation chain as editable recipe text — the same serialisation the
/// command line takes. Edits re-parse into the chain live; a parse error is
/// shown and leaves the chain untouched. Ctrl+C/V belong to the field (the
/// app pauses command dispatch while a text field owns the keyboard).
fn recipe_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    ui.label("Recipe");
    let field = ui.add(
        egui::TextEdit::singleline(&mut batch.recipe_text)
            .id_salt("batch_recipe")
            .font(egui::TextStyle::Monospace)
            .desired_width(ui.available_width()),
    );
    if field.changed() {
        match image_op::parse_recipe(&batch.recipe_text) {
            Ok(ops) => {
                batch.operations = ops;
                batch.recipe_error = None;
            }
            Err(e) => batch.recipe_error = Some(e),
        }
    }
    if let Some(error) = &batch.recipe_error {
        ui.label(egui::RichText::new(error).color(theme::RED));
    }
}


fn run_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    let can_run = !batch.running && !batch.files.is_empty();
    ui.horizontal(|ui| {
        if batch.running {
            ui.add(egui::Spinner::new());
            ui.label("Converting\u{2026}");
        } else if !batch.statuses.is_empty() {
            let (success, skipped, failed) = summarize(&batch.statuses);
            ui.label(format!(
                "Done: {} success, {} skipped, {} failed",
                success, skipped, failed
            ));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(can_run, egui::Button::new("Run")).clicked() {
                start_batch(batch);
            }
        });
    });

    if batch.running {
        // Determinate: `run_batch_with_progress` reports (done, total).
        let (done, total) = batch.progress;
        let fraction = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };
        ui.add(egui::ProgressBar::new(fraction).show_percentage());
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_op() -> ImageOp {
        ImageOp::Resize {
            width: Some(100),
            height: None,
            filter: ResampleFilter::Lanczos3,
        }
    }

    #[test]
    fn build_jobs_maps_every_file_with_shared_settings() {
        let files = vec![PathBuf::from("/in/a.png"), PathBuf::from("/in/b.jpg")];
        let ops = vec![ImageOp::Grayscale, sample_op()];
        let jobs = build_jobs(&files, Some(Path::new("/out")), OutputFormat::Jpeg, &ops);

        assert_eq!(jobs.len(), 2);
        for (job, file) in jobs.iter().zip(&files) {
            assert_eq!(&job.source, file);
            assert_eq!(job.target_dir, PathBuf::from("/out"));
            assert_eq!(job.format, OutputFormat::Jpeg);
            assert_eq!(job.operations, ops);
        }
    }

    #[test]
    fn build_jobs_without_a_target_folder_writes_alongside_each_input() {
        let files = vec![PathBuf::from("/in/a.png"), PathBuf::from("/other/b.jpg")];
        let jobs = build_jobs(&files, None, OutputFormat::Jpeg, &[]);

        assert_eq!(jobs[0].target_dir, PathBuf::from("/in"));
        assert_eq!(jobs[1].target_dir, PathBuf::from("/other"));
    }

    #[test]
    fn status_label_is_plain_text_per_variant() {
        assert_eq!(status_label(&None), "-");
        assert_eq!(status_label(&Some(FileStatus::Success)), "Success");
        assert_eq!(
            status_label(&Some(FileStatus::Skipped("exists".to_string()))),
            "Skipped (exists)"
        );
        assert_eq!(
            status_label(&Some(FileStatus::Failed("boom".to_string()))),
            "Failed: boom"
        );
    }

    #[test]
    fn summarize_counts_each_outcome_and_ignores_pending() {
        let statuses = vec![
            Some(FileStatus::Success),
            None,
            Some(FileStatus::Skipped("exists".to_string())),
            Some(FileStatus::Failed("boom".to_string())),
            Some(FileStatus::Success),
        ];
        assert_eq!(summarize(&statuses), (2, 1, 1));
    }

    #[test]
    fn add_files_deduplicates_and_preserves_order() {
        let mut batch = BatchState::default();
        add_files(
            &mut batch,
            vec![
                PathBuf::from("/a.png"),
                PathBuf::from("/b.png"),
                PathBuf::from("/a.png"),
            ],
        );
        assert_eq!(
            batch.files,
            vec![PathBuf::from("/a.png"), PathBuf::from("/b.png")]
        );
    }

    #[test]
    fn resize_with_no_dimensions_queues_nothing() {
        assert_eq!(
            OperationChoice::Resize.to_operation(0, 0, ResampleFilter::Nearest),
            None
        );
        assert_eq!(
            OperationChoice::Grayscale.to_operation(0, 0, ResampleFilter::Nearest),
            Some(ImageOp::Grayscale)
        );
    }

    #[test]
    fn filter_choices_map_to_filter_operations() {
        for kind in [
            FilterKind::Blur,
            FilterKind::Soften,
            FilterKind::Sharpen,
            FilterKind::Gaussian3,
            FilterKind::Gaussian5,
            FilterKind::Emboss,
            FilterKind::Edge,
        ] {
            assert_eq!(
                OperationChoice::Filter(kind).to_operation(0, 0, ResampleFilter::Nearest),
                Some(ImageOp::Filter(kind)),
                "{kind:?} should build a Filter operation"
            );
        }
    }


    #[test]
    fn operation_choice_labels_are_nonempty_and_distinct() {
        let mut seen = std::collections::HashSet::new();
        for choice in OperationChoice::ALL {
            let label = choice.label();
            assert!(!label.is_empty());
            assert!(seen.insert(label), "duplicate operation label {label:?}");
        }
        assert_eq!(seen.len(), OperationChoice::ALL.len());
    }
}

#[cfg(test)]
mod table_probe {
    //! Temporary probe: do the table headers actually paint?
    use super::*;

    #[test]
    fn headers_paint() {
        let ctx = egui::Context::default();
        let mut batch = BatchState {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..BatchState::default()
        };
        for frame in 0..2 {
            let out = ctx.run_ui(egui::RawInput::default(), |ctx| {
                let mut open = true;
                egui::Window::new("Batch Convert")
                    .open(&mut open)
                    .show(ctx, |ui| {
                        file_list_section(ui, &mut batch, &[]);
                    });
            });
            let mut texts: Vec<String> = Vec::new();
            for clipped in &out.shapes {
                if let egui::Shape::Text(text) = &clipped.shape {
                    texts.push(text.galley.text().to_owned());
                }
            }
            let joined = texts.join(" | ");
            println!("frame {frame} text shapes: {joined}");
            if frame == 1 {
                assert!(joined.contains("Filename"), "header text painted: {joined}");
                assert!(joined.contains("Path"), "header text painted: {joined}");
                assert!(joined.contains("a.png"), "row text painted: {joined}");
            }
        }
    }
}
