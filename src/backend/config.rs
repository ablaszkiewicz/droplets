use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub github_ssh_key_path: Option<String>,
    pub do_api_key: Option<String>,
    pub droplet_ssh_key_path: Option<String>,
    pub do_ssh_key_id: Option<i64>,
}

pub fn config_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".droplets");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn load() -> AppConfig {
    let path = config_dir().join("config.json");
    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cfg: &AppConfig) {
    let path = config_dir().join("config.json");
    if let Ok(data) = serde_json::to_string_pretty(cfg) {
        if fs::write(&path, data.as_bytes()).is_ok() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).ok();
        }
    }
}
