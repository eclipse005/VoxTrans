param(
  [string]$NsisDir = ".\target\release\nsis\x64",
  [string]$Makensis = "",
  [switch]$Build,
  [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

# Marker left in the patched template: makes a second run a no-op (a Tauri build
# regenerates installer.nsi, but a failed release can leave the patched file
# behind) and gives the post-patch self-check something to count.
$marker = "; VOXTRANS-WEBVIEW2-PATCH"

# -- Why this patch exists ---------------------------------------------------
# Tauri's stock WebView2 section trusts the EdgeUpdate registry key
# ({F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}\pv) and nothing else. A machine whose
# runtime is present but unregistered -- Edge "debloat" scripts that delete the
# keys, fixed-version deployments, a per-user install made under another
# account -- looks exactly like "WebView2 is missing": the installer downloads
# Microsoft's bootstrapper, that fails for its own reasons, and the whole
# install is then Abort-ed with "Failed to install WebView2!".
#
# Two changes, both anchored so a Tauri template change fails the build loudly
# instead of shipping an unpatched installer (docs/lessons.md, rule 3):
#   1. before trusting the registry, look for msedgewebview2.exe on disk;
#   2. a failed bootstrapper warns and continues instead of aborting -- the app
#      still starts on every machine that actually has the runtime.
#
# Both the CPU and the CUDA installer come from the same template, so both need
# this; see build-release.ps1 and .github/workflows/release.yml.
#
# Usage:
#   .\scripts\patch-nsis-webview2.ps1           patch installer.nsi only
#   .\scripts\patch-nsis-webview2.ps1 -Build    patch, then re-run makensis
#   .\scripts\patch-nsis-webview2.ps1 -SelfTest prove the patch logic itself

function Join-Crlf {
  param([string]$Text)
  # The repo normalizes text files to LF; NSIS templates are CRLF. Normalize the
  # inserted block so a patch never leaves mixed line endings behind.
  return ($Text -replace '\r?\n', "`r`n")
}

$probeBlock = (Join-Crlf @'
  ; VOXTRANS-WEBVIEW2-PATCH: probe the runtime on disk before trusting EdgeUpdate
  ; A runtime that is not registered in EdgeUpdate can still be perfectly
  ; usable: Edge "debloat" tools and fixed-version deployments delete the GUID
  ; keys but leave EdgeWebView\Application\<version>\msedgewebview2.exe. Probe
  ; that before the download below decides the machine has no WebView2 at all.
  ${If} $4 == ""
    StrCpy $5 ""
    FindFirst $0 $1 "$PROGRAMFILES\Microsoft\EdgeWebView\Application\*"
voxtrans_wv2_pf_loop:
    StrCmp $0 "" voxtrans_wv2_pf_done
    ${If} ${FileExists} "$PROGRAMFILES\Microsoft\EdgeWebView\Application\$1\msedgewebview2.exe"
      StrCpy $5 "$1"
      Goto voxtrans_wv2_pf_done
    ${EndIf}
    FindNext $0 $1
    Goto voxtrans_wv2_pf_loop
voxtrans_wv2_pf_done:
    FindClose $0
    ${If} $5 == ""
      FindFirst $0 $1 "$LOCALAPPDATA\Microsoft\EdgeWebView\Application\*"
voxtrans_wv2_pu_loop:
      StrCmp $0 "" voxtrans_wv2_pu_done
      ${If} ${FileExists} "$LOCALAPPDATA\Microsoft\EdgeWebView\Application\$1\msedgewebview2.exe"
        StrCpy $5 "$1"
        Goto voxtrans_wv2_pu_done
      ${EndIf}
      FindNext $0 $1
      Goto voxtrans_wv2_pu_loop
voxtrans_wv2_pu_done:
      FindClose $0
    ${EndIf}
    ${If} $5 != ""
      StrCpy $4 "$5"
      DetailPrint "WebView2 runtime found on disk ($5) without an EdgeUpdate registration"
    ${EndIf}
  ${EndIf}
'@) + "`r`n"

# Single-quoted on purpose: $\r$\n and $(...) are NSIS syntax, PowerShell must
# leave them alone.
$warnMessage = 'MessageBox MB_ICONEXCLAMATION|MB_OK "The WebView2 runtime is not registered on this machine, and installing it automatically failed.$\r$\n$\r$\nVoxTrans will finish installing. If the app does not start, install the WebView2 Evergreen Runtime from https://go.microsoft.com/fwlink/p/?LinkId=2124703 and run VoxTrans again."'

function Apply-WebView2Patch {
  param([string]$Text)

  if ($Text.Contains($marker)) { return $null }

  # Anchor 1: the install decision. Paired with the template comment that
  # follows it so it cannot match the earlier if-block that guards the HKCU read.
  $probeAnchor = '(?m)^  \$\{If\} \$4 == ""\r?\n    ; Webview2 installation\r?$'
  $probeHits = [regex]::Matches($Text, $probeAnchor)
  if ($probeHits.Count -ne 1) {
    throw "WebView2 install-decision anchor matched $($probeHits.Count) time(s), expected 1 - Tauri's NSIS template changed; update patch-nsis-webview2.ps1"
  }
  $evaluateProbe = { param($m) $probeBlock + $m.Value }.GetNewClosure()
  $Text = [regex]::Replace($Text, $probeAnchor, [System.Text.RegularExpressions.MatchEvaluator]$evaluateProbe)

  # Anchor 2: both hard aborts (bootstrapper download failure, bootstrapper exit).
  $abortAnchor = '(?m)^([ \t]*)Abort "\$\(webview2AbortError\)"\r?$'
  $abortHits = [regex]::Matches($Text, $abortAnchor)
  if ($abortHits.Count -ne 2) {
    throw "WebView2 abort anchor matched $($abortHits.Count) time(s), expected 2 - Tauri's NSIS template changed; update patch-nsis-webview2.ps1"
  }
  $evaluateAbort = {
    param($m)
    $indent = $m.Groups[1].Value
    $indent + $warnMessage + "`r`n" + $indent + 'Goto webview2_done'
  }.GetNewClosure()
  $Text = [regex]::Replace($Text, $abortAnchor, [System.Text.RegularExpressions.MatchEvaluator]$evaluateAbort)

  return $Text
}

function Assert-Patched {
  param([string]$Text)

  $problems = @()
  $markers = [regex]::Matches($Text, [regex]::Escape($marker)).Count
  if ($markers -ne 1) { $problems += "$markers probe marker(s), expected 1" }
  $aborts = [regex]::Matches($Text, [regex]::Escape('Abort "$(webview2AbortError)"')).Count
  if ($aborts -ne 0) { $problems += "$aborts hard abort(s) left, expected 0" }
  $labels = [regex]::Matches($Text, '(?m)^voxtrans_wv2_(pf|pu)_(loop|done):\r?$').Count
  if ($labels -ne 4) { $problems += "$labels probe label(s), expected 4" }
  $gotos = [regex]::Matches($Text, 'Goto webview2_done').Count
  if ($gotos -ne 3) { $problems += "$gotos 'Goto webview2_done' (1 stock + 2 failure paths), expected 3" }
  if ($problems.Count -gt 0) {
    throw "post-patch self-check failed: $($problems -join '; ')"
  }
}

function Invoke-SelfTest {
  $fixture = @(
    'Section WebView2',
    '  ${If} $4 == ""',
    '    ; Webview2 installation',
    '    ${If} $UpdateMode <> 1',
    '          Abort "$(webview2AbortError)"',
    '        Goto webview2_done',
    '          Abort "$(webview2AbortError)"',
    '    ${EndIf}',
    '    webview2_done:',
    '  ${EndIf}',
    'SectionEnd'
  ) -join "`r`n"

  $patched = Apply-WebView2Patch -Text $fixture
  if ($null -eq $patched) { throw "self-test: fixture was reported as already patched" }
  Assert-Patched -Text $patched

  # Break test: rename the anchor and the patch must throw, not silently produce
  # a template that still aborts (docs/lessons.md, rule 3).
  $broken = $fixture.Replace('; Webview2 installation', '; Webview2 setup')
  if ($broken -eq $fixture) { throw "self-test: mutation did not change the fixture" }
  $threw = $false
  try { $null = Apply-WebView2Patch -Text $broken } catch { $threw = $true }
  if (-not $threw) { throw "self-test: a renamed anchor was accepted (patch would silently miss)" }

  # Idempotence: running twice must not insert the probe twice.
  $again = Apply-WebView2Patch -Text $patched
  if ($null -ne $again) { throw "self-test: second pass was not a no-op" }

  Write-Host "Self-test OK: applies once, rejects a renamed anchor, idempotent on re-run"
}

if ($SelfTest) {
  Invoke-SelfTest
  exit 0
}

$installerNsi = Join-Path $NsisDir "installer.nsi"
if (-not (Test-Path -LiteralPath $installerNsi)) {
  throw "installer.nsi not found at: $installerNsi (run a tauri bundle build first, or pass -NsisDir)"
}

$utf8NoBom = New-Object System.Text.UTF8Encoding $false
$content = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $installerNsi), $utf8NoBom)

$patchedText = Apply-WebView2Patch -Text $content
if ($null -eq $patchedText) {
  Write-Host "installer.nsi already carries '$marker' - nothing to patch"
} else {
  Assert-Patched -Text $patchedText
  [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $installerNsi), $patchedText, $utf8NoBom)
  Write-Host "Patched installer.nsi: WebView2 is probed on disk, and a failed install warns instead of aborting"
}

if ($Build) {
  if (-not $Makensis) {
    $Makensis = Join-Path $env:LOCALAPPDATA "tauri\NSIS\Bin\makensis.exe"
  }
  if (-not (Test-Path -LiteralPath $Makensis)) {
    throw "makensis not found at '$Makensis' (run a tauri bundle build first, or pass -Makensis)"
  }
  Write-Host "Running makensis (no /DINCLUDE_CUDA_RUNTIME - this is the CPU installer)..."
  & $Makensis $installerNsi | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "makensis failed with exit code $LASTEXITCODE" }
  $outputExe = Join-Path $NsisDir "nsis-output.exe"
  if (-not (Test-Path $outputExe)) { throw "nsis-output.exe not produced" }
  Write-Host "Built: $outputExe ($((Get-Item $outputExe).Length) bytes)"
}
