@echo off
echo Starting Base Mainnet Fork on localhost:8545...
echo.
echo This will create a local simulation of Base Mainnet with real-time data.
echo Press Ctrl+C to stop the fork.
echo.

REM Load the RPC URL from .env file
for /f "tokens=1,2 delims==" %%a in ('type .env ^| findstr /i "BASE_RPC_URL"') do set BASE_RPC_URL=%%b

if "%BASE_RPC_URL%"=="" (
    echo ERROR: BASE_RPC_URL not found in .env file
    echo Please add: BASE_RPC_URL=your_alchemy_or_drpc_url
    pause
    exit /b 1
)

echo Using RPC: %BASE_RPC_URL%
echo.

anvil --fork-url %BASE_RPC_URL% --fork-block-time 2 --chain-id 8453
