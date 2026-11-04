# =============================================================================
# fetch-resources.ps1
# Downloads the bundled binaries ClipHive needs to build and run:
#   - yt-dlp.exe  (video extraction engine)
#   - ffmpeg.exe  (codec / merge support)
#   - ffprobe.exe (format detection)
#   - aria2c.exe  (multi-connection downloader)
#
# They are stored in src-tauri/resources/ and git-ignored on purpose so the
# repository stays small. Run this once before building:
#
#     powershell -ExecutionPolicy Bypass -File scripts/fetch-resources.ps1
# =============================================================================

$ErrorActionPreference = "Stop"

# Resolve paths relative to this script (scripts/ -> repo root)
$root    = Split-Path -Parent $PSScriptRoot
$resDir  = Join-Path $root "src-tauri\resources"
$tempDir = Join-Path $env:TEMP ("cliphive-fetch-" + [Guid]::NewGuid().ToString("N"))

Write-Host "ClipHive resource fetcher" -ForegroundColor Cyan
Write-Host "Target folder: $resDir" -ForegroundColor DarkGray

New-Item -ItemType Directory -Force -Path $resDir  | Out-Null
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

function Download([string]$url, [string]$out) {
    Write-Host "  -> $url"
    Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
}

# --- 1) yt-dlp (single-file Windows executable from official releases) -------
$yt = Join-Path $resDir "yt-dlp.exe"
if (-not (Test-Path $yt)) {
    Write-Host "[1/3] Downloading yt-dlp..." -ForegroundColor Yellow
    Download "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" $yt
} else {
    Write-Host "[1/3] yt-dlp.exe already present, skipping." -ForegroundColor Green
}

# --- 2) ffmpeg + ffprobe (from the gyan.dev essentials build) ----------------
$ffmpeg  = Join-Path $resDir "ffmpeg.exe"
$ffprobe = Join-Path $resDir "ffprobe.exe"
if (-not ((Test-Path $ffmpeg) -and (Test-Path $ffprobe))) {
    Write-Host "[2/3] Downloading ffmpeg + ffprobe..." -ForegroundColor Yellow
    $zip = Join-Path $tempDir "ffmpeg.zip"
    Download "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" $zip

    Expand-Archive -Path $zip -DestinationPath $tempDir -Force

    # The archive nests the binaries under a versioned folder's bin/.
    $found = Get-ChildItem -Path $tempDir -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
    if ($null -eq $found) { throw "ffmpeg.exe was not found inside the downloaded archive." }

    Copy-Item (Join-Path $found.DirectoryName "ffmpeg.exe")  $ffmpeg  -Force
    Copy-Item (Join-Path $found.DirectoryName "ffprobe.exe") $ffprobe -Force
} else {
    Write-Host "[2/3] ffmpeg + ffprobe already present, skipping." -ForegroundColor Green
}

# --- 3) aria2c (multi-connection downloader, from official releases) ---------
$aria = Join-Path $resDir "aria2c.exe"
if (-not (Test-Path $aria)) {
    Write-Host "[3/3] Downloading aria2c..." -ForegroundColor Yellow
    $zip = Join-Path $tempDir "aria2.zip"
    Download "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip" $zip

    $unzip = Join-Path $tempDir "aria2"
    Expand-Archive -Path $zip -DestinationPath $unzip -Force

    $found = Get-ChildItem -Path $unzip -Recurse -Filter "aria2c.exe" | Select-Object -First 1
    if ($null -eq $found) { throw "aria2c.exe was not found inside the downloaded archive." }
    Copy-Item $found.FullName $aria -Force
} else {
    Write-Host "[3/3] aria2c.exe already present, skipping." -ForegroundColor Green
}

# --- Cleanup ------------------------------------------------------------------
Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Done. Bundled binaries are now in:" -ForegroundColor Cyan
Get-ChildItem $resDir | Sort-Object Name | Format-Table Name, @{Label="Size (MB)";Expression={[math]::Round($_.Length/1MB, 2)}} -AutoSize
