@echo off
cd /d "%~dp0"
cargo run -p nctool-cli -- ui
pause
