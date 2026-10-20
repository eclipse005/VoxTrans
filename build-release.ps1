param(
  [string]$Version = "",
  [string]$CudaComputeCap = "61"
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = Read-Host "Enter release version (example: 1.0.1)"
}

$Version = $Version.Trim()
if ($Version -notmatch '^\d+\.\d+\.\d+([\-+][0-9A-Za-z\.-]+)?$') {
  throw "Invalid version format: $Version"
}

function Update-FileContent {
  param(
    [string]$Path,
    [string]$Pattern
  )

  # Read/write as UTF-8 without BOM so Cargo.toml and JSON stay valid for Rust/Node.
  $utf8NoBom = New-Object System.Text.UTF8Encoding $false
  $content = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $Path), $utf8NoBom)
  if (-not [regex]::IsMatch($content, $Pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)) {
    throw "Version field not found in: $Path"
  }
  $replacement = '${1}' + $Version + '${2}'
  $updated = [regex]::Replace($content, $Pattern, $replacement, [System.Text.RegularExpressions.RegexOptions]::Multiline)
  if ($updated -ne $content) {
    [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $Path), $updated, $utf8NoBom)
  }
}

# ── Step 1: Update version ──
Write-Host "==> Updating version to $Version ..."
Update-FileContent -Path ".\package.json" -Pattern '(^\s*"version"\s*:\s*")[^"]+(")'
Update-FileContent -Path ".\src-tauri\Cargo.toml" -Pattern '(^version\s*=\s*")[^"]+(")'
Update-FileContent -Path ".\src-tauri\tauri.conf.json" -Pattern '(^\s*"version"\s*:\s*")[^"]+(")'
Write-Host "    package.json, Cargo.toml, tauri.conf.json updated."

# ── Step 1b: Verify what Step 1 wrote, and the binaries that ship in the bundle ──
node .\scripts\verify-version.mjs --expect $Version
if ($LASTEXITCODE -ne 0) { throw "Version verification failed" }
node .\scripts\verify-binaries.mjs
if ($LASTEXITCODE -ne 0) { throw "Bundled binary verification failed" }

# ── Step 1c: prove the NSIS patch script still matches Tauri's template ──
# Tauri's stock WebView2 section treats a machine without the EdgeUpdate
# registration as "no WebView2" and aborts the whole install when its
# bootstrapper then fails -- which locks out users whose runtime works but is
# unregistered. Both installers are built from that template, so both get the
# disk probe + warn-instead-of-abort patch. Fail here, before a 20 minute build,
# if the patch's anchors no longer match Tauri's template.
$webview2Patch = ".\scripts\patch-nsis-webview2.ps1"
if (-not (Test-Path -LiteralPath $webview2Patch)) {
  throw "WebView2 installer patch missing: $webview2Patch"
}
Write-Host "==> Self-testing the WebView2 installer patch ..."
& powershell -NoProfile -ExecutionPolicy Bypass -File $webview2Patch -SelfTest
if ($LASTEXITCODE -ne 0) { throw "WebView2 installer patch self-test failed" }

# ── Step 2: Locate vcvars ──
$vcvarsPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path -LiteralPath $vcvarsPath)) {
  throw "Visual Studio vcvars64.bat not found: $vcvarsPath"
}

# ── Step 3: Build CPU version ──
Write-Host ""
Write-Host "==> Building CPU version ..."
cmd /c "`"$vcvarsPath`" && npm run tauri build"
if ($LASTEXITCODE -ne 0) { throw "CPU build failed" }

$releaseDir = ".\release"
if (-not (Test-Path -LiteralPath $releaseDir)) {
  New-Item -ItemType Directory -Path $releaseDir | Out-Null
}

# Patch WebView2 handling and rebuild: Tauri has already rendered installer.nsi
# for this variant, but the bundled exe was produced from the unpatched template.
Write-Host ""
Write-Host "==> Patching WebView2 handling (CPU installer) ..."
& powershell -NoProfile -ExecutionPolicy Bypass -File $webview2Patch -Build
if ($LASTEXITCODE -ne 0) { throw "WebView2 installer patch failed" }

$cpuSrc = ".\target\release\nsis\x64\nsis-output.exe"
$cpuDst = Join-Path $releaseDir "VoxTrans_${Version}_cpu.exe"
if (-not (Test-Path -LiteralPath $cpuSrc)) {
  throw "CPU installer not found: $cpuSrc"
}
Move-Item -LiteralPath $cpuSrc -Destination $cpuDst -Force
Write-Host "    CPU installer: $cpuDst"

# ── Step 4: Build CUDA version ──
Write-Host ""
Write-Host "==> Building CUDA version (compute cap $CudaComputeCap) ..."
cmd /c "`"$vcvarsPath`" && set CUDA_COMPUTE_CAP=$CudaComputeCap && npm run tauri build -- --features cuda"
if ($LASTEXITCODE -ne 0) { throw "CUDA build failed" }

# Tauri regenerates installer.nsi on every build, wiping both our patches.
# Order matters: WebView2 is patched first (patch only), then the CUDA runtime
# block, which is the step that re-runs NSIS -- one makensis run, both patches.
Write-Host ""
Write-Host "==> Patching WebView2 handling (CUDA installer) ..."
& powershell -NoProfile -ExecutionPolicy Bypass -File $webview2Patch
if ($LASTEXITCODE -ne 0) { throw "WebView2 installer patch failed" }

# Re-apply the CUDA download blocks and re-run NSIS with /DINCLUDE_CUDA_RUNTIME.
$cudaPatchScript = ".\scripts\apply-cuda-runtime.ps1"
if (Test-Path -LiteralPath $cudaPatchScript) {
  Write-Host ""
  Write-Host "==> Applying CUDA runtime download patch ..."
  & powershell -NoProfile -ExecutionPolicy Bypass -File $cudaPatchScript
  if ($LASTEXITCODE -ne 0) { throw "CUDA patch script failed" }

  $cudaSrc = ".\target\release\nsis\x64\nsis-output.exe"
} else {
  Write-Warning "scripts\apply-cuda-runtime.ps1 not found, using Tauri's raw installer (CUDA version will lack both the runtime download and the WebView2 patch)"
  $cudaSrc = ".\target\release\bundle\nsis\VoxTrans_${Version}_x64-setup.exe"
}

$cudaDst = Join-Path $releaseDir "VoxTrans_${Version}_cuda.exe"
if (-not (Test-Path -LiteralPath $cudaSrc)) {
  throw "CUDA installer not found: $cudaSrc"
}
Move-Item -LiteralPath $cudaSrc -Destination $cudaDst -Force
Write-Host "    CUDA installer: $cudaDst"

# ── Done ──
Write-Host ""
Write-Host "==> Done! Output:"
Write-Host "    $cpuDst"
Write-Host "    $cudaDst"
