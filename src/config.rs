use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct RegisteredDir {
    pub key: String, // shortcut key (single uppercase char)
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub show_hidden: bool,
    pub last_left_dir: Option<String>,
    pub last_right_dir: Option<String>,
    #[serde(default)]
    pub drive_dirs: HashMap<String, String>,
    #[serde(default)]
    pub window_x: Option<f32>,
    #[serde(default)]
    pub window_y: Option<f32>,
    #[serde(default)]
    pub window_width: Option<f32>,
    #[serde(default)]
    pub window_height: Option<f32>,
    #[serde(default)]
    pub registered_dirs: Vec<RegisteredDir>,
    #[serde(default)]
    pub cursor_dirs: HashMap<String, String>,
    #[serde(default)]
    pub sort_dirs: HashMap<String, String>,
    /// LRU order of accessed directories (most recent at end).
    #[serde(default)]
    pub dir_access_order: Vec<String>,
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            show_hidden: true,
            last_left_dir: None,
            last_right_dir: None,
            drive_dirs: HashMap::new(),
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            registered_dirs: Vec::new(),
            cursor_dirs: HashMap::new(),
            sort_dirs: HashMap::new(),
            dir_access_order: Vec::new(),
            font_path: None,
            font_size: None,
        }
    }
}

impl Config {
    pub fn config_path() -> std::path::PathBuf {
        let mut path = dirs_config_dir();
        path.push("f2filer");
        std::fs::create_dir_all(&path).ok();
        path.push("config.json");
        path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Config::default()
        }
    }

    const MAX_DIR_HISTORY: usize = 1000;

    /// Record a directory access, moving it to the end (most recent) of the LRU list.
    pub fn touch_dir(&mut self, dir: &str) {
        self.dir_access_order.retain(|d| d != dir);
        self.dir_access_order.push(dir.to_string());
    }

    /// Trim cursor_dirs and sort_dirs to keep only the most recent MAX_DIR_HISTORY entries.
    pub fn trim_dir_history(&mut self) {
        if self.dir_access_order.len() <= Self::MAX_DIR_HISTORY {
            return;
        }
        let remove_count = self.dir_access_order.len() - Self::MAX_DIR_HISTORY;
        let to_remove: std::collections::HashSet<String> =
            self.dir_access_order.drain(..remove_count).collect();
        self.cursor_dirs.retain(|k, _| !to_remove.contains(k));
        self.sort_dirs.retain(|k, _| !to_remove.contains(k));
    }

    pub fn save(&self) {
        let path = Self::config_path();
        let tmp_path = path.with_extension("json.tmp");
        if let Ok(data) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp_path, &data).is_ok() {
                std::fs::rename(&tmp_path, &path).ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let config = Config::default();
        assert!(config.show_hidden);
        assert!(config.last_left_dir.is_none());
        assert!(config.last_right_dir.is_none());
        assert!(config.drive_dirs.is_empty());
        assert!(config.registered_dirs.is_empty());
    }

    #[test]
    fn config_serialize_deserialize() {
        let mut config = Config::default();
        config.show_hidden = true;
        config.last_left_dir = Some("C:\\Users".to_string());
        config.registered_dirs.push(RegisteredDir {
            key: "D".to_string(),
            name: "Downloads".to_string(),
            path: "C:\\Users\\Downloads".to_string(),
        });

        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();

        assert!(restored.show_hidden);
        assert_eq!(restored.last_left_dir, Some("C:\\Users".to_string()));
        assert_eq!(restored.registered_dirs.len(), 1);
        assert_eq!(restored.registered_dirs[0].key, "D");
        assert_eq!(restored.registered_dirs[0].name, "Downloads");
    }

    #[test]
    fn config_deserialize_with_missing_fields() {
        // Simulates loading old config that lacks new fields
        let json = r#"{"show_hidden":false,"last_left_dir":null,"last_right_dir":null}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.drive_dirs.is_empty());
        assert!(config.registered_dirs.is_empty());
        assert!(config.window_x.is_none());
    }
}

fn dirs_config_dir() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return std::path::PathBuf::from(appdata);
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home).join(".config");
        }
    }
    std::path::PathBuf::from(".")
}
