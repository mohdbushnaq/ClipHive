# Contributing to ClipHive

Thanks for your interest in making ClipHive better! This guide covers how to set up a local build and how to submit changes.

## Ground rules

- Be respectful. Keep discussions focused on the code and the product.
- One change per pull request makes reviews much easier.
- If you're fixing a bug, say what was broken and how your fix resolves it.
- If you're adding a feature, explain the user-facing benefit in the PR description.

## Setting up a local build

### Prerequisites

- [Rust](https://rustup.rs/) — stable toolchain (`rustc` + `cargo`)
- [Node.js 18+](https://nodejs.org/) and npm
- **Windows only:** [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload
- WebView2 Runtime (pre-installed on Windows 10/11)

### Steps

```bash
# 1. Clone and enter the repo
git clone https://github.com/YOUR_USERNAME/cliphive.git
cd cliphive

# 2. Install frontend dependencies
npm install

# 3. Download the bundled video engine + media tools (yt-dlp, ffmpeg) — one-time
powershell -ExecutionPolicy Bypass -File scripts/fetch-resources.ps1

# 4. Run in development mode (hot reload)
npx tauri dev
```

> Step 3 is required before any build: the bundled binaries live in `src-tauri/resources/` and are git-ignored to keep the repo small. The script fetches them automatically.

## Submitting a change

1. **Fork** the repository (or work from your own branch if you have write access).
2. Create a feature branch:
   ```bash
   git checkout -b bionic/your-short-description
   ```
3. Make your changes and test them locally with `npx tauri dev`.
4. Commit with a clear, imperative message (e.g. "Fix pause not resuming downloads").
5. Push the branch and open a pull request against `main`.

## Code style

- **Rust:** run `cargo fmt` before committing; keep functions small and let the compiler's warnings guide you (`cargo clippy` is welcome).
- **Frontend (JS/CSS):** match the existing patterns in `src/`; no new build dependencies unless clearly justified.
- Keep the UI consistent with the ClipHive design system (dark theme, amber/gold accent) defined in `src/styles.css`.

## Reporting issues

Please open an issue before working on a large change so we can align on approach. Include:

- What you expected to happen vs. what actually happened
- Steps to reproduce
- Your OS and ClipHive version (shown in Settings → About)
