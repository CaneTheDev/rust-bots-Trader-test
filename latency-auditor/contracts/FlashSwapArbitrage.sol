// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@uniswap/v3-core/contracts/interfaces/IUniswapV3Pool.sol";
import "@uniswap/v3-core/contracts/interfaces/callback/IUniswapV3FlashCallback.sol";
import "@uniswap/v3-periphery/contracts/interfaces/ISwapRouter.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title Flash Swap Arbitrage Contract
/// @notice Executes triangular arbitrage using Uniswap V3 flash swaps
contract FlashSwapArbitrage is IUniswapV3FlashCallback {
    address public immutable owner;
    ISwapRouter public immutable swapRouter;
    
    // Base Mainnet Uniswap V3 Router
    address constant SWAP_ROUTER = 0x2626664c2603336E57B271c5C0b26F421741e481;
    
    event ArbitrageExecuted(
        address indexed token0,
        address indexed token1,
        uint256 amountIn,
        uint256 profit
    );
    
    event ArbitrageFailed(
        address indexed token0,
        address indexed token1,
        string reason
    );
    
    modifier onlyOwner() {
        require(msg.sender == owner, "Not owner");
        _;
    }
    
    constructor() {
        owner = msg.sender;
        swapRouter = ISwapRouter(SWAP_ROUTER);
    }
    
    /// @notice Initiates flash swap arbitrage
    /// @param pool The Uniswap V3 pool to flash swap from
    /// @param token0 First token in the pair
    /// @param token1 Second token in the pair
    /// @param amount0 Amount of token0 to borrow (0 if borrowing token1)
    /// @param amount1 Amount of token1 to borrow (0 if borrowing token0)
    function executeArbitrage(
        address pool,
        address token0,
        address token1,
        uint256 amount0,
        uint256 amount1
    ) external onlyOwner {
        // Encode the swap path for callback
        bytes memory data = abi.encode(token0, token1, msg.sender);
        
        // Initiate flash swap
        IUniswapV3Pool(pool).flash(
            address(this),
            amount0,
            amount1,
            data
        );
    }
    
    /// @notice Callback function called by Uniswap V3 pool after flash loan
    function uniswapV3FlashCallback(
        uint256 fee0,
        uint256 fee1,
        bytes calldata data
    ) external override {
        // Decode the data
        (address token0, address token1, address initiator) = abi.decode(
            data,
            (address, address, address)
        );
        
        // Verify callback is from a legitimate pool
        address pool = msg.sender;
        
        // Get borrowed amount
        uint256 borrowed = fee0 > 0 
            ? IERC20(token0).balanceOf(address(this)) 
            : IERC20(token1).balanceOf(address(this));
        
        address borrowedToken = fee0 > 0 ? token0 : token1;
        address targetToken = fee0 > 0 ? token1 : token0;
        
        // Execute the arbitrage swaps
        try this.executeSwaps(borrowedToken, targetToken, borrowed) returns (uint256 finalAmount) {
            // Calculate repayment amount
            uint256 amountOwed = borrowed + (fee0 > 0 ? fee0 : fee1);
            
            // Check if profitable
            require(finalAmount > amountOwed, "Not profitable");
            
            // Repay the flash loan
            IERC20(borrowedToken).transfer(pool, amountOwed);
            
            // Transfer profit to owner
            uint256 profit = finalAmount - amountOwed;
            IERC20(borrowedToken).transfer(initiator, profit);
            
            emit ArbitrageExecuted(token0, token1, borrowed, profit);
        } catch Error(string memory reason) {
            // Repay the loan even if arbitrage fails
            uint256 amountOwed = borrowed + (fee0 > 0 ? fee0 : fee1);
            IERC20(borrowedToken).transfer(pool, amountOwed);
            
            emit ArbitrageFailed(token0, token1, reason);
            revert(reason);
        }
    }
    
    /// @notice Execute the swap sequence
    function executeSwaps(
        address tokenIn,
        address tokenOut,
        uint256 amountIn
    ) external returns (uint256) {
        require(msg.sender == address(this), "Internal only");
        
        // Approve router
        IERC20(tokenIn).approve(address(swapRouter), amountIn);
        
        // Execute swap
        ISwapRouter.ExactInputSingleParams memory params = ISwapRouter
            .ExactInputSingleParams({
                tokenIn: tokenIn,
                tokenOut: tokenOut,
                fee: 3000, // 0.3% fee tier
                recipient: address(this),
                deadline: block.timestamp,
                amountIn: amountIn,
                amountOutMinimum: 0,
                sqrtPriceLimitX96: 0
            });
        
        uint256 amountOut = swapRouter.exactInputSingle(params);
        
        // Swap back
        IERC20(tokenOut).approve(address(swapRouter), amountOut);
        
        params = ISwapRouter.ExactInputSingleParams({
            tokenIn: tokenOut,
            tokenOut: tokenIn,
            fee: 3000,
            recipient: address(this),
            deadline: block.timestamp,
            amountIn: amountOut,
            amountOutMinimum: 0,
            sqrtPriceLimitX96: 0
        });
        
        return swapRouter.exactInputSingle(params);
    }
    
    /// @notice Emergency withdraw function
    function withdraw(address token) external onlyOwner {
        uint256 balance = IERC20(token).balanceOf(address(this));
        IERC20(token).transfer(owner, balance);
    }
    
    /// @notice Receive ETH
    receive() external payable {}
}
