use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub default_quality: String,
    pub default_format: String, // "mp4" | "webm" | "audio_mp3" | "audio_m4a"
    pub max_concurrent: usize,
    pub speed_limit_mbps: u32,  // 0 = unlimited
    pub proxy_url: Option<String>,
    pub theme: String,          // "dark" | "light" | "system"
    pub filename_pattern: String,
    /// Base folder where downloads are saved (default = the user's Videos folder).
    pub download_path: Option<String>,
    /// When true, files go into <download_path>/ClipHive ; when false, directly in <download_path>.
    pub create_subfolder: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_quality: "auto".into(),
            default_format: "mp4".into(),
            max_concurrent: 3,
            speed_limit_mbps: 0,
            proxy_url: None,
            theme: "dark".into(),
            filename_pattern: "{title}_{quality}".into(),
            download_path: dirs::video_dir().map(|p| p.to_string_lossy().to_string()),
            create_subfolder: true,
        }
    }
}

fn settings_path() -> PathBuf {
    let dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.join("cliphive").join("settings.json")
}

pub fn load() -> Result<AppSettings, String> {
    let path = settings_path();
    if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| format!("Corrupt settings: {}", e))
    } else {
        Ok(AppSettings::default())
    }
}

pub fn save(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}
