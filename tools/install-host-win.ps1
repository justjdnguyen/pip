# Registers the Better PiP companion app with Chromium browsers on Windows
# (per user, no admin needed), so the extension can launch it. Safe to run again.
#
# Usage: powershell -ExecutionPolicy Bypass -File tools\install-host-win.ps1 [path\to\better-pip-host.exe]
#   Defaults to the release build if there is one, else the debug build.
#   -Uninstall removes the registrations.
#
# Windows browsers find native hosts through the registry: the default value of
# HKCU\<browser key>\NativeMessagingHosts\com.betterpip.host is the path of a
# manifest JSON. Helium reads the Chromium key (checked in its chrome.dll).
param([string]$Bin = "", [switch]$Uninstall)
$ErrorActionPreference = "Stop"

$HostName = "com.betterpip.host"
$ExtensionId = "fblfebboimckibidkphfchjlmohondgm"
$Root = Split-Path -Parent $PSScriptRoot
$Keys = @(
  "Software\Google\Chrome",
  "Software\Chromium",              # Chromium, Helium
  "Software\Microsoft\Edge",
  "Software\BraveSoftware\Brave-Browser",
  "Software\Vivaldi"
)
$ManifestDir = Join-Path $env:LOCALAPPDATA "BetterPiP"
$Manifest = Join-Path $ManifestDir "$HostName.json"

if ($Uninstall) {
  foreach ($k in $Keys) {
    $path = "HKCU:\$k\NativeMessagingHosts\$HostName"
    if (Test-Path $path) { Remove-Item $path -Force; "removed: $k" }
  }
  if (Test-Path $Manifest) { Remove-Item $Manifest -Force }
  exit 0
}

if (-not $Bin) {
  $release = Join-Path $Root "companion\src-tauri\target\release\better-pip-host.exe"
  $debug = Join-Path $Root "companion\src-tauri\target\debug\better-pip-host.exe"
  if (Test-Path $release) { $Bin = $release }
  elseif (Test-Path $debug) { $Bin = $debug }
  else { throw "Build the app first: cd companion\src-tauri; cargo build --release" }
}
$Bin = (Resolve-Path $Bin).Path

New-Item -ItemType Directory -Force $ManifestDir | Out-Null
$json = [ordered]@{
  name = $HostName
  description = "Better PiP companion app"
  path = $Bin
  type = "stdio"
  allowed_origins = @("chrome-extension://$ExtensionId/")
} | ConvertTo-Json
# UTF-8 without BOM (browsers reject a BOM).
[System.IO.File]::WriteAllText($Manifest, $json, (New-Object System.Text.UTF8Encoding $false))

foreach ($k in $Keys) {
  $path = "HKCU:\$k\NativeMessagingHosts\$HostName"
  New-Item -Path $path -Force | Out-Null
  Set-ItemProperty -Path $path -Name "(default)" -Value $Manifest
  "registered: $k"
}
"Manifest: $Manifest"
"App: $Bin"
