@echo off
echo ===============================================================
echo    MOONWELL LIQUIDATION BOT - Base Mainnet
echo ===============================================================
echo.
echo Mode: SIMULATION (no real trades until you add private key)
echo Target: Moonwell lending protocol on Base
echo.
echo Press Ctrl+C to stop.
echo.

cargo run --release --bin liquidator
