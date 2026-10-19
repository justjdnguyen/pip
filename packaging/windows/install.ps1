# Per-user companion installation from an extracted release archive.
param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'
$InstallDir = Join-Path $env:LOCALAPPDATA 'BetterPiP\app'
$InstalledBin = Join-Path $InstallDir 'better-pip-host.exe'
$Register = Join-Path $PSScriptRoot 'tools\install-host-win.ps1'

if ($Uninstall) {
  # Remove the binary first so a running host produces a clear, retryable failure.
  if (Test-Path -LiteralPath $InstalledBin) { Remove-Item -LiteralPath $InstalledBin -Force }
  foreach ($name in @('LICENSE', 'THIRD-PARTY-LICENSES.html', 'PRIVACY.md')) {
    $notice = Join-Path $InstallDir $name
    if (Test-Path -LiteralPath $notice) { Remove-Item -LiteralPath $notice -Force }
  }
  & $Register -Uninstall
  Write-Output 'Better PiP companion uninstalled. Remove the browser extension separately.'
  exit 0
}

$SourceBin = Join-Path $PSScriptRoot 'better-pip-host.exe'
if (-not (Test-Path -LiteralPath $SourceBin -PathType Leaf)) {
  throw 'Extract the complete companion ZIP before running Install.cmd.'
}
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
# Copy before registering. If a running host locks the existing binary, retry after
# closing the browser; the old registration and installation are left intact.
Copy-Item -LiteralPath $SourceBin -Destination $InstalledBin -Force
foreach ($name in @('LICENSE', 'THIRD-PARTY-LICENSES.html', 'PRIVACY.md')) {
  Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $InstallDir -Force
}
& $Register -Bin $InstalledBin
Write-Output 'Better PiP companion installed for this user.'
