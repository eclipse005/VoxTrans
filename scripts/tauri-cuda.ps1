param(
  [ValidateSet("dev", "build", "check")]
  [string]$Command = "dev",
  [string]$CudaComputeCap = "61"
)

$ErrorActionPreference = "Stop"

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
