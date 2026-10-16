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
  "demucs.exe"
)

$releaseUrl = "https://api.github.com/repos/$repo/releases/tags/$toolsTag"
Write-Host "Fetching release info: $releaseUrl"

# Unauthenticated API calls are limited to 60/hour per IP and CI runners share
# IPs — the first CI run to use this script died with a 429. Use GH_TOKEN /
# GITHUB_TOKEN when present (workflows pass secrets.GITHUB_TOKEN); plain local
# runs stay unauthenticated.
$headers = @{ Accept = "application/vnd.github+json" }
$token = if ($env:GH_TOKEN) { $env:GH_TOKEN } else { $env:GITHUB_TOKEN }
if ($token) { $headers["Authorization"] = "Bearer $token" }

$releaseJson = Invoke-RestMethod -Uri $releaseUrl -Headers $headers

# The release assets can be re-uploaded, so the API's own sha256 digest of the
# asset is the trust anchor for what we just fetched (same rule as
# scripts/binaries.sha256 for the working tree). GitHub serves `digest` as
# "sha256:<hex>"; if it is ever missing, warn rather than block the download.
$assetMap = @{}
foreach ($asset in $releaseJson.assets) {
  $assetMap[$asset.name] = @{ url = $asset.browser_download_url; digest = $asset.digest }
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

  $asset = $assetMap[$bin]
  if (-not $asset -or -not $asset.url) {
    Write-Warning "Asset not found in release: $bin"
    continue
  }

  Invoke-WebRequest -Uri $asset.url -OutFile $dst

  if ($asset.digest -and $asset.digest.StartsWith("sha256:")) {
    $expected = $asset.digest.Substring("sha256:".Length).ToLowerInvariant()
    $actual = (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
      Remove-Item -LiteralPath $dst -Force
      throw "digest mismatch for ${bin}: expected $expected, got $actual (file removed)"
    }
    Write-Host "  digest ok"
  } else {
    Write-Warning "Asset $bin has no digest in the API response; cannot verify its content"
  }

  Write-Host "  -> $dst"
}

Write-Host "Done."
