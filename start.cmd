@echo off
if not exist "%~dp0orbit-launcher.exe" (
  echo orbit-launcher.exe is missing. Extract all files from the ZIP into one folder.
  echo If you work from the source code, build it first. See README.md.
  pause
  exit /b 1
)
start "" "%~dp0orbit-launcher.exe" --config "%~dp0data\config.json"
