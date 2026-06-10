param(
  [switch]$Force
)

$ErrorActionPreference = "Stop"

$repo = "eclipse005/VoxTrans"
$toolsTag = "tools"
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

$releaseUrl = "https://api.github.com/repos/$repo/releases/tags/$toolsTag"
Write-Host "Fetching release info: $releaseUrl"
$releaseJson = Invoke-RestMethod -Uri $releaseUrl -Headers @{ Accept = "application/vnd.github+json" }

$assetMap = @{}
foreach ($asset in $releaseJson.assets) {
  $assetMap[$asset.name] = $asset.browser_download_url
}

foreach ($bin in $binaries) {
  $dst = Join-Path $binDir $bin
  if (Test-Path -LiteralPath $dst) {
    if ($Force) {
      Write-Host "Re-downloading $bin ..."
    } else {
      Write-Host "Skipping $bin (already exists, use -Force to re-download)"
      continue
    }
  } else {
    Write-Host "Downloading $bin ..."
  }

  $url = $assetMap[$bin]
  if (-not $url) {
    Write-Warning "Asset not found in release: $bin"
    continue
  }

  Invoke-WebRequest -Uri $url -OutFile $dst
  Write-Host "  -> $dst"
}

Write-Host "Done."
