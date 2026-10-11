//! Batch Convert window (Roadmap.md §5): the interactive front end for the
//! `crate::app::batch` engine. Mirrors `ui_settings_window` in style, keeps the
//! window body in a free function over `&mut BatchState` (so it never borrows
//! the whole `PhotonApp`), and runs conversions on a background thread —
//! reporting through the `BatchMessage` channel drained in `check_batch_status`.

use crate::app::batch::{
    self, ConversionJob, FileStatus, Operation, OutputFormat, OverwritePolicy, ResampleFilter,
};
use crate::app::{BatchMessage, BatchState, PhotonApp};
use eframe::egui;
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
}

impl OperationChoice {
    const ALL: [OperationChoice; 8] = [
        OperationChoice::Rotate90,
        OperationChoice::Rotate180,
        OperationChoice::Rotate270,
        OperationChoice::FlipHorizontal,
        OperationChoice::FlipVertical,
        OperationChoice::Grayscale,
        OperationChoice::Negative,
        OperationChoice::Resize,
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
        }
    }

    /// Build the engine operation for this choice. `width`/`height` of `0` mean
    /// "auto" (keep aspect ratio); a resize with both unset is a no-op and
    /// returns `None` so the UI doesn't queue a step that does nothing.
    fn to_operation(self, width: u32, height: u32, filter: ResampleFilter) -> Option<Operation> {
        Some(match self {
            OperationChoice::Rotate90 => Operation::Rotate90,
            OperationChoice::Rotate180 => Operation::Rotate180,
            OperationChoice::Rotate270 => Operation::Rotate270,
            OperationChoice::FlipHorizontal => Operation::FlipHorizontal,
            OperationChoice::FlipVertical => Operation::FlipVertical,
            OperationChoice::Grayscale => Operation::Grayscale,
            OperationChoice::Negative => Operation::Negative,
            OperationChoice::Resize => {
                if width == 0 && height == 0 {
                    return None;
                }
                Operation::Resize {
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

fn operation_label(op: &Operation) -> String {
    match op {
        Operation::Rotate90 => "Rotate 90".to_string(),
        Operation::Rotate180 => "Rotate 180".to_string(),
        Operation::Rotate270 => "Rotate 270".to_string(),
        Operation::FlipHorizontal => "Flip horizontal".to_string(),
        Operation::FlipVertical => "Flip vertical".to_string(),
        Operation::Grayscale => "Grayscale".to_string(),
        Operation::Negative => "Negative".to_string(),
        Operation::Resize {
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

/// Build one [`ConversionJob`] per source file, all sharing the run's target
/// folder, format, quality and operation chain. Pure — no I/O.
fn build_jobs(
    files: &[PathBuf],
    target_dir: &Path,
    format: OutputFormat,
    jpeg_quality: u8,
    operations: &[Operation],
) -> Vec<ConversionJob> {
    files
        .iter()
        .map(|source| ConversionJob {
            source: source.clone(),
            target_dir: target_dir.to_path_buf(),
            format,
            operations: operations.to_vec(),
            jpeg_quality,
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

/// Convert `jobs` on a background thread and report the per-file statuses back
/// over the returned channel. `run_batch` fans the work out across `rayon`'s
/// pool, so the one message arrives once every file is done.
fn spawn_batch(jobs: Vec<ConversionJob>, policy: OverwritePolicy) -> Receiver<BatchMessage> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let statuses = batch::run_batch(&jobs, policy);
        let _ = tx.send(BatchMessage::Finished(statuses));
    });
    rx
}

/// Validate the current state and kick off a run. No-op when preconditions are
/// unmet (no output folder); the Run button is disabled in that case anyway.
fn start_batch(batch: &mut BatchState) {
    let Some(dir) = batch.output_dir.clone() else {
        return;
    };
    let jobs = build_jobs(
        &batch.files,
        &dir,
        batch.format,
        batch.jpeg_quality,
        &batch.operations,
    );
    batch.statuses = vec![None; jobs.len()];
    batch.running = true;
    batch.receiver = Some(spawn_batch(jobs, batch.overwrite));
}

impl PhotonApp {
    /// Show the Batch Convert window when it is open. Called from the toolbar
    /// each frame (mirroring how `ui_settings_window` is driven from `main`).
    pub(crate) fn ui_batch_window(&mut self, ctx: &egui::Context) {
        // Take a local copy of the open flag: the body borrows `self.batch`
        // mutably, which would otherwise conflict with `Window::open`.
        let mut open = self.batch.open;
        egui::Window::new("Batch Convert")
            .default_width(480.0)
            .open(&mut open)
            .show(ctx, |ui| {
                batch_window_body(ui, &mut self.batch, &self.images)
            });
        self.batch.open = open;
    }

    /// Poll the background batch run and swap in its statuses. Called every
    /// frame (like `check_save_status`), so a run finishes even if the window
    /// has been closed; `request_repaint` keeps the spinner and progress bar
    /// animating while it works.
    pub(crate) fn check_batch_status(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = self.batch.receiver.take() {
            match receiver.try_recv() {
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

/// The window body. A free function over the batch state and the app's current
/// file list, so the closure passed to `egui::Window::show` captures disjoint
/// fields of `PhotonApp` rather than the whole app.
fn batch_window_body(ui: &mut egui::Ui, batch: &mut BatchState, images: &[PathBuf]) {
    file_list_section(ui, batch, images);
    ui.separator();
    output_section(ui, batch);
    ui.separator();
    operations_section(ui, batch);
    ui.separator();
    run_section(ui, batch);
}

fn file_list_section(ui: &mut egui::Ui, batch: &mut BatchState, images: &[PathBuf]) {
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
    });

    ui.label(format!("{} file(s)", batch.files.len()));

    let mut remove: Option<usize> = None;
    egui::ScrollArea::vertical()
        .id_salt("batch_file_list")
        .max_height(140.0)
        .show(ui, |ui| {
            for (index, path) in batch.files.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button(icons::X).clicked() {
                        remove = Some(index);
                    }
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    ui.label(name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status = match batch.statuses.get(index) {
                            Some(status) => status,
                            None => &None,
                        };
                        ui.label(status_label(status));
                    });
                });
            }
        });
    if let Some(index) = remove {
        batch.files.remove(index);
        if index < batch.statuses.len() {
            batch.statuses.remove(index);
        }
    }
}

fn output_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    ui.horizontal(|ui| {
        ui.label("Format:");
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
            ui.add(egui::DragValue::new(&mut batch.jpeg_quality).range(1..=100));
        }
    });

    ui.horizontal(|ui| {
        ui.label("If target exists:");
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
    });

    ui.horizontal(|ui| {
        if ui.button("Output folder\u{2026}").clicked() {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                batch.output_dir = Some(dir);
            }
        }
        match &batch.output_dir {
            Some(dir) => {
                ui.label(dir.display().to_string());
            }
            None => {
                ui.label("(none selected)");
            }
        }
    });
}

fn operations_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    ui.label("Operations (applied in order):");

    let mut remove_op: Option<usize> = None;
    if batch.operations.is_empty() {
        ui.label("(no operations)");
    }
    for (index, op) in batch.operations.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.small_button(icons::X).clicked() {
                remove_op = Some(index);
            }
            ui.label(operation_label(op));
        });
    }
    if let Some(index) = remove_op {
        batch.operations.remove(index);
    }

    ui.horizontal(|ui| {
        let choice = OperationChoice::ALL[batch.pending_op.min(OperationChoice::ALL.len() - 1)];
        egui::ComboBox::from_id_salt("batch_op")
            .selected_text(choice.label())
            .show_ui(ui, |ui| {
                for (index, option) in OperationChoice::ALL.iter().enumerate() {
                    ui.selectable_value(&mut batch.pending_op, index, option.label());
                }
            });

        if choice == OperationChoice::Resize {
            ui.label("W:");
            ui.add(egui::DragValue::new(&mut batch.resize_width).range(0..=100_000));
            ui.label("H:");
            ui.add(egui::DragValue::new(&mut batch.resize_height).range(0..=100_000));
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
                        ui.selectable_value(&mut batch.resize_filter, filter, filter_label(filter));
                    }
                });
        }

        if ui.button("Add step").clicked() {
            if let Some(op) =
                choice.to_operation(batch.resize_width, batch.resize_height, batch.resize_filter)
            {
                batch.operations.push(op);
            }
        }
    });
    ui.label("Resize W/H 0 = keep aspect ratio.");
}

