param(
  [string]$Tag = "latest"
)

$ErrorActionPreference = "Stop"

$repo = "eclipse005/VoxTrans"
$binDir = Join-Path $PSScriptRoot ".." "src-tauri" "bin"

if (-not (Test-Path -LiteralPath $binDir)) {
  New-Item -ItemType Directory -Path $binDir | Out-Null
}

$binaries = @(
  "ffmpeg.exe",
  "yt-dlp.exe",
  "fireredvad.exe",
  "demucs.exe"
)

if ($Tag -eq "latest") {
  $releaseUrl = "https://api.github.com/repos/$repo/releases/latest"
} else {
  $releaseUrl = "https://api.github.com/repos/$repo/releases/tags/$Tag"
}

Write-Host "Fetching release info: $releaseUrl"
$releaseJson = Invoke-RestMethod -Uri $releaseUrl -Headers @{ Accept = "application/vnd.github+json" }

$assetMap = @{}
foreach ($asset in $releaseJson.assets) {
  $assetMap[$asset.name] = $asset.browser_download_url
}

foreach ($bin in $binaries) {
  $dst = Join-Path $binDir $bin
  if (Test-Path -LiteralPath $dst) {
    Write-Host "Skipping $bin (already exists)"
    continue
  }

  $url = $assetMap[$bin]
  if (-not $url) {
    Write-Warning "Asset not found in release: $bin"
    continue
  }

  Write-Host "Downloading $bin ..."
  Invoke-WebRequest -Uri $url -OutFile $dst
  Write-Host "  -> $dst"
}

Write-Host "Done."
