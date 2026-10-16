param(
  [string]$NsisDir = ".\target\release\nsis\x64",
  [string]$Makensis = ""
)

$ErrorActionPreference = "Stop"

# Tauri caches NSIS under the user profile; resolve it instead of hardcoding a
# machine-specific path, so the same script runs on a CI runner.
if (-not $Makensis) {
  $Makensis = Join-Path $env:LOCALAPPDATA "tauri\NSIS\Bin\makensis.exe"
}
if (-not (Test-Path -LiteralPath $Makensis)) {
  throw "makensis not found at '$Makensis' (run a tauri bundle build first, or pass -Makensis)"
}

$installerNsi = Join-Path $NsisDir "installer.nsi"
if (-not (Test-Path $installerNsi)) {
  throw "installer.nsi not found at: $installerNsi"
}

# ── 1. Inject CUDA runtime download + delete blocks ──
# Scheme B: only compute runtime DLLs (cudart / cublas / cublasLt / curand).
# Do NOT download nvrtc / nvrtc-builtins / nvJitLink / CUDA headers — kernels
# ship as precompiled multi-arch PTX embedded in the engine crates.
#
# Anchor strategy: each block is inserted directly BEFORE the next section's
# banner comment, and every anchor must match exactly once or the script
# throws. An earlier version replaced the whitespace between two adjacent
# banner comments ("Copy external binaries" .. "Create file associations"),
# which silently stopped matching when Tauri 2.10 began rendering an
# `{{#each binaries}}` File loop between them — v1.6.0 shipped a CUDA
# installer whose download block was simply absent, and nobody could see it.
# A silent miss is the failure mode this script must never have again.
$content = Get-Content $installerNsi -Raw

$installBlock = @'
  !ifdef INCLUDE_CUDA_RUNTIME
  ; CUDA 12.8 user-mode libs only (no NVRTC). Skip if already present.
  ${IfNot} ${FileExists} "$INSTDIR\cudart64_12.dll"
    DetailPrint "Downloading CUDA runtime..."
    NSISdl::download "https://modelscope.cn/models/eclipse005/cuda-runtime-12.8/resolve/master/cudart64_12.dll" "$INSTDIR\cudart64_12.dll"
    Pop $0
    ${If} $0 != "success"
      MessageBox MB_ICONEXCLAMATION "Failed to download cudart64_12.dll (error: $0).$\r$\nThe CUDA build will fail to launch. Please rerun the installer with a working network."
      Goto cuda_done
    ${EndIf}
    NSISdl::download "https://modelscope.cn/models/eclipse005/cuda-runtime-12.8/resolve/master/cublas64_12.dll" "$INSTDIR\cublas64_12.dll"
    Pop $0
    ${If} $0 != "success"
      MessageBox MB_ICONEXCLAMATION "Failed to download cublas64_12.dll (error: $0).$\r$\nThe CUDA build will fail to launch. Please rerun the installer with a working network."
      Goto cuda_done
    ${EndIf}
    NSISdl::download "https://modelscope.cn/models/eclipse005/cuda-runtime-12.8/resolve/master/curand64_10.dll" "$INSTDIR\curand64_10.dll"
    Pop $0
    ${If} $0 != "success"
      MessageBox MB_ICONEXCLAMATION "Failed to download curand64_10.dll (error: $0).$\r$\nThe CUDA build will fail to launch. Please rerun the installer with a working network."
      Goto cuda_done
    ${EndIf}
    NSISdl::download "https://modelscope.cn/models/eclipse005/cuda-runtime-12.8/resolve/master/cublasLt64_12.dll" "$INSTDIR\cublasLt64_12.dll"
    Pop $0
    ${If} $0 != "success"
      MessageBox MB_ICONEXCLAMATION "Failed to download cublasLt64_12.dll (error: $0).$\r$\nThe CUDA build will fail to launch. Please rerun the installer with a working network."
      Goto cuda_done
    ${EndIf}
    DetailPrint "CUDA runtime download complete"
  ${Else}
    DetailPrint "Existing CUDA runtime detected, skipping download"
  ${EndIf}
  cuda_done:
  !endif

