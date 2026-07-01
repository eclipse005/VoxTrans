param(
  [ValidateSet("dev", "build", "check")]
  [string]$Command = "dev",
  [string]$CudaComputeCap = "61"
)

$ErrorActionPreference = "Stop"

# Best-effort yt-dlp update: pull the latest release, but never fail the build
# when the network is unreachable — fall back to the existing binary if present.
# Only refresh during `build`; skip for `dev`/`check` to keep local iteration fast.
# Pinned yt-dlp release. Update version + SHA256 together when bumping.
$YtDlpVersion = "2026.06.09"
$YtDlpSha256 = "3a48cb955d55c8821b60ccbdbbc6f61bc958f2f3d3b7ad5eaf3d83a543293a27"
$YtDlpUrl = "https://github.com/yt-dlp/yt-dlp/releases/download/$YtDlpVersion/yt-dlp.exe"

$ytDlpDir = "src-tauri\bin"
$ytDlpPath = Join-Path $ytDlpDir "yt-dlp.exe"
if ($Command -eq "build") {
  try {
    Write-Host "Downloading yt-dlp $YtDlpVersion..."
    Invoke-WebRequest -Uri $YtDlpUrl -OutFile $ytDlpPath -UseBasicParsing -ErrorAction Stop
    $hash = (Get-FileHash -Path $ytDlpPath -Algorithm SHA256).Hash.ToLower()
    if ($hash -ne $YtDlpSha256) {
      Remove-Item -Path $ytDlpPath -Force -ErrorAction SilentlyContinue
      throw "yt-dlp SHA256 mismatch (expected $YtDlpSha256, got $hash)"
    }
    Write-Host "yt-dlp $YtDlpVersion downloaded and verified."
  } catch {
    if (Test-Path -LiteralPath $ytDlpPath) {
      Write-Host "yt-dlp update failed ($_), using existing binary."
    } else {
      Write-Host "yt-dlp update failed and no existing binary found at $ytDlpPath — build will continue, download manually if needed."
    }
  }
}

$vcvarsPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path -LiteralPath $vcvarsPath)) {
  throw "Visual Studio vcvars64.bat not found: $vcvarsPath"
}

if ($Command -eq "check") {
  cmd /c "`"$vcvarsPath`" && set CUDA_COMPUTE_CAP=$CudaComputeCap && cargo check -p voxtrans --features cuda"
  exit $LASTEXITCODE
}

cmd /c "`"$vcvarsPath`" && set CUDA_COMPUTE_CAP=$CudaComputeCap && npm run tauri $Command -- --features cuda"
exit $LASTEXITCODE
