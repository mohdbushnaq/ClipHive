# Changelog

All notable changes to ClipHive are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- In-app auto-updates: **Settings → Check for updates** downloads the latest release, verifies its signature, installs it over the existing version and relaunches — no manual reinstall.
- Updates are served from GitHub Releases via a signed `update-manifest.json`; each release is signed with a Tauri updater key (private key stays out of the repo).

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
