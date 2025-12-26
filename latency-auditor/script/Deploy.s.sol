// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../contracts/FlashSwapArbitrage.sol";

contract DeployScript is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        
        vm.startBroadcast(deployerPrivateKey);
        
        FlashSwapArbitrage arbitrage = new FlashSwapArbitrage();
        
        console.log("FlashSwapArbitrage deployed at:", address(arbitrage));
        
        vm.stopBroadcast();
    }
}
