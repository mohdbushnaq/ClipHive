# ClipHive

**Every video on any page. One paste. No install.**

ClipHive is a fast, lightweight desktop app that finds and downloads videos from any webpage. Paste a link — it detects every downloadable video on that page, lets you pick quality and format, and saves the files to your computer. Built with Rust for speed and small size.

---

## Features

### 🎯 Page Scanner
Paste **any** URL — an article, course page, blog post, social profile. ClipHive scans the entire page HTML and finds every video source: embedded players (YouTube, Vimeo, Wistia), direct file links (.mp4, .webm, .mov), JSON-LD structured data, and more. Pick which ones to grab or download them all at once.

### 📥 Smart Downloads
- **Quality selection** — Auto (best available), 4K, 1080p, 720p, 480p, 360p
- **Format options** — MP4, WebM, or audio-only (MP3/M4A)
- **Real-time progress** — speed, ETA, and percentage for every download
- **Pause / Resume / Cancel** — full control over active downloads
- **Concurrent downloads** — run multiple at once with configurable limits

### 📋 Batch Mode
Paste multiple video URLs (one per line), validate them all at once, see which are valid, and queue the good ones in a single click. Perfect for playlists of links you've collected.

### ⚙️ Settings
- Default quality and format preferences
- Speed limit (MB/s) to preserve bandwidth
- Proxy support for restricted networks
- Custom filename patterns (`{title}_{quality}`, etc.)
- Dark theme (built-in)

### 📁 Library
Browse all your completed downloads. Search by title, open the file's folder in Explorer, or remove entries from history.

---

## Why ClipHive?

| | ClipHive | Typical alternatives |
|--|----------|---------------------|
| **Install size** | ~120 MB (self-contained) | 300+ MB (Electron apps) |
| **Runtime memory** | ~50 MB (Rust + WebView2) | 300–800 MB (Chromium) |
| **Dependencies** | None — everything bundled | Python, Node.js, system ffmpeg |
| **Page scanning** | ✅ Finds ALL videos on any page | ❌ Usually one URL = one video |
| **API access** | Built-in IPC for scripting | Rarely available |
| **Ads / nag screens** | None | Common in free tiers |

---

## Requirements

- **Windows 10 or 11** (64-bit)
- That's it. No Python, no ffmpeg, no other software needed.

> ClipHive bundles its own video download engine and media processing tools inside the installer. Everything is self-contained.

---

## Installation

### Option A: Installer (recommended)

1. Download `ClipHive_0.1.0_x64-setup.exe` from [Releases](../../releases)
2. Double-click to install (per-user, no admin required)
3. Launch ClipHive from your Start Menu or Desktop shortcut

### Option B: Portable

1. Download the `cliphive.zip` portable build from [Releases](../../releases)
2. Extract to any folder (USB drive works great)
3. Run `cliphive.exe` — no installation needed

---

## Usage

### Quick start

1. Open ClipHive
2. Paste a video URL or any page URL into the input field
3. Click **"Get Video"** for a single video, or **"Scan Page"** to find all videos on that page
4. Select your preferred quality and format
5. Click **"Add to Download Queue"** (or "Download Selected" for multiple)
6. Watch progress in real-time — speed, ETA, percentage
7. When done, click **"Open Folder"** to access the file

### Where do files go?

Downloads are saved to: `Your Downloads folder / ClipHive/`

You can change this in Settings (coming in v0.2).

---

## Supported Platforms & Sources

ClipHive uses a modern video extraction engine that supports 1000+ sites including:

- YouTube, Vimeo, Dailymotion
- TikTok, Instagram, Twitter/X, Facebook
- SoundCloud, Twitch, and hundreds more
- **Any page** with embedded `<video>` tags, iframes, or direct media file links (Page Scanner)

> Support for specific sites depends on the upstream extraction engine. If a site stops working, updating ClipHive to the latest version usually fixes it.

---

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- [Node.js 18+](https://nodejs.org/)
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (C++ workload) — for Windows builds
- WebView2 Runtime (pre-installed on Windows 10/11)

### Building from source

```bash
# Clone the repo
git clone https://github.com/YOUR_USERNAME/cliphive.git
cd cliphive

# Install frontend dependencies
npm install

# Download the bundled video engine + media tools (yt-dlp, ffmpeg) — one-time step
powershell -ExecutionPolicy Bypass -File scripts/fetch-resources.ps1

# Build in development mode (hot reload)
npx tauri dev

# Build release installer
npx tauri build
```

The release installer will be at:
```
src-tauri/target/release/bundle/nsis/ClipHive_0.1.0_x64-setup.exe
```

### Project structure

```
cliphive/
├── src/                    # Frontend (HTML/CSS/JS)
│   ├── index.html          # App UI — all tabs and views
│   ├── main.js             # Frontend logic + Tauri IPC calls
│   └── styles.css          # ClipHive design system (dark theme, amber/gold)
├── src-tauri/              # Rust backend (Tauri 2)
│   ├── Cargo.toml          # Rust dependencies
│   ├── tauri.conf.json     # App config, window settings, bundle options
│   ├── icons/icon.ico      # App icon (multi-resolution)
│   ├── resources/          # Bundled binaries (yt-dlp, ffmpeg)
│   └── src/
│       ├── main.rs         # Entry point
│       ├── lib.rs          # Tauri commands (IPC API exposed to frontend)
│       ├── download/       # Download queue management
│       ├── page_scan/      # Page scanner (HTML → video extraction)
│       └── settings/       # Settings persistence (%APPDATA%)
├── scripts/
│   └── fetch-resources.ps1 # Downloads bundled binaries (yt-dlp, ffmpeg) for building
├── package.json            # Frontend build tooling (Vite + Tauri CLI)
├── vite.config.js          # Vite configuration
├── LICENSE                 # MIT License
└── README.md               # This file
```

### Architecture

```
┌─────────────────────────────────────────────┐
│  Frontend (WebView2)                         │
│  HTML + CSS + JS → Tauri IPC calls          │
├─────────────────────────────────────────────┤
│  Rust Backend (Tauri 2 core)                │
│  • analyze_url()    → yt-dlp --dump-json    │
│  • start_download() → yt-dlp subprocess     │
│  • scan_page()      → HTTP fetch + regex    │
│  • get/save_settings→ JSON in %APPDATA%     │
├─────────────────────────────────────────────┤
│  Bundled Binaries (in resources/)           │
│  • yt-dlp.exe   — video extraction engine   │
│  • ffmpeg.exe   — codec/merge support       │
│  • ffprobe.exe  — format detection          │
└─────────────────────────────────────────────┘
```

---

## License

[MIT](./LICENSE) — free to use, modify, and distribute. See [CONTRIBUTING.md](./CONTRIBUTING.md) if you'd like to help.

---

## Disclaimer

ClipHive is a download tool. You are responsible for ensuring you have the right to download content from any given source. Do not use this software to infringe copyright or violate any platform's terms of service. The authors accept no liability for how this software is used.
