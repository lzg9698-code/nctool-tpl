@echo off
cd /d "%~dp0"

set EXE=target\release\nctool.exe
set PORT=8787

REM === 检查端口是否被占用 ===
netstat -ano | findstr ":%PORT% " | findstr LISTENING >nul
if not errorlevel 1 (
    echo [nctool] 端口 %PORT% 已被占用，可能已有 UI 服务在运行。
    echo [nctool] 请先关闭旧服务，或修改脚本中的 PORT 变量。
    echo.
    pause
    exit /b 1
)

REM === 检查/编译 release 二进制 ===
if not exist "%EXE%" (
    echo [nctool] 未找到 release 二进制，正在编译（首次约 1-3 分钟）...
    where cargo >nul 2>nul
    if errorlevel 1 (
        echo [nctool] 错误：未找到 cargo，请先安装 Rust 工具链。
        echo [nctool] 下载地址：https://rustup.rs
        echo.
        pause
        exit /b 1
    )
    cargo build --release -p nctool-cli
    if errorlevel 1 (
        echo.
        echo [nctool] 编译失败，请检查上方错误信息。
        echo.
        pause
        exit /b 1
    )
)

REM === 启动服务（--open：服务就绪后自动打开浏览器）===
echo [nctool] 正在启动 UI 服务，浏览器将自动打开...
echo [nctool] 地址：http://127.0.0.1:%PORT%
echo [nctool] 关闭此窗口或按 Ctrl+C 停止服务。
echo.

"%EXE%" ui --host 127.0.0.1 --port %PORT% --open

echo.
echo [nctool] 服务已停止。
echo.
pause