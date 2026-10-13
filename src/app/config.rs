//! Load and save the user configuration file (`config.ron`).
//!
//! Settings are serialized with serde + RON (`#[serde(default)]` on
//! `AppSettings` means missing fields fall back to defaults; unknown fields
//! are ignored). One unparseable value fails the whole file — we then start
//! from defaults and the next settings change rewrites the file, so nothing
//! is lost beyond the single bad value.

use crate::app::{AppSettings, PhotonApp};
use std::path::PathBuf;

/// Resolve the configuration directory: `$PHOTON_CONFIG_DIR` if set, else the
/// `directories` crate's per-project config dir (XDG config on Linux,
/// `%APPDATA%` on Windows, `~/Library/Application Support` on macOS).
/// `None` when the OS gives us nothing (config disabled).
pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PHOTON_CONFIG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }

    directories::ProjectDirs::from("", "", "Photon").map(|dirs| dirs.config_dir().to_path_buf())
}

pub(crate) fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.ron"))
}

/// Parse settings from RON text. Missing or unknown fields fall back to
/// defaults; a malformed file yields defaults (the caller warns).
pub(crate) fn parse_config(text: &str) -> AppSettings {
    ron::from_str(text).unwrap_or_else(|e| {
        log::warn!("Config: failed to parse config.ron ({}), using defaults", e);
        AppSettings::default()
    })
}

/// Serialize settings to pretty RON.
pub(crate) fn serialize_settings(settings: &AppSettings) -> String {
    ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default()).unwrap_or_else(|e| {
        log::error!(
            "Config: failed to serialize settings ({}), writing defaults",
            e
        );
        ron::ser::to_string_pretty(&AppSettings::default(), ron::ser::PrettyConfig::default())
            .expect("default settings always serialize")
    })
}

impl PhotonApp {
    /// Load `config.ron` at startup, if present and readable.
    pub(crate) fn load_config(&mut self) {
        let Some(path) = config_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        self.settings = parse_config(&text);
        self.saved_settings = Some(self.settings.clone());
    }

    /// Write the config file when settings differ from the last write.
    /// Called every frame; the comparison is trivially cheap. Writes go
    /// through a temp file so a crash can't truncate the config.
    pub(crate) fn save_config_if_changed(&mut self) {
        if self.saved_settings.as_ref() == Some(&self.settings) {
            return;
        }
        self.saved_settings = Some(self.settings.clone());

        let Some(path) = config_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error!("Failed to create config dir {:?}: {}", dir, e);
                return;
            }
        }
        let tmp = path.with_extension("ron.tmp");
        let write = std::fs::write(&tmp, serialize_settings(&self.settings))
            .and_then(|()| std::fs::rename(&tmp, &path));
        if let Err(e) = write {
            log::error!("Failed to write config {:?}: {}", path, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::view::FitMode;
    use crate::app::{SortMode, ZoomReset};

    #[test]
    fn empty_config_yields_defaults() {
        assert_eq!(parse_config(""), AppSettings::default());
    }

    #[test]
    fn roundtrip_preserves_settings() {
        let settings = AppSettings {
            dark_mode: false,
            font_size: 24.0,
            scale_to_fit: false,
            use_sharp_scaling: true,
            checkerboard_bg: false,
            tile_repeat: true,
            zoom_reset: ZoomReset::Never,
            fit_mode: FitMode::AlwaysFit,
            lock_zoom: true,
            exposure: -1.5,
            fit_window: crate::app::view::FitWindow::AlwaysResize,
            sort_mode: SortMode::FileSizeSmallest,
            shortcut_overrides: vec![
                ("next".into(), Some("Ctrl+N".into())),
                ("fullscreen".into(), None),
            ],
        };

        let parsed = parse_config(&serialize_settings(&settings));
        assert_eq!(parsed, settings);
    }

    #[test]
    fn shortcut_overrides_parse_from_raw_config() {
        let parsed = parse_config(
            "(\n    shortcut_overrides: [(\"next\", Some(\"Ctrl+N\")), (\"fullscreen\", None)],\n)",
        );
        assert_eq!(
            parsed.shortcut_overrides,
            vec![
                ("next".to_owned(), Some("Ctrl+N".to_owned())),
                ("fullscreen".to_owned(), None),
            ]
        );
    }

    #[test]
    fn partial_config_fills_defaults() {
        let parsed = parse_config("(\n    dark_mode: false,\n)");
        assert!(!parsed.dark_mode);
        assert_eq!(parsed.zoom_reset, ZoomReset::DimensionChange);
        assert_eq!(parsed.font_size, 16.0);
    }

    #[test]
    fn partial_config_without_fit_mode_defaults_to_reduce_only() {
        let parsed = parse_config("(\n    dark_mode: false,\n)");
        assert_eq!(parsed.fit_mode, FitMode::ReduceOnly);
    }

    #[test]
    fn always_fit_roundtrips_through_ron() {
        let settings = AppSettings {
            fit_mode: FitMode::AlwaysFit,
            ..AppSettings::default()
        };
        let parsed = parse_config(&serialize_settings(&settings));
        assert_eq!(parsed.fit_mode, FitMode::AlwaysFit);
    }

    #[test]
    fn malformed_config_yields_defaults() {
        assert_eq!(
            parse_config("this is not (valid ron"),
            AppSettings::default()
        );
        assert_eq!(
            parse_config("(zoom_reset: sideways)"),
            AppSettings::default()
        );
    }
}
