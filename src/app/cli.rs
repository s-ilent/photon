//! The chainable command line (Roadmap.md §10): argv parsing into a launch
//! description, the usage text, and the per-frame consumption of the launch
//! once the first image is displayed.
//!
//! Op switches serialise through the same [`ImageOp`] text form as batch
//! recipes (`image_op::from_cli`), so the command line and the Convert window
//! cannot drift apart.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::app::image_op::ImageOp;
use crate::app::PhotonApp;

/// Output extensions `--save` accepts; the encode itself is extension-driven
/// (`saver::encode_to_path`).
const SAVE_EXTENSIONS: &[&str] = &["png", "jpeg", "jpg", "webp", "bmp", "tiff"];

/// What `parse_args` decided the process should do.
pub(crate) enum Parsed {
    Help,
    Launch(CliLaunch),
}

/// A resolved command line: where to open, what to apply and where to save.
pub(crate) struct CliLaunch {
    /// Every non-switch argument, expanded, in order.
    pub(crate) paths: Vec<PathBuf>,
    /// The op chain, left to right.
    pub(crate) ops: Vec<ImageOp>,
    /// `--save` target.
    pub(crate) save_to: Option<PathBuf>,
    /// `--settings`.
    pub(crate) open_settings: bool,
}

/// Parse command-line arguments (without the program name).
///
/// Switches take their value either as the next argument (`--rotate right`) or
/// joined with `=` (`--rotate=right`); no-argument switches (`--grayscale`,
/// `--settings`, …) reject both spellings of a value. `--` ends switch
/// parsing; a bare `-` is a path. Paths and the `--save` value get `~` and
/// `$VAR`/`${VAR}` expansion.
pub(crate) fn parse_args(args: &[OsString]) -> Result<Parsed, String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut ops: Vec<ImageOp> = Vec::new();
    let mut save_to: Option<PathBuf> = None;
    let mut open_settings = false;
    let mut paths_only = false;

    let mut i = 0;
    while i < args.len() {
        let token = &args[i];
        i += 1;
        let text = token.to_string_lossy();

        if paths_only || !text.starts_with('-') || text == "-" {
            paths.push(expand_path(token)?);
            continue;
        }
        if text == "--" {
            paths_only = true;
            continue;
        }
        if text == "-h" {
            return Ok(Parsed::Help);
        }

        let Some(rest) = text.strip_prefix("--") else {
            return Err(format!("unknown switch {text:?}"));
        };
        let (name, inline) = match rest.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (rest, None),
        };

        match name {
            "help" => {
                if let Some(value) = inline {
                    return Err(format!("--help takes no argument (got {value:?})"));
                }
                return Ok(Parsed::Help);
            }
            "settings" => {
                if let Some(value) = inline {
                    return Err(format!("--settings takes no argument (got {value:?})"));
                }
                open_settings = true;
            }
            "save" => {
                if save_to.is_some() {
                    return Err("--save given more than once".to_string());
                }
                let value = switch_value(inline, args, &mut i)
                    .ok_or_else(|| "--save needs an output file path".to_string())?;
                let path = expand_path(OsStr::new(value))?;
                validate_save_path(&path)?;
                save_to = Some(path);
            }
            "flip" | "mirror" | "grayscale" | "negative" => {
                // No-argument ops never consume the next token; a joined value
                // is rejected through the shared parser.
                ops.push(ImageOp::from_cli(&switch_text(name, inline))?);
            }
            "rotate" | "resize" | "filter" | "crop" => {
                let value = switch_value(inline, args, &mut i);
                ops.push(ImageOp::from_cli(&switch_text(name, value))?);
            }
            other => return Err(format!("unknown switch --{other}")),
        }
    }

    if paths.len() > 1 && paths.iter().any(|p| p.is_dir()) {
        return Err(
            "pass either one folder or a list of files (a folder mixed with files is not supported)"
                .to_string(),
        );
    }

    Ok(Parsed::Launch(CliLaunch {
        paths,
        ops,
        save_to,
        open_settings,
    }))
}

/// The text handed to `ImageOp::from_cli` for one switch: name and optional
/// value, joined with a space (the parser also accepts `=`).
fn switch_text(name: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("--{name} {value}"),
        None => format!("--{name}"),
    }
}

