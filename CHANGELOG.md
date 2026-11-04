# Changelog

All notable changes to ClipHive are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-11-04

### Fixed
- **“[Errno 22] Invalid argument” on large videos.** yt-dlp could not open its output file when a title produced an over-long or invalid Windows filename. Downloads now use bounded, Windows-safe names (`title` capped at 100 characters plus the video id), `--windows-filenames` and `--trim-filenames`, and automatically retry with an id-only filename if the first attempt is still rejected.
- If the bundled aria2c downloader fails, the download is retried automatically with yt-dlp's native downloader instead of failing outright.
- Download errors are now explained instead of dumped raw: invalid filename, not enough free disk space while merging a large 4K/HDR file, HTTP 403, private/age-restricted videos, and aria2c failures each get a plain-language message.
- **Settings → Save no longer fails.** The Rust command expected an argument named `s` while the frontend sent `settings`, producing “command save_settings missing required key s”. The argument name now matches.
- Removed the Save Settings button. Every settings change is saved automatically (debounced), with a small “Saved ✓” toast in the top-right corner. Errors appear in the same toast.

### Added
- **Connections per Download is now 1–64** (was 1–16), for much faster multi-connection downloads on high-bandwidth links.
- **Thumbnails in the Library.** Completed downloads now display their thumbnail. Items without a stored thumbnail get a YouTube thumbnail derived from the URL, or a neutral placeholder.
- Batch and page-scan downloads now fetch their title and thumbnail from the video metadata, so their cards and Library entries show the same information as single-video downloads.
- **Automatic update check on launch.** ClipHive checks for a new version about two seconds after the window opens; if one is found it downloads, verifies and installs it. The manual “Check for updates” button remains available.

## [0.2.0] - 2026-11-04

### Added
- **Multi-connection downloads.** Regular files are now split into pieces and fetched over several parallel HTTP connections with the bundled aria2c, instead of one serial stream, so downloads can use the full available bandwidth.
- Fragmented streams (DASH/HLS) now download several fragments at once (`--concurrent-fragments`).
- New **Settings → Connections per Download (1–16)** option (default 8) controls how many parallel connections are used.
- aria2c is bundled with the app and fetched by `scripts/fetch-resources.ps1`; if it is missing, ClipHive falls back to yt-dlp's native downloader.
- Progress parsing understands aria2c's progress summary as well as yt-dlp's own output, so the percentage, speed and ETA stay live during multi-connection downloads.

## [0.1.2] - 2026-11-04

### Fixed
- **Live download progress.** The progress bar never moved because the backend read yt-dlp's progress from stderr, while yt-dlp writes it to stdout, and it was suppressed by `Stdio::null()`. Progress is now streamed from stdout with `--newline`.
- Download cards show a real progress bar with the exact percentage, current download speed and ETA as the transfer runs.
- Failed downloads now report yt-dlp's actual error line instead of a generic "Download failed".

## [0.1.1] - 2026-11-04

### Fixed
- **Settings → Check for updates no longer errors out.** The v0.1.0 release shipped without the signed `update-manifest.json`, so the updater's endpoint returned HTTP 404 and the app showed a raw failure message. The manifest is now published with every release.
- Update errors are reported in plain language (missing manifest, no connection, signature check failed) instead of the raw underlying error, and the Check button is always re-enabled afterwards.
- Update downloads now show percentage progress and a proper "installing" state instead of a static message.

### Changed
- `scripts/publish-update.ps1` now points the manifest at the release's own tag (`releases/download/vX.Y.Z/...`) rather than `releases/latest`, so older installs keep working after newer releases are published.

## [0.1.0] - 2026-11-02

### Added
- Initial public release of ClipHive.
- **Page Scanner** — paste any URL and detect every video on the page, then pick specific ones or grab them all at once.
- Smart downloads with quality selection (Auto / 4K / 1080p / 720p / 480p / 360p) and format options (MP4, WebM, audio-only).
- Real-time download progress: speed, ETA, and percentage per file.
- Pause / Resume / Cancel control over active downloads.
- Concurrent downloads with configurable limits.
- **Batch mode** — paste multiple URLs (one per line), validate them all, and queue the valid ones in one click.
- Settings: default quality/format, speed limit, proxy support, custom filename patterns.
- Library view to browse completed downloads, search by title, open folders, or clear history.
- Self-contained Windows installer and portable build with the video engine and media tools bundled inside — no manual prerequisites for end users.

### Notes
- Built with Rust + Tauri 2 for a small install size and low memory footprint compared to Electron-based alternatives.
