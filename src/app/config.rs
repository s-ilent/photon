//! Load and save the user configuration file (`photon.conf`).
//!
//! Plain `key = value` lines with `#` comments, deliberately hand-parsed:
//! the settings are a small fixed set, so a real TOML/Serde stack isn't worth
//! the dependency weight. Unknown keys and invalid values fall back to the
//! default with a warning on stdout; missing keys always do.

use crate::app::{AppSettings, PhotonApp, SortMode, ZoomReset};
use std::path::PathBuf;

/// Resolve the configuration directory: `$PHOTON_CONFIG_DIR`, then
/// `$XDG_CONFIG_HOME/photon`, then `$HOME/.config/photon`.
/// `None` when no usable base directory is set (config disabled).
pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PHOTON_CONFIG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }

    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var("HOME").ok()?).join(".config"),
    };
    Some(base.join("photon"))
}

pub(crate) fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("photon.conf"))
}

/// Parse settings from config text. Missing, unknown, or invalid entries keep
/// their default value.
pub(crate) fn parse_config(text: &str) -> AppSettings {
    let mut settings = AppSettings::default();

    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || value.is_empty() {
            continue;
        }

        let mut invalid = false;
        match key {
            "dark_mode" => match value.parse() {
                Ok(v) => settings.dark_mode = v,
                Err(_) => invalid = true,
            },
            "font_size" => match value.parse::<f32>() {
                Ok(v) if (6.0..=96.0).contains(&v) => settings.font_size = v,
                _ => invalid = true,
            },
            "scale_to_fit" => match value.parse() {
                Ok(v) => settings.scale_to_fit = v,
                Err(_) => invalid = true,
            },
            "use_sharp_scaling" => match value.parse() {
                Ok(v) => settings.use_sharp_scaling = v,
                Err(_) => invalid = true,
            },
            "checkerboard_bg" => match value.parse() {
                Ok(v) => settings.checkerboard_bg = v,
                Err(_) => invalid = true,
            },
            "zoom_reset" => match ZoomReset::from_config(value) {
                Some(v) => settings.zoom_reset = v,
                None => invalid = true,
            },
            "sort_mode" => match SortMode::from_config(value) {
                Some(v) => settings.sort_mode = v,
                None => invalid = true,
            },
            other => {
                println!("Config: ignoring unknown key {:?}", other);
                continue;
            }
        }
        if invalid {
            println!("Config: invalid value {:?} for key {:?}, using default", value, key);
        }
    }

    settings
}

/// Serialize settings to config-file text.
pub(crate) fn serialize_settings(settings: &AppSettings) -> String {
    format!(
        "# Photon configuration\n\
         # One \"key = value\" per line; unknown keys are ignored on load.\n\
         dark_mode = {}\n\
         font_size = {}\n\
         scale_to_fit = {}\n\
         use_sharp_scaling = {}\n\
         checkerboard_bg = {}\n\
         zoom_reset = {}\n\
         sort_mode = {}\n",
        settings.dark_mode,
        settings.font_size,
        settings.scale_to_fit,
        settings.use_sharp_scaling,
        settings.checkerboard_bg,
        settings.zoom_reset.config_value(),
        settings.sort_mode.config_value(),
    )
}

impl PhotonApp {
    /// Load `photon.conf` at startup, if present and readable.
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
                println!("Failed to create config dir {:?}: {}", dir, e);
                return;
            }
        }
        let tmp = path.with_extension("conf.tmp");
        let write =
            std::fs::write(&tmp, serialize_settings(&self.settings)).and_then(|()| {
                std::fs::rename(&tmp, &path)
            });
        if let Err(e) = write {
            println!("Failed to write config {:?}: {}", path, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_yields_defaults() {
        assert_eq!(parse_config(""), AppSettings::default());
        assert_eq!(parse_config("# only a comment\n\n"), AppSettings::default());
    }

    #[test]
    fn roundtrip_preserves_settings() {
        let settings = AppSettings {
            dark_mode: false,
            font_size: 24.0,
            scale_to_fit: false,
            use_sharp_scaling: true,
            checkerboard_bg: false,
            zoom_reset: ZoomReset::Never,
            sort_mode: SortMode::FileSizeSmallest,
        };

        let parsed = parse_config(&serialize_settings(&settings));
        assert_eq!(parsed, settings);
    }

    #[test]
    fn overrides_apply_and_unknown_keys_are_ignored() {
        let text = "\
            # comment\n\
            dark_mode = false\n\
            zoom_reset = never\n\
            sort_mode = name_za\n\
            font_size = 31.5\n\
            future_feature = enabled\n\
            garbage line without equals\n";
        let parsed = parse_config(text);
        assert!(!parsed.dark_mode);
        assert_eq!(parsed.zoom_reset, ZoomReset::Never);
        assert_eq!(parsed.sort_mode, SortMode::AlphabeticalDesc);
        assert_eq!(parsed.font_size, 31.5);
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let text = "zoom_reset = sideways\nsort_mode = random\nfont_size = 1000\n";
        let parsed = parse_config(text);
        assert_eq!(parsed, AppSettings::default());
    }
}