/// The value of a value-taking switch: the text after `=`, or the next
/// argument unless it is itself a switch. `None` means no value is available.
/// On the next-argument form `*i` is advanced past the consumed value.
fn switch_value<'a>(
    inline: Option<&'a str>,
    args: &'a [OsString],
    i: &mut usize,
) -> Option<&'a str> {
    if let Some(value) = inline {
        return Some(value);
    }
    match args.get(*i) {
        Some(next) if !next.to_string_lossy().starts_with('-') => {
            *i += 1;
            next.to_str()
        }
        _ => None,
    }
}

/// Expand `~` and `$VAR`/`${VAR}` in a path argument.
fn expand_path(token: &OsStr) -> Result<PathBuf, String> {
    let text = token.to_string_lossy();
    let expanded = shellexpand::full(&text).map_err(|e| format!("cannot expand {text:?}: {e}"))?;
    Ok(PathBuf::from(expanded.as_ref()))
}

/// `--save` pre-flight: the parent directory must exist and the extension must
/// name a format Photon can encode, so a bad output path exits before the GUI
/// opens. Writing onto the input file is allowed — the user named it.
fn validate_save_path(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    if !parent.is_dir() {
        return Err(format!(
            "--save: directory {} does not exist",
            parent.display()
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !SAVE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!(
            "--save: unsupported output extension {ext:?} (use png, jpeg, jpg, webp, bmp or tiff)"
        ));
    }
    Ok(())
}

/// The `--help` text: usage line, switch list, semantics.
pub(crate) fn usage() -> &'static str {
    "Usage: photon [SWITCH]... [FILE|FOLDER]...

Op switches are applied left to right and chain. `--opt value` and
`--opt=value` are equivalent; `--` ends switch parsing. A leading `~` and
$VAR / ${VAR} are expanded in file paths.

  --rotate left|right|180   Rotate the image
  --flip                    Flip vertically
  --mirror                  Flip horizontally
  --grayscale               Convert to grayscale
  --negative                Invert the colours
  --resize WxH[:FILTER]     Resize; 0 keeps the aspect ratio (1280x0).
                            FILTER: lanczos3 (default), nearest, triangle,
                            catmullrom, gaussian
  --filter NAME             Convolution filter: blur, soften, sharpen,
                            gaussian3, gaussian5, emboss, edge
  --crop L,T,R,B            Trim pixel margins left, top, right, bottom
  --save PATH               Write the transformed first image to PATH
                            (extensions: png, jpeg, jpg, webp, bmp, tiff)
  --settings                Open the Settings window on launch
  --help, -h                Show this help and exit

Notes:
- The op switches are transient viewer edits: nothing is written unless
  --save names a file, and the edits are discarded when you navigate away.
- --save writes the transformed first image to the exact path given; the
  input file is never touched unless it is named as the output.
- Two or more file arguments form a temporary playlist in argument order (a
  folder mixed with files is an error). A single file opens together with
  its folder, as usual.
"
}