'@

$uninstallBlock = @'
  !ifdef INCLUDE_CUDA_RUNTIME
    ${If} $DeleteAppDataCheckboxState = 1
    ${AndIf} $UpdateMode <> 1
      Delete "$INSTDIR\cudart64_12.dll"
      Delete "$INSTDIR\cublas64_12.dll"
      Delete "$INSTDIR\curand64_10.dll"
      Delete "$INSTDIR\cublasLt64_12.dll"
    ${EndIf}
  !endif

'@

# Tauri's uninstaller section also unconditionally wipes $INSTDIR\bin and tries
# to remove $INSTDIR. We gate those on the same "user really wants to wipe
# everything" checkbox so that a plain uninstall (no checkbox) preserves
# $INSTDIR contents (bin/, models/, output/, etc.) for the next install.
$binAndInstdirBlock = @'
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /REBOOTOK "$INSTDIR\bin"
    RmDir /r "$INSTDIR"
  ${EndIf}
'@

# MatchEvaluator (scriptblock) everywhere below: NSIS's $0 / ${...} text must
# not be interpreted as regex backreferences or PowerShell variables.
function Insert-Before-Required {
  param(
    [string]$Text,
    [string]$AnchorPattern,
    [string]$Block,
    [string]$Label
  )
  $anchorMatches = [regex]::Matches($Text, $AnchorPattern)
  if ($anchorMatches.Count -ne 1) {
    throw "NSIS anchor '$Label' matched $($anchorMatches.Count) time(s) in installer.nsi - Tauri's NSIS template changed; update apply-cuda-runtime.ps1 before building the CUDA installer"
  }
  return [regex]::Replace($Text, $AnchorPattern, [System.Text.RegularExpressions.MatchEvaluator]{ param($m) $Block + $m.Value })
}

$content = Insert-Before-Required -Text $content -AnchorPattern '(?m)^  ; Create file associations\r?$' -Block $installBlock -Label 'Create file associations'
$content = Insert-Before-Required -Text $content -AnchorPattern '(?m)^  ; Delete app associations\r?$' -Block $uninstallBlock -Label 'Delete app associations'

$rmdirPattern = '(?ms)  RMDir /REBOOTOK "\$INSTDIR\\bin"\r?\n  RMDir "\$INSTDIR"'
$rmdirMatches = [regex]::Matches($content, $rmdirPattern)
if ($rmdirMatches.Count -ne 1) {
  throw "NSIS anchor 'RMDir /REBOOTOK ...RMDir \$INSTDIR' matched $($rmdirMatches.Count) time(s) - Tauri's NSIS template changed; update apply-cuda-runtime.ps1"
}
$content = [regex]::Replace($content, $rmdirPattern, [System.Text.RegularExpressions.MatchEvaluator]{ param($m) $binAndInstdirBlock })

# Belt and braces: both runtime blocks are in, and they are the only ones.
$defineCount = [regex]::Matches($content, '!ifdef INCLUDE_CUDA_RUNTIME').Count
if ($defineCount -ne 2 -or $content -notmatch 'cuda_done:') {
  throw "post-patch self-check failed: $defineCount INCLUDE_CUDA_RUNTIME blocks, cuda_done present: $($content -match 'cuda_done:')"
}

Set-Content -LiteralPath $installerNsi -Value $content -NoNewline
Write-Host "Patched installer.nsi with conditional CUDA runtime block"

# ── 2. Rebuild the installer with /DINCLUDE_CUDA_RUNTIME ──
$nsisFile = Join-Path $NsisDir "installer.nsi"
Write-Host "Running makensis with /DINCLUDE_CUDA_RUNTIME..."
& $Makensis /DINCLUDE_CUDA_RUNTIME $nsisFile | Out-Null
if ($LASTEXITCODE -ne 0) {
  throw "makensis failed with exit code $LASTEXITCODE"
}

$outputExe = Join-Path $NsisDir "nsis-output.exe"
if (-not (Test-Path $outputExe)) {
  throw "nsis-output.exe not produced"
}

Write-Host "Built: $outputExe ($((Get-Item $outputExe).Length) bytes)"
