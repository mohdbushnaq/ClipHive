pub mod download;
pub mod page_scan;
pub mod settings;

use tauri::Emitter;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::process::Command;

/// Resolve the path to a bundled resource (yt-dlp.exe, ffmpeg.exe).
/// In release builds, resources are placed next to the executable.
fn resource_path(name: &str) -> PathBuf {
    // Tauri places resources in the same directory as the exe on Windows,
    // or in a "resources" subfolder depending on bundle config.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // Try alongside the exe first (NSIS install puts them there)
    let direct = exe_dir.join(name);
    if direct.exists() {
        return direct;
    }

    // Fallback: in a "resources" subfolder
    let res = exe_dir.join("resources").join(name);
    if res.exists() {
        return res;
    }

    // Last resort: assume it's in PATH
    PathBuf::from(name)
}

/// Get the directory containing ffmpeg (for setting PATH env var)
fn ffmpeg_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()?
        .parent()?.to_path_buf();
    
    // Check alongside exe
    if exe_dir.join("ffmpeg.exe").exists() {
        return Some(exe_dir);
    }
    // Check resources subfolder
    let res = exe_dir.join("resources");
    if res.join("ffmpeg.exe").exists() {
        return Some(res);
    }
    None
}

// --- Shared state ---

struct AppState {
    downloads: Mutex<Vec<DownloadItem>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadItem {
    pub id: String,
    pub url: String,
    pub title: String,
    pub quality: String,
    pub format: String,
    pub status: String, // "queued" | "downloading" | "paused" | "complete" | "failed"
    pub progress_pct: f64,
    pub speed_bps: Option<u64>,
    pub eta_sec: Option<u64>,
    pub file_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoInfo {
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_sec: Option<u64>,
    pub platform: String,
    pub qualities: Vec<QualityOption>,
    pub subtitles: Vec<SubtitleOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityOption {
    pub label: String,
    pub height: u32,
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleOption {
    pub language: String,
    pub auto_generated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScannedVideo {
    pub url: String,
    pub title: Option<String>,
    pub source: String,
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageScanResult {
    pub page_url: String,
    pub page_title: String,
    pub videos: Vec<ScannedVideo>,
}

// --- Tauri Commands (exposed to frontend via window.__TAURI__) ---

/// Analyze a video URL using yt-dlp --dump-json
#[tauri::command]
async fn analyze_url(url: String) -> Result<VideoInfo, String> {
    let ytdlp = resource_path("yt-dlp.exe");
    
    // Add ffmpeg dir to PATH for this process
    let mut cmd = Command::new(&ytdlp);
    if let Some(dir) = ffmpeg_dir() {
        let path_env = format!("{};{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        cmd.env("PATH", path_env);
    }

    let output = cmd
        .args(["--dump-json", "--no-playlist", &url])
        .output()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.lines().next().unwrap_or("yt-dlp failed").to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse yt-dlp output: {}", e))?;

    Ok(VideoInfo {
        title: data["title"].as_str().unwrap_or("Unknown").to_string(),
        thumbnail_url: data["thumbnail"].as_str().map(String::from),
        duration_sec: data["duration"].as_u64(),
        platform: detect_platform(&url),
        qualities: parse_qualities(&data),
        subtitles: parse_subtitles(&data),
    })
}

/// Start a video download via yt-dlp subprocess
#[tauri::command]
async fn start_download(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
    quality: String,
    format: String,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();

    // Build yt-dlp args
    let mut args: Vec<String> = vec![];
    
    if format.starts_with("audio_") {
        args.push("-x".into());
        args.push("--audio-format".into());
        args.push(format.replace("audio_", ""));
        args.push("-f".into());
        args.push("bestaudio/best".into());
    } else if quality == "auto" || quality.is_empty() {
        args.push("-f".into());
        args.push("bestvideo+bestaudio/best".into());
        args.push("--merge-output-format".into());
        args.push(if format == "webm" { "webm" } else { "mp4" }.into());
    } else {
        let h = quality.trim_end_matches('p').parse::<u32>().unwrap_or(1080);
        args.push("-f".into());
        args.push(format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]"));
        args.push("--merge-output-format".into());
        args.push(if format == "webm" { "webm" } else { "mp4" }.into());
    }

    // Output to downloads folder with template
    let download_dir = dirs::download_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ClipHive");
    std::fs::create_dir_all(&download_dir).ok();
    
    args.push("-o".into());
    args.push(download_dir.join("%(title)s.%(ext)s").to_string_lossy().to_string());
    args.push("--no-playlist".into());
    args.push(url.clone());

    // Add to state as queued
    {
        let mut downloads = state.downloads.lock().unwrap();
        downloads.insert(0, DownloadItem {
            id: id.clone(),
            url: url.clone(),
            title: "Downloading…".into(),
            quality: quality.clone(),
            format: format.clone(),
            status: "downloading".into(),
            progress_pct: 0.0,
            speed_bps: None,
            eta_sec: None,
            file_path: None,
            error: None,
        });
    }

    // Spawn yt-dlp in background task
    let id_clone = id.clone();
    let app_clone = app_handle.clone();
    
    tokio::spawn(async move {
        let ytdlp = resource_path("yt-dlp.exe");
        let mut cmd = Command::new(&ytdlp);
        if let Some(dir) = ffmpeg_dir() {
            let path_env = format!("{};{}", dir.display(), std::env::var("PATH").unwrap_or_default());
            cmd.env("PATH", path_env);
        }

        let mut child = match cmd
            .args(&args)
            .stdout(std::process::Stdio::null()) // discard stdout, only need stderr for progress
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = app_clone.emit("download-event", serde_json::json!({
                    "id": id_clone, "status": "failed", "error": e.to_string()
                }));
                return;
            }
        };

        // Read stdout for progress (yt-dlp prints progress to stderr by default)
        if let Some(stderr) = child.stderr.take() {
            use tokio::io::AsyncBufReadExt;
            use tokio::io::BufReader;
            let mut lines = BufReader::new(stderr).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // Parse progress: "[download]  45.2% of ~ 10.5MiB at 2.3MiB/s ETA 00:12"
                if line.contains("[download]") && line.contains("%") {
                    let pct = extract_pct(&line);
                    let speed = extract_speed(&line);
                    let eta = extract_eta(&line);

                    let _ = app_clone.emit("download-event", serde_json::json!({
                        "id": id_clone,
                        "status": "downloading",
                        "progress_pct": pct,
                        "speed_bps": speed,
                        "eta_sec": eta
                    }));
                }
            }
        }

        let status = child.wait().await;
        
        if status.map(|s| s.success()).unwrap_or(false) {
            let _ = app_clone.emit("download-event", serde_json::json!({
                "id": id_clone, "status": "complete", "progress_pct": 100.0
            }));
        } else {
            let _ = app_clone.emit("download-event", serde_json::json!({
                "id": id_clone, "status": "failed", "error": "Download failed"
            }));
        }
    });

    Ok(id)
}

/// Get all downloads (current state)
#[tauri::command]
fn get_downloads(state: tauri::State<'_, AppState>) -> Vec<DownloadItem> {
    state.downloads.lock().unwrap().clone()
}

/// Cancel a download by killing its yt-dlp process
#[tauri::command]
async fn cancel_download(_id: String) -> Result<(), String> {
    // On Windows, we'd track PIDs. For now, use taskkill on yt-dlp processes.
    // A production version would maintain a HashMap<id, ChildHandle>.
    #[cfg(windows)]
    {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/F", "/IM", "yt-dlp.exe"])
            .output()
            .await;
    }
    
    #[cfg(not(windows))]
    {
        let _ = tokio::process::Command::new("pkill")
            .arg("-f")
            .arg(format!("yt-dlp.*{}", id))
            .output()
            .await;
    }

    Ok(())
}

/// Reveal a file in the system file manager
#[tauri::command]
fn reveal_in_folder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("File not found".into());
    }

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        let dir = p.parent().unwrap_or(p);
        std::process::Command::new("xdg-open")
            .arg(dir.to_string_lossy().to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Page Scanner: fetch a URL and extract all video sources from the HTML
#[tauri::command]
async fn scan_page(url: String) -> Result<PageScanResult, String> {
    page_scan::scan(&url).await
}

/// Get app settings
#[tauri::command]
fn get_settings() -> Result<settings::AppSettings, String> {
    settings::load()
}

/// Save app settings
#[tauri::command]
fn save_settings(s: settings::AppSettings) -> Result<(), String> {
    settings::save(&s)
}

// --- Helpers ---

fn detect_platform(url: &str) -> String {
    if url.contains("youtube.com") || url.contains("youtu.be") { "youtube".into() }
    else if url.contains("vimeo.com") { "vimeo".into() }
    else if url.contains("tiktok.com") { "tiktok".into() }
    else if url.contains("facebook.com") || url.contains("fb.watch") { "facebook".into() }
    else if url.contains("dailymotion.com") { "dailymotion".into() }
    else if url.contains("twitter.com") || url.contains("x.com") { "twitter".into() }
    else if url.contains("instagram.com") { "instagram".into() }
    else if url.contains("soundcloud.com") { "soundcloud".into() }
    else { "other".into() }
}

fn parse_qualities(data: &serde_json::Value) -> Vec<QualityOption> {
    let mut seen = std::collections::HashSet::new();
    let mut result = vec![QualityOption { label: "Auto (best)".into(), height: 0, fps: None }];

    if let Some(formats) = data["formats"].as_array() {
        for f in formats {
            let h = f["height"].as_u64().unwrap_or(0) as u32;
            if h == 0 || seen.contains(&h) { continue; }
            if f["vcodec"].as_str() == Some("none") { continue; }
            seen.insert(h);
            let label = if h >= 2160 { format!("4K ({}p)", h) } else { format!("{}p", h) };
            result.push(QualityOption { label, height: h, fps: f["fps"].as_u64().map(|v| v as u32) });
        }
    }

    result.sort_by(|a, b| b.height.cmp(&a.height));
    result
}

fn parse_subtitles(data: &serde_json::Value) -> Vec<SubtitleOption> {
    let mut subs = vec![];
    if let Some(subs_obj) = data["subtitles"].as_object() {
        for (lang, _) in subs_obj {
            subs.push(SubtitleOption {
                language: if lang == "en" { "English".into() } else { lang.to_string() },
                auto_generated: true,
            });
        }
    }
    subs
}

fn extract_pct(line: &str) -> f64 {
    let re = regex::Regex::new(r"(\d+\.?\d*)%").unwrap();
    re.captures(line).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0)
}

fn extract_speed(line: &str) -> Option<u64> {
    let re = regex::Regex::new(r"at\s+([\d.]+)\s*(B|KiB|MiB|GiB)/s").unwrap();
    if let Some(caps) = re.captures(line) {
        let val: f64 = caps.get(1)?.as_str().parse().ok()?;
        let mult: u64 = match caps.get(2)?.as_str() {
            "B" => 1, "KiB" => 1024, "MiB" => 1024*1024, _ => 1024*1024*1024
        };
        Some((val * mult as f64) as u64)
    } else { None }
}

fn extract_eta(line: &str) -> Option<u64> {
    let re = regex::Regex::new(r"ETA\s+(\d+):(\d+)").unwrap();
    if let Some(caps) = re.captures(line) {
        let min: u64 = caps.get(1)?.as_str().parse().ok()?;
        let sec: u64 = caps.get(2)?.as_str().parse().ok()?;
        Some(min * 60 + sec)
    } else { None }
}

// --- App entry point ---

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { downloads: Mutex::new(vec![]) })
        .invoke_handler(tauri::generate_handler![
            analyze_url,
            start_download,
            get_downloads,
            cancel_download,
            reveal_in_folder,
            scan_page,
            get_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipHive");
}