impl PhotonApp {
    /// Consume the command-line launch once the first image is on screen
    /// (Roadmap §10): apply the op chain, then write the `--save` target.
    /// Called each frame from the poll block, after `check_image_info`.
    pub(crate) fn check_cli_launch(&mut self, ctx: &egui::Context) {
        if self.cli_ops.is_none() && self.cli_save_to.is_none() {
            return;
        }
        // Wait until the first image is displayed (or has failed).
        if self.is_loading_dir || self.is_loading {
            return;
        }
        if self.error_overlay.is_some() {
            log::warn!("Dropping command-line ops/save: the first image failed to load");
            self.cli_ops = None;
            self.cli_save_to = None;
            return;
        }
        if let Some(ops) = self.cli_ops.take() {
            self.apply_edit(&ops);
        }
        if let Some(path) = self.cli_save_to.take() {
            self.save_to_path(path);
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::image_op;
    use std::sync::Arc;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn launch(items: &[&str]) -> CliLaunch {
        match parse_args(&args(items)).expect("parse should succeed") {
            Parsed::Help => panic!("expected a launch, got Help"),
            Parsed::Launch(l) => l,
        }
    }

    fn resize(width: Option<u32>, height: Option<u32>) -> ImageOp {
        ImageOp::Resize {
            width,
            height,
            filter: image_op::ResampleFilter::Lanczos3,
        }
    }

    #[test]
    fn interleaved_switches_and_paths_keep_their_order() {
        let l = launch(&["--grayscale", "a.png", "--rotate", "right", "b.png"]);
        assert_eq!(l.ops, vec![ImageOp::Grayscale, ImageOp::RotateRight]);
        assert_eq!(
            l.paths,
            vec![PathBuf::from("a.png"), PathBuf::from("b.png")]
        );
    }

    #[test]
    fn switch_and_value_may_be_joined_with_equals() {
        let l = launch(&["--rotate=right", "--resize=800x600", "a.png"]);
        assert_eq!(
            l.ops,
            vec![ImageOp::RotateRight, resize(Some(800), Some(600))]
        );
        assert_eq!(l.paths, vec![PathBuf::from("a.png")]);
    }

    #[test]
    fn no_argument_switches_take_no_value() {
        let l = launch(&["--flip", "--mirror", "--grayscale", "--negative", "a.png"]);
        assert_eq!(
            l.ops,
            vec![
                ImageOp::FlipVertical,
                ImageOp::FlipHorizontal,
                ImageOp::Grayscale,
                ImageOp::Negative,
            ]
        );
        assert_eq!(l.paths, vec![PathBuf::from("a.png")]);
    }

    #[test]
    fn double_dash_ends_switch_parsing_and_dash_is_a_path() {
        let l = launch(&["--", "--grayscale", "-", "a.png"]);
        assert!(l.ops.is_empty());
        assert_eq!(
            l.paths,
            vec![
                PathBuf::from("--grayscale"),
                PathBuf::from("-"),
                PathBuf::from("a.png")
            ]
        );
    }

    #[test]
    fn single_dash_values_are_not_consumed_from_the_next_token() {
        // The value must not swallow a following switch.
        assert!(parse_args(&args(&["--rotate", "--grayscale"])).is_err());
        assert!(parse_args(&args(&["--rotate"])).is_err());
        assert!(parse_args(&args(&["--save"])).is_err());
    }

    #[test]
    fn save_may_only_be_given_once() {
        let tmp = std::env::temp_dir();
        let out = tmp.join("photon_cli_test_out.png");
        assert!(parse_args(&args(&[
            "--save",
            out.to_str().unwrap(),
            "--save",
            out.to_str().unwrap()
        ]))
        .err()
        .is_some_and(|e| e.contains("more than once")));
    }

    #[test]
    fn save_preflight_rejects_a_missing_directory() {
        let missing = std::env::temp_dir()
            .join("photon_no_such_dir_98123749")
            .join("out.png");
        assert!(parse_args(&args(&["--save", missing.to_str().unwrap()]))
            .err()
            .is_some_and(|e| e.contains("does not exist")));
    }

    #[test]
    fn save_preflight_rejects_an_unsupported_extension() {
        let out = std::env::temp_dir().join("photon_cli_test_out.gif");
        assert!(parse_args(&args(&["--save", out.to_str().unwrap()]))
            .err()
            .is_some_and(|e| e.contains("unsupported output extension")));
    }

    #[test]
    fn save_accepts_a_bare_filename_and_all_listed_extensions() {
        for ext in ["png", "jpeg", "jpg", "webp", "bmp", "tiff"] {
            let out = format!("out.{ext}");
            match parse_args(&args(&["--save", &out])) {
                Ok(Parsed::Launch(l)) => {
                    assert_eq!(l.save_to, Some(PathBuf::from(out.clone())));
                }
                _ => panic!("--save {out} should parse"),
            }
        }
    }

    #[test]
    fn a_folder_mixed_with_files_is_an_error() {
        let dir = std::env::temp_dir();
        let err = parse_args(&args(&[dir.to_str().unwrap(), "a.png"]))
            .err()
            .expect("mixed folder and files should be an error");
        assert!(err.contains("either one folder or a list of files"));
    }

    #[test]
    fn a_single_folder_or_file_is_not_an_error() {
        let dir = std::env::temp_dir();
        assert!(parse_args(&args(&[dir.to_str().unwrap()])).is_ok());
        assert!(parse_args(&args(&["a.png"])).is_ok());
    }

    #[test]
    fn settings_flag_opens_the_settings_window() {
        assert!(!launch(&["a.png"]).open_settings);
        assert!(launch(&["--settings"]).open_settings);
    }

    #[test]
    fn help_has_a_long_and_a_short_form() {
        assert!(matches!(parse_args(&args(&["--help"])), Ok(Parsed::Help)));
        assert!(matches!(parse_args(&args(&["-h"])), Ok(Parsed::Help)));
    }

    #[test]
    fn unknown_switches_are_rejected_with_the_switch_name() {
        assert!(parse_args(&args(&["--nope"])).is_err());
        assert!(parse_args(&args(&["-x"])).is_err());
        // Op errors propagate from the shared parser.
        assert!(parse_args(&args(&["--rotate", "45"])).is_err());
        assert!(parse_args(&args(&["--filter", "tint"])).is_err());
    }

    #[test]
    fn path_arguments_expand_environment_variables() {
        std::env::set_var("PHOTON_TEST_VAR", "photon-expanded");
        let l = launch(&["$PHOTON_TEST_VAR/a.png", "${PHOTON_TEST_VAR}/b.png"]);
        assert_eq!(
            l.paths,
            vec![
                PathBuf::from("photon-expanded/a.png"),
                PathBuf::from("photon-expanded/b.png")
            ]
        );
    }

    #[test]
    fn save_value_expands_environment_variables() {
        // A distinct variable from the path test (tests run in parallel), and
        // an existing directory so the pre-flight passes.
        std::env::set_var("PHOTON_TEST_SAVE_VAR", std::env::temp_dir());
        let spec = "${PHOTON_TEST_SAVE_VAR}/photon_cli_expanded_out.png";
        match parse_args(&args(&["--save", spec])) {
            Ok(Parsed::Launch(l)) => {
                assert_eq!(
                    l.save_to,
                    Some(std::env::temp_dir().join("photon_cli_expanded_out.png"))
                );
            }
            _ => panic!("expansion should have succeeded"),
        }
    }

    #[test]
    fn a_failed_expansion_is_an_error() {
        std::env::remove_var("PHOTON_TEST_UNSET_VAR_98123");
        assert!(parse_args(&args(&["${PHOTON_TEST_UNSET_VAR_98123}/a.png"]))
            .err()
            .is_some_and(|e| e.contains("cannot expand")));
    }

    /// App with a two-image playlist whose first image is cached and decoded,
    /// so `apply_edit` and `save_to_path` have pixels to work on without a
    /// GPU or disk file (mirrors the loader tests' `cached_app` pattern).
    fn cli_app() -> PhotonApp {
        let mut app = PhotonApp::new();
        app.load_playlist(vec![PathBuf::from("a.png"), PathBuf::from("b.png")]);
        // The playlist load cleared the cache; fill it for the displayed
        // target so `request_current` takes the cached path without a worker.
        for name in ["a.png", "b.png"] {
            app.texture_cache.put(
                PathBuf::from(name),
                Arc::new(crate::render::RenderImage {
                    width: 8,
                    height: 8,
                    is_cubemap: false,
                    color_space: crate::image_loader::ColorSpace::Srgb,
                    hdr: false,
                    layers: vec![crate::render::Layer { tiles: Vec::new() }],
                }),
            );
        }
        app.request_current();
        app.active_rgba = Some(vec![vec![100; 8 * 8 * 4]]);
        app
    }

    #[test]
    fn cli_ops_and_save_apply_once_the_first_image_is_displayed() {
        let mut app = cli_app();
        app.cli_ops = Some(vec![ImageOp::Grayscale]);
        app.cli_save_to = Some(std::env::temp_dir().join("photon_cli_seq_out.png"));

        let ctx = egui::Context::default();

        // Still loading: nothing is consumed yet.
        app.is_loading = true;
        app.check_cli_launch(&ctx);
        assert!(app.cli_ops.is_some());
        assert!(app.cli_save_to.is_some());

        // Image displayed: the chain is applied (one undo step) and the save
        // encode is started through the status channel.
        app.is_loading = false;
        app.check_cli_launch(&ctx);
        assert!(app.cli_ops.is_none());
        assert!(app.cli_save_to.is_none());
        assert!(
            !app.edit_undo.is_empty(),
            "the chain landed in the undo stack"
        );
        let rx = app
            .save_status_receiver
            .take()
            .expect("the background save should have been started");
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("the save should complete")
            .expect("the save should succeed");
        assert!(std::env::temp_dir().join("photon_cli_seq_out.png").exists());
        let _ = std::fs::remove_file(std::env::temp_dir().join("photon_cli_seq_out.png"));
    }

    #[test]
    fn cli_ops_are_dropped_when_the_first_image_fails_to_load() {
        let mut app = cli_app();
        app.cli_ops = Some(vec![ImageOp::Grayscale]);
        app.cli_save_to = Some(PathBuf::from("out.png"));
        app.error_overlay = Some(crate::app::StatusOverlay::Error("boom".to_string()));

        app.check_cli_launch(&egui::Context::default());
        assert!(app.cli_ops.is_none());
        assert!(app.cli_save_to.is_none());
        assert!(app.save_status_receiver.is_none(), "no save was started");
    }
}
