pub mod download;
pub mod page_scan;
pub mod settings;

use tauri::{Emitter, Manager};
use serde::{Serialize, Deserialize};
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

/// Resolve (and create) the effective download output directory from settings.
fn resolve_output_dir() -> PathBuf {
    let s = settings::load().unwrap_or_default();
    let base = s.download_path
        .as_ref()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::video_dir())
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = if s.create_subfolder { base.join("ClipHive") } else { base };
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Find the most recently modified media file in a directory (locates a finished download).
fn latest_media_file(dir: &std::path::Path) -> Option<String> {
    let exts = ["mp4","webm","mkv","mov","avi","mp3","m4a","flac","wav"];
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() { continue; }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            if !exts.contains(&ext.as_str()) { continue; }
            let md = match e.metadata() { Ok(m) => m, Err(_) => continue };
            let mt = match md.modified() { Ok(t) => t, Err(_) => continue };
            match &best {
                Some((t, _)) => { if mt > *t { best = Some((mt, p)); } }
                None => best = Some((mt, p)),
            }
        }
    }
    best.map(|(_, p)| p.to_string_lossy().to_string())
}

// --- Library persistence (completed downloads) ---
fn data_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("cliphive")
}
fn library_path() -> PathBuf { data_dir().join("library.json") }
fn load_library() -> Vec<DownloadItem> {
    let p = library_path();
    if !p.exists() { return vec![]; }
    std::fs::read_to_string(p).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
fn persist_library(items: &[DownloadItem]) {
    let p = library_path();
    if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(json) = serde_json::to_string_pretty(items) { let _ = std::fs::write(p, json); }
}

// --- Shared state ---

struct AppState {
    downloads: Mutex<Vec<DownloadItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub completed_at: Option<i64>,
}

impl Default for DownloadItem {
    fn default() -> Self {
        Self {
            id: String::new(), url: String::new(), title: String::new(),
            quality: String::new(), format: String::new(), status: String::new(),
            progress_pct: 0.0, speed_bps: None, eta_sec: None,
            file_path: None, error: None, thumbnail_url: None, completed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoInfo {
    pub url: String,
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
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; const CREATE_NO_WINDOW: u32 = 0x08000000; cmd.creation_flags(CREATE_NO_WINDOW); }
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
        url: url.clone(),
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
    title: Option<String>,
    thumbnail_url: Option<String>,
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

    // Output to the configured download folder (created if missing)
    let output_dir = resolve_output_dir();
    args.push("-o".into());
    args.push(output_dir.join("%(title)s.%(ext)s").to_string_lossy().to_string());
    args.push("--no-playlist".into());
    // Emit one progress line at a time (newline-terminated) instead of carriage-return updates.
    args.push("--newline".into());
    args.push(url.clone());

    // Add to state as downloading (with title/thumbnail for the UI)
    {
        let mut downloads = state.downloads.lock().unwrap();
        downloads.insert(0, DownloadItem {
            id: id.clone(),
            url: url.clone(),
            title: title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Downloading…".into()),
            quality: quality.clone(),
            format: format.clone(),
            status: "downloading".into(),
            progress_pct: 0.0,
            speed_bps: None,
            eta_sec: None,
            file_path: None,
            error: None,
            thumbnail_url: thumbnail_url.clone(),
            completed_at: None,
        });
    }

    // Spawn yt-dlp in a background task (hidden window on Windows)
    let id_clone = id.clone();
    let app_clone = app_handle.clone();
    let output_dir_clone = output_dir.clone();

    tokio::spawn(async move {
        let ytdlp = resource_path("yt-dlp.exe");
        let mut cmd = Command::new(&ytdlp);
        if let Some(dir) = ffmpeg_dir() {
            let path_env = format!("{};{}", dir.display(), std::env::var("PATH").unwrap_or_default());
            cmd.env("PATH", path_env);
        }
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; const CREATE_NO_WINDOW: u32 = 0x08000000; cmd.creation_flags(CREATE_NO_WINDOW); }

        let mut child = match cmd
            .args(&args)
            .stdout(std::process::Stdio::piped())
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

        // Tell the UI a download has started before the first progress line arrives.
        let _ = app_clone.emit("download-event", serde_json::json!({
            "id": id_clone, "status": "downloading", "progress_pct": 0.0
        }));

        use tokio::io::{AsyncBufReadExt, BufReader};
        use std::time::{Duration, Instant};

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // yt-dlp writes download progress to stdout, not stderr. Read both pipes
        // concurrently so neither can fill up and stall the process.
        let progress_handle = {
            let app = app_clone.clone();
            let id_line = id_clone.clone();
            tokio::spawn(async move {
                if let Some(out) = stdout {
                    let mut lines = BufReader::new(out).lines();
                    let mut last_emit = Instant::now() - Duration::from_secs(1);
                    while let Ok(Some(line)) = lines.next_line().await {
                        if !line.contains("[download]") || !line.contains('%') { continue; }
                        // Keep the UI readable: at most ~8 updates per second.
                        if last_emit.elapsed() < Duration::from_millis(120) { continue; }
                        last_emit = Instant::now();
                        let _ = app.emit("download-event", serde_json::json!({
                            "id": id_line,
                            "status": "downloading",
                            "progress_pct": extract_pct(&line),
                            "speed_bps": extract_speed(&line),
                            "eta_sec": extract_eta(&line),
                        }));
                    }
                }
            })
        };

        let stderr_handle = {
            tokio::spawn(async move {
                let mut err_text = String::new();
                if let Some(err) = stderr {
                    let mut lines = BufReader::new(err).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if err_text.len() < 8000 {
                            err_text.push_str(&line);
                            err_text.push('\n');
                        }
                    }
                }
                err_text
            })
        };

        let ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
        let _ = progress_handle.await;
        let err_text = stderr_handle.await.unwrap_or_default();
        let error_line = err_text
            .lines()
            .rev()
            .find(|l| l.to_lowercase().contains("error"))
            .or_else(|| err_text.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("Download failed")
            .trim()
            .to_string();
        let file_path = if ok { latest_media_file(&output_dir_clone) } else { None };
        let dir_str = output_dir_clone.to_string_lossy().to_string();

        // Update shared state with the final result, then persist completed items
        let state = app_clone.state::<AppState>();
        {
            let mut dl = state.downloads.lock().unwrap();
            if let Some(item) = dl.iter_mut().find(|d| d.id == id_clone) {
                item.status = if ok { "complete".into() } else { "failed".into() };
                item.error = if ok { None } else { Some(error_line.clone()) };
                if ok {
                    item.progress_pct = 100.0;
                    item.file_path = file_path.clone();
                    item.completed_at = Some(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0));
                }
            }
        }

        if ok {
            let completed: Vec<DownloadItem> = state.downloads.lock().unwrap()
                .iter().filter(|d| d.status == "complete").cloned().collect();
            persist_library(&completed);
        }

        let _ = app_clone.emit("download-event", serde_json::json!({
            "id": id_clone,
            "status": if ok { "complete" } else { "failed" },
            "progress_pct": if ok { serde_json::json!(100.0) } else { serde_json::Value::Null },
            "file_path": file_path,
            "dir": dir_str,
            "error": if ok { serde_json::Value::Null } else { serde_json::json!(error_line) }
        }));
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
        if p.is_dir() {
            std::process::Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            std::process::Command::new("explorer")
                .args(["/select,", &path])
                .spawn()
                .map_err(|e| e.to_string())?;
        }
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

/// Open a media file with the system's default player
#[tauri::command]
fn play_file(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("File not found".into());
    }
    open::that(&path).map_err(|e| e.to_string())
}

/// Remove a download from the library and (optionally) delete its media file on disk
#[tauri::command]
fn delete_download(app_handle: tauri::AppHandle, id: String, delete_file: Option<bool>) -> Result<(), String> {
    let remove_file = delete_file.unwrap_or(true);
    let state = app_handle.state::<AppState>();

    // Capture the file path before removing the item
    let file_to_delete = {
        let dl = state.downloads.lock().unwrap();
        dl.iter().find(|d| d.id == id).and_then(|d| d.file_path.clone())
    };

    {
        let mut dl = state.downloads.lock().unwrap();
        dl.retain(|d| d.id != id);
    }

    // Persist the updated library (completed items only)
    let completed: Vec<DownloadItem> = {
        let dl = state.downloads.lock().unwrap();
        dl.iter().filter(|d| d.status == "complete").cloned().collect()
    };
    persist_library(&completed);

    if remove_file {
        if let Some(fp) = file_to_delete {
            let _ = std::fs::remove_file(&fp);
        }
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
    // yt-dlp prints ETA as mm:ss, or h:mm:ss for longer downloads.
    let re = regex::Regex::new(r"ETA\s+(?:(\d+):)?(\d+):(\d+)").unwrap();
    let caps = re.captures(line)?;
    let h: u64 = caps.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
    let min: u64 = caps.get(2)?.as_str().parse().ok()?;
    let sec: u64 = caps.get(3)?.as_str().parse().ok()?;
    Some(h * 3600 + min * 60 + sec)
}

/// Return the running app's version string (shown in Settings → Updates).
#[tauri::command]
fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// --- App entry point ---

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState { downloads: Mutex::new(load_library()) })
        .invoke_handler(tauri::generate_handler![
            analyze_url,
            start_download,
            get_downloads,
            cancel_download,
            reveal_in_folder,
            scan_page,
            get_settings,
            save_settings,
            get_version,
            play_file,
            delete_download,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipHive");
}

#[cfg(test)]
mod tests {
    use super::{extract_pct, extract_speed, extract_eta};

    #[test]
    fn parses_yt_dlp_progress_line() {
        let line = "[download]  45.3% of    1.05MiB at  2.50MiB/s ETA 00:12";
        assert!((extract_pct(line) - 45.3).abs() < 0.001);
        assert_eq!(extract_speed(line), Some(2_621_440));
        assert_eq!(extract_eta(line), Some(12));
    }

    #[test]
    fn handles_unknown_speed_and_eta() {
        let line = "[download]  10.0% of 1.05KiB at  Unknown B/s ETA Unknown";
        assert!((extract_pct(line) - 10.0).abs() < 0.001);
        assert_eq!(extract_speed(line), None);
        assert_eq!(extract_eta(line), None);
    }

    #[test]
    fn handles_hour_long_eta_and_final_line() {
        let long = "[download]  50.0% of 2.00GiB at 1.00MiB/s ETA 01:02:03";
        assert_eq!(extract_eta(long), Some(3723));

        let final_line = "[download] 100% of    1.05KiB in 00:00:00 at 8.68KiB/s";
        assert!((extract_pct(final_line) - 100.0).abs() < 0.001);
        assert_eq!(extract_speed(final_line), Some(8888));
    }
}