fn run_section(ui: &mut egui::Ui, batch: &mut BatchState) {
    let can_run = !batch.running && !batch.files.is_empty() && batch.output_dir.is_some();
    ui.horizontal(|ui| {
        if ui.add_enabled(can_run, egui::Button::new("Run")).clicked() {
            start_batch(batch);
        }
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
    });

    if batch.running {
        // Indeterminate: `run_batch` reports only when the whole list is done.
        ui.add(egui::ProgressBar::new(0.0).animate(true).show_percentage());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_op() -> Operation {
        Operation::Resize {
            width: Some(100),
            height: None,
            filter: ResampleFilter::Lanczos3,
        }
    }

    #[test]
    fn build_jobs_maps_every_file_with_shared_settings() {
        let files = vec![PathBuf::from("/in/a.png"), PathBuf::from("/in/b.jpg")];
        let ops = vec![Operation::Grayscale, sample_op()];
        let jobs = build_jobs(&files, Path::new("/out"), OutputFormat::Jpeg, 80, &ops);

        assert_eq!(jobs.len(), 2);
        for (job, file) in jobs.iter().zip(&files) {
            assert_eq!(&job.source, file);
            assert_eq!(job.target_dir, PathBuf::from("/out"));
            assert_eq!(job.format, OutputFormat::Jpeg);
            assert_eq!(job.jpeg_quality, 80);
            assert_eq!(job.operations, ops);
        }
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
            Some(Operation::Grayscale)
        );
    }
}
