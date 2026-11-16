//! App settings, stored as JSON. With `FOLIO_LIBRARY` (test runs) they live inside that library, so a
//! test never touches the real settings.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub appearance: String,
    pub accent_hex: String,
    pub editor_font: String,
    pub editor_font_size: f32,
    pub line_spacing: f32,
    pub editor_width: f32,
    pub code_font: String,
    pub code_font_size: f32,
    pub code_theme_light: String,
    pub code_theme_dark: String,
    pub show_line_numbers: bool,
    pub floating_style: String,
    pub corner_radius: f32,
    pub run_timeout: f64,
    pub interpreter_overrides: BTreeMap<String, String>,
    pub library_path: String,
    pub ai_provider: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub max_output_kb: f64,
    pub trash_retention_days: i64,
    pub backups_enabled: bool,
    pub backup_interval_hours: f64,
    pub backup_folder: String,
    pub daily_folder: String,
    pub daily_template: String,
    pub heading_after_title: bool,
    pub menu_bar_icon: bool,
    pub capture_hot_key: String,
    pub shortcuts: BTreeMap<String, String>,
    pub saved_connections: Vec<serde_json::Value>,
    pub animations: bool,
    pub spellcheck: bool,
    pub include_subfolders: bool,
    pub search_everywhere: bool,
    pub last_backup: f64,
    pub recent: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        Settings {
            appearance: "system".into(),
            accent_hex: "#E5A50A".into(),
            editor_font: "System".into(),
            editor_font_size: 15.0,
            line_spacing: 4.0,
            editor_width: 760.0,
            code_font: "SF Mono".into(),
            code_font_size: 13.0,
            code_theme_light: "xcode-light".into(),
            code_theme_dark: "xcode-dark".into(),
            show_line_numbers: true,
            floating_style: "glass".into(),
            corner_radius: 16.0,
            run_timeout: 30.0,
            interpreter_overrides: BTreeMap::new(),
            library_path: format!("{home}/Documents/Folio"),
            ai_provider: "off".into(),
            ollama_url: "http://localhost:11434".into(),
            ollama_model: String::new(),
            max_output_kb: 1000.0,
            trash_retention_days: 30,
            backups_enabled: true,
            backup_interval_hours: 24.0,
            backup_folder: String::new(),
            daily_folder: "Tagesnotizen".into(),
            daily_template: String::new(),
            heading_after_title: true,
            menu_bar_icon: false,
            capture_hot_key: String::new(),
            shortcuts: BTreeMap::new(),
            saved_connections: vec![],
            animations: true,
            spellcheck: true,
            include_subfolders: false,
            search_everywhere: true,
            last_backup: 0.0,
            recent: vec![],
        }
    }
}

pub fn app_support() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/Folio Rust")
}

/// Where settings are kept: `FOLIO_SETTINGS`, else inside a test library, else Application Support.
pub fn settings_path() -> PathBuf {
    if let Ok(p) = std::env::var("FOLIO_SETTINGS") {
        return PathBuf::from(p);
    }
    if let Ok(lib) = std::env::var("FOLIO_LIBRARY") {
        return PathBuf::from(lib).join(".folio/rust-settings.json");
    }
    app_support().join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings = std::fs::read(settings_path()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        if let Ok(lib) = std::env::var("FOLIO_LIBRARY") {
            s.library_path = lib;
        }
        s
    }

    pub fn save(&self) {
        let p = settings_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = folio_core::model::write_atomic(&p, &serde_json::to_vec_pretty(self).unwrap());
    }

    pub fn write_options(&self) -> folio_core::rtf::WriteOptions {
        folio_core::rtf::WriteOptions {
            body_size: self.editor_font_size,
            body_font: match self.editor_font.as_str() {
                "System" | "" => ".AppleSystemUIFont".into(),
                "New York" => ".NewYork-Regular".into(),
                other => other.into(),
            },
            mono_font: "Menlo-Regular".into(),
        }
    }

    pub fn backup_dir(&self) -> PathBuf {
        if !self.backup_folder.trim().is_empty() {
            return PathBuf::from(self.backup_folder.trim());
        }
        if std::env::var("FOLIO_LIBRARY").is_ok() {
            return PathBuf::from(&self.library_path).join(".folio/backups");
        }
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join("Library/Application Support/Folio/Backups")
    }
}
