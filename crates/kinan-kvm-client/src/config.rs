//! Global application settings (theme, language).
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// User preferences persisted across launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub language: String,
    pub privacy_warning_seen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: "auto".into(),
            privacy_warning_seen: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
    System,
}

pub fn config_dir() -> PathBuf {
    if let Some(pd) = directories::ProjectDirs::from("io", "kinan-kvm-client", "kinan-kvm-client") {
        pd.config_dir().to_path_buf()
    } else {
        PathBuf::from(".")
    }
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.toml")
}

pub fn load_settings() -> Settings {
    match std::fs::read_to_string(settings_path()) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(s: &Settings) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let body = toml::to_string_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(settings_path(), body)
}
