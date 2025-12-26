@echo off
echo Deploying FlashSwapArbitrage to local fork...
echo.

REM Use one of Anvil's default test accounts
set PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80

forge script script/Deploy.s.sol:DeployScript --rpc-url http://localhost:8545 --broadcast --private-key %PRIVATE_KEY%

echo.
echo Contract deployed to fork! Check the output above for the address.
pause
