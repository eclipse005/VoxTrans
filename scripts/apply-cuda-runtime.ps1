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
    Delete "$INSTDIR\cudart64_12.dll"
    Delete "$INSTDIR\cublas64_12.dll"
    Delete "$INSTDIR\curand64_10.dll"
    Delete "$INSTDIR\cublasLt64_12.dll"
  !endif

  ; Delete app associations
'@

# Replace the empty "Copy external binaries" / "Create file associations" placeholder
$content = $content -replace `
  '(?ms)  ; Copy external binaries\s*\r?\n\s*; Create file associations', `
  $installBlock

# Replace the empty "Delete external binaries" / "Delete app associations" placeholder
$content = $content -replace `
  '(?ms)  ; Delete external binaries\s*\r?\n\s*; Delete app associations', `
  $uninstallBlock

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
