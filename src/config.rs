use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub show_hidden: bool,
    pub last_left_dir: Option<String>,
    pub last_right_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            show_hidden: false,
            last_left_dir: None,
            last_right_dir: None,
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

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(data) = serde_json::to_string_pretty(self) {
            std::fs::write(path, data).ok();
        }
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
