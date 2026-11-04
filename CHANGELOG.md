# Changelog

All notable changes to ClipHive are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
