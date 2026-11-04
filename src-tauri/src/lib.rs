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

/// Find a bundled resource next to the executable (or in its resources/ folder).
fn find_resource(name: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))?;
    let direct = exe_dir.join(name);
    if direct.exists() { return Some(direct); }
    let res = exe_dir.join("resources").join(name);
    if res.exists() { return Some(res); }
    None
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
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
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

/// Build a yt-dlp command with the bundled tools available on PATH and no console window.
fn ytdlp_command() -> Command {
    let ytdlp = resource_path("yt-dlp.exe");
    let mut cmd = Command::new(&ytdlp);

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut tool_dirs: Vec<String> = Vec::new();
    if let Some(dir) = ffmpeg_dir() {
        tool_dirs.push(dir.display().to_string());
    }
    if let Some(p) = find_resource("aria2c.exe") {
        if let Some(dir) = p.parent() {
            tool_dirs.push(dir.display().to_string());
        }
    }
    if !tool_dirs.is_empty() {
        let path_env = format!(
            "{};{}",
            tool_dirs.join(";"),
            std::env::var("PATH").unwrap_or_default()
        );
        cmd.env("PATH", path_env);
    }
    cmd
}

/// Read a video's title and thumbnail without downloading the media.
/// Used to fill in cards for scans/batch jobs that only have a bare URL.
async fn probe_media(url: &str) -> (Option<String>, Option<String>) {
    let mut cmd = ytdlp_command();
    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        cmd.args(["--dump-json", "--no-playlist", url]).output(),
    )
    .await
    {
        Ok(Ok(o)) => o,
        _ => return (None, None),
    };
    if !output.status.success() {
        return (None, None);
    }
    let data: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let title = data["title"].as_str().map(String::from);
    let thumb = data["thumbnail"].as_str().map(String::from);
    (title, thumb)
}

/// Build the yt-dlp argument list for one download attempt.
fn build_download_args(
    url: &str,
    quality: &str,
    format: &str,
    conns: usize,
    use_aria2c: bool,
    template: &str,
) -> Vec<String> {
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

    // Filenames: strip characters Windows rejects and cap their length.
    // This is what stops "[Errno 22] Invalid argument" on long/complex titles.
    args.push("--windows-filenames".into());
    args.push("--trim-filenames".into());
    args.push("120".into());
    args.push("--no-mtime".into());
    args.push("--no-playlist".into());
    args.push("--newline".into());

    // Fragmented streams (DASH/HLS): fetch several fragments at once.
    args.push("--concurrent-fragments".into());
    args.push(conns.to_string());

    // Regular files: aria2c splits the file and fetches pieces in parallel.
    if use_aria2c && find_resource("aria2c.exe").is_some() {
        args.push("--downloader".into());
        args.push("aria2c".into());
        args.push("--downloader-args".into());
        args.push(format!(
            "aria2c:-x{} -s{} -k1M --summary-interval=1 --console-log-level=warn --file-allocation=none",
            conns, conns
        ));
    }

    args.push("-o".into());
    args.push(template.to_string());
    args.push(url.into());
    args
}

/// Turn a raw yt-dlp error into something a user can act on.
fn friendly_download_error(raw: &str) -> String {
    let low = raw.to_lowercase();
    if low.contains("errno 22") || low.contains("unable to open for writing") {
        return "Couldn't write the file — the name or save folder is invalid. Try a different save folder.".into();
    }
    if low.contains("conversion failed") || low.contains("no space left") || low.contains("not enough space") {
        return "Not enough free disk space to finish this download. A 4K/HDR video needs roughly twice its size free while it is merged.".into();
    }
    if low.contains("http error 403") || low.contains("forbidden") {
        return "The video server refused the download (HTTP 403). Try again or pick a lower quality.".into();
    }
    if low.contains("unsupported url") || low.contains("not a valid url") {
        return "That URL isn't a supported video page.".into();
    }
    if low.contains("private") || low.contains("sign in") || low.contains("confirm your age") {
        return "This video is private, age-restricted, or requires sign-in.".into();
    }
    if low.contains("aria2c exited") {
        return "The multi-connection downloader failed. ClipHive will retry with the standard downloader.".into();
    }
    raw.to_string()
}

