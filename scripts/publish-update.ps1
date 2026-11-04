# =============================================================================
# publish-update.ps1 - Sign the ClipHive installer and generate update-manifest.json
#
# Run this from anywhere; it resolves paths relative to itself.
# Typical use (after bumping the version in src-tauri/tauri.conf.json + package.json
# and running `npx tauri build`):
#
#   powershell -ExecutionPolicy Bypass -File scripts/publish-update.ps1 -Version 0.2.0 -Notes "Bug fixes"
#
# What it does:
#   1. Finds the freshly built installer (ClipHive_*_x64-setup.exe) in the NSIS bundle folder
#   2. Signs it with your private key (tauri-updater.key), bound to the version
#   3. Writes update-manifest.json next to the installer (this is what "Check for updates" reads)
#   4. Prints exactly which two files to attach to a GitHub Release
# =============================================================================

param(
    [string]$Version,
    [string]$Notes = ""
)

$ErrorActionPreference = "Stop"

# --- Resolve paths (script lives in <root>/scripts/) -------------------------
$root      = Split-Path -Parent $PSScriptRoot
$bundleDir = Join-Path $root "src-tauri\target\release\bundle\nsis"
$keyFile   = Join-Path $root "tauri-updater.key"
$repoUrl   = "https://github.com/mohdbushnaq/ClipHive"

# --- 1) Locate the built installer -------------------------------------------
if (-not (Test-Path $bundleDir)) {
    throw "Bundle folder not found: $bundleDir`nRun 'npx tauri build' first."
}
$installer = Get-ChildItem -Path $bundleDir -Filter "ClipHive_*_x64-setup.exe" |
             Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($null -eq $installer) {
    throw "No ClipHive_*_x64-setup.exe found in $bundleDir`nRun 'npx tauri build' first."
}

# --- Determine the version ----------------------------------------------------
if (-not $Version) {
    if ($installer.BaseName -match 'ClipHive_(\d+\.\d+\.\d+)_x64') { $Version = $Matches[1] }
}
if (-not $Version) { throw "Could not determine the version. Pass it explicitly: -Version X.Y.Z" }

# --- 2) Sign the installer (bound to this version) ---------------------------
Write-Host "Signing $($installer.Name) as v$Version ..." -ForegroundColor Cyan
$oldLocation = Get-Location
Set-Location $root   # so npx resolves the project-local Tauri CLI
try {
    & npx tauri signer sign $installer.FullName -f $keyFile --app-version $Version | Out-Null
} finally {
    Set-Location $oldLocation
}
$sigPath = "$($installer.FullName).sig"
if (-not (Test-Path $sigPath)) { throw "Signing failed: no .sig file was produced." }
$signature = (Get-Content $sigPath -Raw).Trim()

# --- 3) Build update-manifest.json -------------------------------------------
$pubDate    = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
$updateUrl  = "$repoUrl/releases/download/v$Version/$($installer.Name)"
$manifest   = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = $pubDate
    platforms = @{
        "windows-x86_64" = @{
            signature = $signature
            url       = $updateUrl
        }
    }
}
$manifestPath = Join-Path $bundleDir "update-manifest.json"
$json = $manifest | ConvertTo-Json -Depth 10
# WriteAllText uses UTF-8 WITHOUT a BOM (Set-Content -Encoding UTF8 adds one, which breaks JSON parsers).
[System.IO.File]::WriteAllText($manifestPath, $json)

# --- Done ---------------------------------------------------------------------
Write-Host ""
Write-Host "Done!" -ForegroundColor Green
Write-Host ("  Installer : {0}" -f $installer.FullName)
Write-Host ("  Signature : {0}" -f $sigPath)
Write-Host ("  Manifest  : {0}" -f $manifestPath)
Write-Host ""
Write-Host "=== NEXT: publish it to GitHub so existing installs can update ===" -ForegroundColor Yellow
Write-Host "1. Open  $repoUrl/releases"
Write-Host "2. Click 'Draft a new release' and set the tag to  v$Version"
Write-Host "3. Attach BOTH of these files:"
Write-Host ("     [1] {0}" -f $installer.Name)
Write-Host "     [2] update-manifest.json"
Write-Host "4. Click 'Publish release'."
Write-Host ""
Write-Host "Existing ClipHive installs will now find this version via  Settings -> Check for updates."
