@echo off
cd /d "%~dp0"

set FRONTEND=gui\frontend
set EXE=target\debug\nctool-gui.exe

REM === 检查工具链 ===
where cargo >nul 2>nul
if errorlevel 1 (
    echo [nctool] 错误：未找到 cargo，请先安装 Rust 工具链。
    echo [nctool] 下载地址：https://rustup.rs
    echo.
    pause
    exit /b 1
)

where npm >nul 2>nul
if errorlevel 1 (
    echo [nctool] 错误：未找到 npm，构建前端需要 Node.js。
    echo [nctool] 下载地址：https://nodejs.org
    echo.
    pause
    exit /b 1
)

REM === 构建前端 ===
REM GUI 的前端资源是「编译期嵌入」进二进制的，所以必须先产出 gui\frontend\dist，
REM 否则改过前端源码也不会生效（dist 过期不会报错，只会静默跑旧界面）。
REM 成功时静默、失败时才打印日志：vite 的输出含非 ASCII 的装饰符号（对勾/竖线），
REM 在 GBK 控制台下会显示成乱码；重定向掉既消除乱码又让输出更干净。
echo [nctool] 正在构建前端...
set FELOG=%TEMP%\nctool_fe_build.log
call npm --prefix "%FRONTEND%" run build >"%FELOG%" 2>&1
if errorlevel 1 (
    echo.
    echo [nctool] 前端构建失败，日志（%FELOG%）：
    echo ------------------------------------------------------------
    type "%FELOG%"
    echo ------------------------------------------------------------
    echo.
    pause
    exit /b 1
)

REM === 编译 GUI ===
REM 必须带 custom-protocol：不带时会被判定为 dev 模式，应用去连 devUrl
REM （http://localhost:1420），没有 Vite dev server 就是白屏。
echo [nctool] 正在编译 GUI（首次约 1-5 分钟，之后增量很快）...
cargo build -p nctool-gui --features custom-protocol
if errorlevel 1 (
    echo.
    echo [nctool] 编译失败，请检查上方错误信息。
    echo.
    pause
    exit /b 1
)

if not exist "%EXE%" (
    echo [nctool] 错误：未生成 %EXE%
    echo.
    pause
    exit /b 1
)

echo [nctool] 正在启动 nctool 桌面 GUI...
echo.

REM 用 start 脱离本控制台：关掉这个黑窗口不会关掉 GUI。
REM 传绝对路径（%~dp0 是脚本所在目录）——start 对相对路径的解析依赖当前目录，不稳。
start "" "%~dp0%EXE%"

echo [nctool] 已启动。关闭此窗口不会关闭 GUI。
REM 停顿 3 秒让你能看到上面的提示。这里必须用 timeout.exe 的绝对路径：
REM Git 的 usr\bin 里也有个 GNU timeout，且 PATH 顺序常排在 System32 之前，
REM 若被它抢先会报 "timeout: invalid time interval '/t'"。
"%SystemRoot%\System32\timeout.exe" /t 3 >nul 2>nul
exit /b 0
