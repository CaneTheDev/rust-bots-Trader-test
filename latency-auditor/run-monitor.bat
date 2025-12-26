@echo off
chcp 65001 >nul
echo ===============================================================
echo        ARBITRAGE PRICE MONITOR - Base Mainnet
echo ===============================================================
echo.
echo This monitors REAL Base Mainnet prices and calculates
echo potential arbitrage profits. NO MONEY SPENT.
echo.
echo Press Ctrl+C to stop early.
echo.

cd /d "%~dp0"
cargo run --release --bin monitor

pause
