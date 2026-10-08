@echo off
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\stop_ui.ps1"
if errorlevel 1 (
    pause
    exit /b 1
)
cargo run -p nctool-cli --features nc-bundle --locked --target-dir target/parameters-nc -- --profile nc ui
set "NCTOOL_UI_EXIT_CODE=%ERRORLEVEL%"
pause
exit /b %NCTOOL_UI_EXIT_CODE%
