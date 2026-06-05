param(
  [string]$NsisDir = ".\target\release\nsis\x64",
  [string]$Makensis = "C:\Users\ADMIN\AppData\Local\tauri\NSIS\Bin\makensis.exe"
)

$ErrorActionPreference = "Stop"

$installerNsi = Join-Path $NsisDir "installer.nsi"
if (-not (Test-Path $installerNsi)) {
  throw "installer.nsi not found at: $installerNsi"
}

# ── 1. Inject CUDA runtime download + delete blocks ──
$content = Get-Content $installerNsi -Raw

$installBlock = @'
  ; Copy external binaries
  !ifdef INCLUDE_CUDA_RUNTIME
  ; Skip download if DLLs already present (e.g., update from bundled version)
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

  ; Create file associations
'@

$uninstallBlock = @'
  ; Delete external binaries
  !ifdef INCLUDE_CUDA_RUNTIME
    ${If} $DeleteAppDataCheckboxState = 1
    ${AndIf} $UpdateMode <> 1
      Delete "$INSTDIR\cudart64_12.dll"
      Delete "$INSTDIR\cublas64_12.dll"
      Delete "$INSTDIR\curand64_10.dll"
      Delete "$INSTDIR\cublasLt64_12.dll"
    ${EndIf}
  !endif

  ; Delete app associations
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

# Replace the empty "Copy external binaries" / "Create file associations" placeholder.
# Use a MatchEvaluator (scriptblock) so $0 / $1 in $installBlock are not
# interpreted as regex backreferences or PowerShell variables.
$content = [regex]::Replace(
  $content,
  '(?ms)  ; Copy external binaries\s*\r?\n\s*; Create file associations',
  [System.Text.RegularExpressions.MatchEvaluator]{ param($m) $installBlock }
)

# Replace the empty "Delete external binaries" / "Delete app associations" placeholder
$content = [regex]::Replace(
  $content,
  '(?ms)  ; Delete external binaries\s*\r?\n\s*; Delete app associations',
  [System.Text.RegularExpressions.MatchEvaluator]{ param($m) $uninstallBlock }
)

# Wrap Tauri-generated `RMDir /REBOOTOK "$INSTDIR\bin"` + `RMDir "$INSTDIR"`
# behind the same checkbox + update-mode guard.
$content = [regex]::Replace(
  $content,
  '(?ms)  RMDir /REBOOTOK "\$INSTDIR\\bin"\r?\n  RMDir "\$INSTDIR"',
  [System.Text.RegularExpressions.MatchEvaluator]{ param($m) $binAndInstdirBlock }
)

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
