//! Rendered-output checks (UI-DESIGN §8): draw the tool-window bodies at
//! their real window sizes and save PNGs under `target/snapshots/`. A layout
//! change isn't done when it compiles — re-run this and read the images.

use crate::app::{BatchState, PhotonApp};
use eframe::egui;

const OUT: &str = "target/snapshots";

/// Each harness builds a wgpu context, and the test harness runs the two
/// render tests in parallel threads; two contexts created at once segfault the
/// driver intermittently. Renders serialize on this lock.
static RENDER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Draw `body` for a few frames (so layout settles) and save the render.
fn render(path: &str, size: [f32; 2], mut body: impl FnMut(&mut egui::Ui)) {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::new_ui(move |ui| body(ui));
    harness.set_size(egui::vec2(size[0], size[1]));
    // The real windows carry the app fonts (phosphor icons) and theme; without
    // them the icon glyphs render as tofu and colours cannot be judged.
    crate::app::PhotonApp::load_custom_font(&harness.ctx);
    let defaults = crate::app::AppSettings::default();
    crate::app::PhotonApp::apply_theme(&harness.ctx, defaults.dark_mode, defaults.font_size);
    for _ in 0..3 {
        harness.run();
    }
    let image = harness.render().expect("kittest render");
    let path = std::path::Path::new(OUT).join(path);
    std::fs::create_dir_all(path.parent().unwrap()).expect("create snapshot dir");
    image.save(&path).expect("save snapshot png");
}

/// A batch state with rows that exercise every status icon and two chain
/// steps, so the tables render populated.
fn sample_batch() -> BatchState {
    use crate::app::batch::{FileStatus, OutputFormat};
    use crate::app::image_op::ImageOp;
    let mut batch = BatchState {
        files: vec![
            std::path::PathBuf::from("textures/really_long_texture_name_a.png"),
            std::path::PathBuf::from("b.jpg"),
            std::path::PathBuf::from("c.dds"),
        ],
        format: OutputFormat::Jpeg,
        operations: vec![ImageOp::Grayscale, ImageOp::RotateRight],
        statuses: vec![
            Some(FileStatus::Success),
            Some(FileStatus::Skipped("exists".to_owned())),
            None,
        ],
        ..BatchState::default()
    };
    batch.recipe_text = crate::app::image_op::recipe_text(&batch.operations);
    batch
}

#[test]
fn render_batch_convert() {
    let mut batch = sample_batch();
    // The window's default inner size.
    render("batch_convert_default.png", [640.0, 560.0], |ui| {
        crate::ui::batch_window::render_check_body(ui, &mut batch, &[]);
    });
    render("batch_convert_small.png", [420.0, 300.0], |ui| {
        crate::ui::batch_window::render_check_body(ui, &mut batch, &[]);
    });
    // A size the user actually had: the layout must look intentional large too.
    render("batch_convert_large.png", [873.0, 825.0], |ui| {
        crate::ui::batch_window::render_check_body(ui, &mut batch, &[]);
    });
}

#[test]
fn render_preferences() {
    for (name, page) in [("general", 0usize), ("view", 1), ("shortcuts", 2)] {
        let mut app = PhotonApp::new();
        app.settings_open = true;
        app.preferences_page = page;
        render(&format!("preferences_{name}.png"), [560.0, 430.0], |ui| {
            crate::ui::settings_window::render_check_preferences_body(&mut app, ui);
        });
    }
    // Cramped size, to catch clipping: General and the widest form (View).
    for (name, page) in [("preferences_small.png", 0usize), ("preferences_small_view.png", 1)] {
        let mut app = PhotonApp::new();
        app.settings_open = true;
        app.preferences_page = page;
        render(name, [470.0, 340.0], |ui| {
            crate::ui::settings_window::render_check_preferences_body(&mut app, ui);
        });
    }
    // Wide: the form block centres on its content column instead of filling
    // the pane.
    let mut app = PhotonApp::new();
    app.settings_open = true;
    app.preferences_page = 1;
    render("preferences_wide.png", [760.0, 430.0], |ui| {
        crate::ui::settings_window::render_check_preferences_body(&mut app, ui);
    });
}

/// The bottom info bar with a real-looking image row; the mouse mode sits at
/// the far right, clear of the readouts.
#[test]
fn render_info_bar() {
    let mut app = PhotonApp::new();
    app.images = vec![std::path::PathBuf::from(
        "vacation/very_long_photo_name_dsc_0001.jpg",
    )];
    app.image_dimensions = Some((3840, 2160));
    app.current_color_space = Some(crate::image_loader::ColorSpace::Srgb);
    app.mode = crate::app::MouseMode::Select;
    render("info_bar.png", [900.0, 40.0], |ui| {
        egui::Panel::bottom("info").show_inside(ui, |ui| app.ui_info_bar(ui));
    });
}
