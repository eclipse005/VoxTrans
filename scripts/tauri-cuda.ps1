param(
  [ValidateSet("dev", "build", "check")]
  [string]$Command = "dev",
  [string]$CudaComputeCap = "61"
)

$ErrorActionPreference = "Stop"

# Best-effort yt-dlp update: pull the latest release, but never fail the build
# when the network is unreachable — fall back to the existing binary if present.
# Only refresh during `build` (packaging), so installer builds always ship the
# newest yt-dlp; `dev`/`check` skip it to keep local iteration fast.
$ytDlpDir = "src-tauri\bin"
$ytDlpPath = Join-Path $ytDlpDir "yt-dlp.exe"
if ($Command -eq "build") {
  try {
    Write-Host "Updating yt-dlp to latest..."
    Invoke-WebRequest -Uri "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" -OutFile $ytDlpPath -UseBasicParsing -ErrorAction Stop
    Write-Host "yt-dlp updated to latest."
  } catch {
    if (Test-Path -LiteralPath $ytDlpPath) {
      Write-Host "yt-dlp update failed ($_); using existing binary."
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
