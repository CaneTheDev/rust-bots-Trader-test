@echo off
echo ========================================
echo  Latency Auditor - Local Test
echo ========================================
echo.

cd /d "%~dp0"

echo [1/2] Building release binary...
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo BUILD FAILED!
    pause
    exit /b 1
)

echo.
echo [2/2] Running latency auditor...
echo.
cargo run --release

pause
