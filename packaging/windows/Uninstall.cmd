@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" -Uninstall
if errorlevel 1 (
  echo Uninstall failed. Close browsers using Better PiP and try again.
  pause
  exit /b 1
)
pause
