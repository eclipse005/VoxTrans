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
  $tmpPath = Join-Path ([System.IO.Path]::GetTempPath()) "yt-dlp.latest.exe"
  try {
    Write-Host "Updating yt-dlp to latest..."
    $response = Invoke-WebRequest -Uri "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" -OutFile $tmpPath -UseBasicParsing -PassThru -ErrorAction Stop
    # A truncated download still yields an .exe but it fails to launch
    # ("Could not load PyInstaller's embedded PKG archive"), which shipped a
    # broken yt-dlp once already. Verify the byte count before replacing.
    $downloaded = (Get-Item -LiteralPath $tmpPath).Length
    if ($response.RawContentLength -gt 0 -and $downloaded -ne $response.RawContentLength) {
      throw "incomplete download ($downloaded of $($response.RawContentLength) bytes)"
    }
    if ($downloaded -lt 10MB) {
      throw "suspiciously small download ($downloaded bytes)"
    }
    Move-Item -LiteralPath $tmpPath -Destination $ytDlpPath -Force
    Write-Host "yt-dlp updated to latest."
  } catch {
    Remove-Item -LiteralPath $tmpPath -ErrorAction SilentlyContinue
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