/// Run yt-dlp for one attempt, streaming progress to the UI.
async fn run_ytdlp(app: &tauri::AppHandle, id: &str, args: &[String]) -> (bool, String) {
    let mut cmd = ytdlp_command();
    let mut child = match cmd
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (false, format!("Failed to run yt-dlp: {}", e)),
    };

    let _ = app.emit(
        "download-event",
        serde_json::json!({ "id": id, "status": "downloading", "progress_pct": 0.0 }),
    );

    use tokio::io::{AsyncBufReadExt, BufReader};
    use std::time::{Duration, Instant};

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let progress_handle = {
        let app = app.clone();
        let id_line = id.to_string();
        tokio::spawn(async move {
            if let Some(out) = stdout {
                let mut lines = BufReader::new(out).lines();
                let mut last_emit = Instant::now() - Duration::from_secs(1);
                while let Ok(Some(line)) = lines.next_line().await {
                    let is_progress = (line.contains("[download]") && line.contains('%'))
                        || (line.contains("[#") && line.contains('%'));
                    if !is_progress {
                        continue;
                    }
                    // Keep the UI readable: at most ~8 updates per second.
                    if last_emit.elapsed() < Duration::from_millis(120) {
                        continue;
                    }
                    last_emit = Instant::now();
                    let _ = app.emit(
                        "download-event",
                        serde_json::json!({
                            "id": id_line,
                            "status": "downloading",
                            "progress_pct": extract_pct(&line),
                            "speed_bps": extract_speed(&line),
                            "eta_sec": extract_eta(&line),
                        }),
                    );
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
                    if err_text.len() < 12000 {
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

    (ok, error_line)
}

/// Start a video download via yt-dlp.
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

    let settings = settings::load().unwrap_or_default();
    let conns = settings.connections.clamp(1, 64);
    let aria2c_available = find_resource("aria2c.exe").is_some();

    let output_dir = resolve_output_dir();
    let dir_str = output_dir.to_string_lossy().to_string();

    // Short, Windows-safe names: bounded title + video id. A fallback that uses
    // only the id is used when the first attempt still hits a filename error.
    let template = output_dir
        .join("%(title).100s [%(id)s].%(ext)s")
        .to_string_lossy()
        .to_string();
    let short_template = output_dir
        .join("[%(id)s].%(ext)s")
        .to_string_lossy()
        .to_string();

    // Add to state as downloading (with title/thumbnail for the UI).
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

    let id_clone = id.clone();
    let app_clone = app_handle.clone();
    let url_clone = url.clone();
    let quality_clone = quality.clone();
    let format_clone = format.clone();
    let base_template = template.clone();
    let fallback_template = short_template.clone();
    let output_dir_clone = output_dir.clone();

    tokio::spawn(async move {
        // Fill in metadata the caller did not provide (batch/scan downloads).
        let mut thumb = thumbnail_url.clone();
        let mut display_title = title.clone().filter(|t| !t.trim().is_empty());
        if thumb.is_none() || display_title.is_none() {
            let (probe_title, probe_thumb) = probe_media(&url_clone).await;
            if display_title.is_none() {
                display_title = probe_title;
            }
            if thumb.is_none() {
                thumb = probe_thumb;
            }
        }
        if display_title.is_some() || thumb.is_some() {
            {
                let state = app_clone.state::<AppState>();
                let mut dl = state.downloads.lock().unwrap();
                if let Some(item) = dl.iter_mut().find(|d| d.id == id_clone) {
                    if let Some(t) = display_title.as_ref() {
                        item.title = t.clone();
                    }
                    if thumb.is_some() {
                        item.thumbnail_url = thumb.clone();
                    }
                }
            }
            let _ = app_clone.emit("download-event", serde_json::json!({
                "id": id_clone,
                "status": "downloading",
                "title": display_title,
                "thumbnail_url": thumb,
            }));
        }

        // Attempt 1: aria2c + descriptive filename. If the filename is rejected
        // (Errno 22), retry with an id-only name. If aria2c itself fails, retry
        // with yt-dlp's native downloader.
        let args = build_download_args(
            &url_clone, &quality_clone, &format_clone, conns, aria2c_available, &base_template,
        );
        let (mut ok, mut error_line) = run_ytdlp(&app_clone, &id_clone, &args).await;

        if !ok {
            let low = error_line.to_lowercase();
            let retry_args = if low.contains("errno 22") || low.contains("unable to open for writing") {
                let _ = app_clone.emit("download-event", serde_json::json!({
                    "id": id_clone, "status": "downloading", "note": "Retrying with a shorter file name…"
                }));
                build_download_args(
                    &url_clone, &quality_clone, &format_clone, conns, aria2c_available, &fallback_template,
                )
            } else if aria2c_available
                && !low.contains("conversion failed")
                && !low.contains("no space left")
            {
                let _ = app_clone.emit("download-event", serde_json::json!({
                    "id": id_clone, "status": "downloading", "note": "Retrying without aria2c…"
                }));
                build_download_args(
                    &url_clone, &quality_clone, &format_clone, conns, false, &base_template,
                )
            } else {
                Vec::new()
            };

            if !retry_args.is_empty() {
                let (retry_ok, retry_err) = run_ytdlp(&app_clone, &id_clone, &retry_args).await;
                ok = retry_ok;
                error_line = retry_err;
            }
        }

        let file_path = if ok { latest_media_file(&output_dir_clone) } else { None };
        let friendly_error = friendly_download_error(&error_line);

        // Update shared state with the final result, then persist completed items.
        {
            let state = app_clone.state::<AppState>();
            let mut dl = state.downloads.lock().unwrap();
            if let Some(item) = dl.iter_mut().find(|d| d.id == id_clone) {
                item.status = if ok { "complete".into() } else { "failed".into() };
                item.error = if ok { None } else { Some(friendly_error.clone()) };
                if ok {
                    item.progress_pct = 100.0;
                    item.file_path = file_path.clone();
                    item.completed_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0),
                    );
                }
            }
        }

        if ok {
            let completed: Vec<DownloadItem> = app_clone
                .state::<AppState>()
                .downloads
                .lock()
                .unwrap()
                .iter()
                .filter(|d| d.status == "complete")
                .cloned()
                .collect();
            persist_library(&completed);
        }

        let _ = app_clone.emit("download-event", serde_json::json!({
            "id": id_clone,
            "status": if ok { "complete" } else { "failed" },
            "progress_pct": if ok { serde_json::json!(100.0) } else { serde_json::Value::Null },
            "file_path": file_path,
            "dir": dir_str,
            "error": if ok { serde_json::Value::Null } else { serde_json::json!(friendly_error) }
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

/// Save app settings (argument name matches the `{ settings }` payload from the frontend)
#[tauri::command]
fn save_settings(settings: settings::AppSettings) -> Result<(), String> {
    settings::save(&settings)
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
    // yt-dlp: "at 2.50MiB/s"
    let yt = regex::Regex::new(r"at\s+([\d.]+)\s*(B|KiB|MiB|GiB)/s").unwrap();
    if let Some(caps) = yt.captures(line) {
        return bps(caps.get(1)?.as_str(), caps.get(2)?.as_str());
    }
    // aria2c: "DL:2.7MiB" (per second)
    let aria = regex::Regex::new(r"DL:([\d.]+)(B|KiB|MiB|GiB)").unwrap();
    if let Some(caps) = aria.captures(line) {
        return bps(caps.get(1)?.as_str(), caps.get(2)?.as_str());
    }
    None
}

fn bps(val: &str, unit: &str) -> Option<u64> {
    let v: f64 = val.parse().ok()?;
    let mult: u64 = match unit {
        "B" => 1,
        "KiB" => 1024,
        "MiB" => 1024 * 1024,
        "GiB" => 1024 * 1024 * 1024,
        _ => return None,
    };
    Some((v * mult as f64) as u64)
}

fn extract_eta(line: &str) -> Option<u64> {
    // yt-dlp prints ETA as mm:ss, or h:mm:ss for longer downloads.
    let yt = regex::Regex::new(r"ETA\s+(?:(\d+):)?(\d+):(\d+)").unwrap();
    if let Some(caps) = yt.captures(line) {
        let h: u64 = caps.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
        let min: u64 = caps.get(2)?.as_str().parse().ok()?;
        let sec: u64 = caps.get(3)?.as_str().parse().ok()?;
        return Some(h * 3600 + min * 60 + sec);
    }
    // aria2c prints ETA:6s, ETA:1m30s or ETA:1h2m3s.
    let aria = regex::Regex::new(r"ETA:(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?").unwrap();
    if let Some(caps) = aria.captures(line) {
        let h: u64 = caps.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
        let min: u64 = caps.get(2).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
        let sec: u64 = caps.get(3).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
        return Some(h * 3600 + min * 60 + sec);
    }
    None
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

    #[test]
    fn parses_aria2c_progress_summary() {
        let line = "[#8dde18 0.9MiB/2.7MiB(33%) CN:3 DL:2.7MiB ETA:1s]";
        assert!((extract_pct(line) - 33.0).abs() < 0.001);
        assert_eq!(extract_speed(line), Some(2_831_155)); // 2.7 MiB/s
        assert_eq!(extract_eta(line), Some(1));
    }

    #[test]
    fn parses_aria2c_compound_eta() {
        let line = "[#aa8a23 138MiB/152MiB(90%) CN:8 DL:45MiB ETA:1m30s]";
        assert_eq!(extract_eta(line), Some(90));
        let line2 = "[#aa8a23 138MiB/152MiB(90%) CN:8 DL:45MiB ETA:1h2m3s]";
        assert_eq!(extract_eta(line2), Some(3723));
    }
}
