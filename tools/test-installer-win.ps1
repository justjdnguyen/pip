$ErrorActionPreference = 'Stop'
$root = (Get-Location).Path
$testId = [guid]::NewGuid().ToString('N')
$testRoot = Join-Path ([IO.Path]::GetTempPath()) "better-pip-installer-test-$testId"
$package = Join-Path $testRoot 'package'
$userRoot = Join-Path $testRoot 'user'
$registryRoot = "Software\BetterPiPInstallerTests\$testId"
$originalLocalAppData = $env:LOCALAPPDATA
[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($registryRoot).Dispose()
try {
  New-Item -ItemType Directory -Path (Join-Path $package 'tools') -Force | Out-Null
  Copy-Item packaging/windows/install.ps1 $package
  Copy-Item tools/install-host-win.ps1 (Join-Path $package 'tools')
  Copy-Item LICENSE $package
  Copy-Item PRIVACY.md $package
  [IO.File]::WriteAllText((Join-Path $package 'THIRD-PARTY-LICENSES.html'), 'installer test license notices')
  [IO.File]::WriteAllText((Join-Path $package 'better-pip-host.exe'), 'installer test placeholder')
  $env:LOCALAPPDATA = $userRoot
  # Redirect only this process's HKCU provider into an isolated test subkey.
  Remove-PSDrive HKCU
  New-PSDrive -Name HKCU -PSProvider Registry -Root "HKEY_CURRENT_USER\$registryRoot" | Out-Null
  & (Join-Path $package 'install.ps1')
  $manifestPath = Join-Path $userRoot 'BetterPiP\com.betterpip.host.json'
  $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
  if ($manifest.path -ne (Join-Path $userRoot 'BetterPiP\app\better-pip-host.exe')) { throw 'Wrong install path' }
  if (-not (Test-Path -LiteralPath $manifest.path)) { throw 'Binary not copied' }
  if (-not (Test-Path -LiteralPath (Join-Path (Split-Path $manifest.path) 'THIRD-PARTY-LICENSES.html'))) { throw 'License notices not installed' }
  if ($manifest.allowed_origins[0] -ne 'chrome-extension://fblfebboimckibidkphfchjlmohondgm/') { throw 'Wrong extension ID' }
  foreach ($browser in @('Software\Google\Chrome','Software\Chromium','Software\Microsoft\Edge','Software\BraveSoftware\Brave-Browser','Software\Vivaldi')) {
    $entry = Get-Item -LiteralPath "HKCU:\$browser\NativeMessagingHosts\com.betterpip.host"
    if ($entry.GetValue('') -ne $manifestPath) { throw "Bad registration for $browser" }
  }
  & (Join-Path $package 'install.ps1')
  & (Join-Path $package 'install.ps1') -Uninstall
  if ((Test-Path -LiteralPath $manifestPath) -or (Test-Path -LiteralPath $manifest.path)) { throw 'Uninstall left files' }
  Write-Output 'PASS: isolated Windows install, all five registrations, reinstall and uninstall'
} finally {
  $env:LOCALAPPDATA = $originalLocalAppData
  Remove-PSDrive HKCU -ErrorAction SilentlyContinue
  New-PSDrive -Name HKCU -PSProvider Registry -Root HKEY_CURRENT_USER | Out-Null
  [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($registryRoot, $false)
}
