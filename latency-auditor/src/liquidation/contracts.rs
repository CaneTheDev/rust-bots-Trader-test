//! Moonwell contract addresses and ABIs for Base Mainnet

/// Moonwell Comptroller - manages all markets
pub const COMPTROLLER: &str = "0xfBb21d0380beE3312B33c4353c8936a0F13EF26C";

/// Moonwell Oracle (Chainlink-based)
pub const ORACLE: &str = "0xEC942bE8A8114bFD0396A5052c36027f2cA6a9d0";

/// Multi-Reward Distributor
pub const REWARD_DISTRIBUTOR: &str = "0xe9005b078701e2A0948D2EaC43010D35870Ad9d2";

/// Moonwell Views - helper contract for batch queries
pub const MOONWELL_VIEWS: &str = "0x6834770aba6c2028f448e3259ddee4bcb879d459";

/// Multicall3 - for batching multiple calls into one
pub const MULTICALL3: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

/// Balancer Vault for flash loans
pub const BALANCER_VAULT: &str = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";

/// WETH on Base
pub const WETH: &str = "0x4200000000000000000000000000000000000006";

/// USDC on Base  
pub const USDC: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

/// Moonwell Markets on Base (mTokens)
pub mod markets {
    pub const MW_WETH: &str = "0x628ff693426583D9a7FB391E54366292F509D457";
    pub const MW_USDC: &str = "0xEdc817A28E8B93B03976FBd4a3dDBc9f7D176c22";
    pub const MW_CBETH: &str = "0x3bf93770f2d4a794c3d9EBEfBAeBAE2a8f09A5E5";
    pub const MW_WSTETH: &str = "0x627Fe393Bc6EdDA28e99AE648fD6fF362514304b";
    pub const MW_RETH: &str = "0xcb1dacd30638ae38f2b94ea64f066045b7d45f44";
    pub const MW_DAI: &str = "0x73b06D8d18De422E269645eaCe15400DE7462417";
    pub const MW_AERO: &str = "0x73902f619CEB9B31FD8EFecf435CbDf89E369Ba6";
    pub const MW_CBBTC: &str = "0xF877ACaFA28c19b96727966690b2f44d35aD5976";
    pub const MW_WEETH: &str = "0xb8051464C8c92209C92F3a4CD9C73746C4c3CFb3";
    pub const MW_EURC: &str = "0xb682c840B5F4FC58B20769E691A6fa1305A501a2";
    pub const MW_USDS: &str = "0xb6419c6C2e60c4025D6D06eE4F913ce89425a357";
    
    pub fn all() -> Vec<&'static str> {
        vec![
            MW_WETH, MW_USDC, MW_CBETH, MW_WSTETH, MW_RETH,
            MW_DAI, MW_AERO, MW_CBBTC, MW_WEETH, MW_EURC, MW_USDS
        ]
    }
    
    pub fn symbol(address: &str) -> &'static str {
        match address.to_lowercase().as_str() {
            a if a == MW_WETH.to_lowercase() => "mWETH",
            a if a == MW_USDC.to_lowercase() => "mUSDC",
            a if a == MW_CBETH.to_lowercase() => "mcbETH",
            a if a == MW_WSTETH.to_lowercase() => "mwstETH",
            a if a == MW_RETH.to_lowercase() => "mrETH",
            a if a == MW_DAI.to_lowercase() => "mDAI",
            a if a == MW_AERO.to_lowercase() => "mAERO",
            a if a == MW_CBBTC.to_lowercase() => "mcbBTC",
            a if a == MW_WEETH.to_lowercase() => "mweETH",
            a if a == MW_EURC.to_lowercase() => "mEURC",
            a if a == MW_USDS.to_lowercase() => "mUSDS",
            _ => "unknown",
        }
    }
}

/// Function selectors for contract calls
pub mod selectors {
    // Comptroller
    pub const GET_ALL_MARKETS: &str = "0xb0772d0b";  // getAllMarkets()
    pub const GET_ASSETS_IN: &str = "0xabfceffc";    // getAssetsIn(address)
    pub const ACCOUNT_LIQUIDITY: &str = "0x5ec88c79"; // getAccountLiquidity(address)
    pub const MARKETS: &str = "0x8e8f294b";          // markets(address)
    
    // mToken
    pub const BORROW_BALANCE_STORED: &str = "0x95dd9193"; // borrowBalanceStored(address)
    pub const BALANCE_OF: &str = "0x70a08231";            // balanceOf(address)
    pub const EXCHANGE_RATE_STORED: &str = "0x182df0f5";  // exchangeRateStored()
    pub const UNDERLYING: &str = "0x6f307dc3";            // underlying()
    pub const BORROW_INDEX: &str = "0xaa5af0fd";          // borrowIndex()
    
    // Oracle
    pub const GET_UNDERLYING_PRICE: &str = "0xfc57d4df"; // getUnderlyingPrice(address)
    
    // Liquidation
    pub const LIQUIDATE_BORROW: &str = "0xf5e3c462"; // liquidateBorrow(borrower, repayAmount, mTokenCollateral)
}

/// Event signatures for monitoring
pub mod events {
    // Borrow event: Borrow(address borrower, uint256 borrowAmount, uint256 accountBorrows, uint256 totalBorrows)
    pub const BORROW: &str = "0x13ed6866d4e1ee6da46f845c46d7e54120883d75c5ea9a2dacc1c4ca8984ab80";
    
    // RepayBorrow event
    pub const REPAY_BORROW: &str = "0x1a2a22cb034d26d1854bdc6666a5b91688a9e6e9e5a8e8c8e8e8e8e8e8e8e8e8";
    
    // LiquidateBorrow event
    pub const LIQUIDATE_BORROW: &str = "0x298637f684da70674f26509b10f07ec2fbc77a335ab1e7d6215a4b2484d8bb52";
    
    // Mint (deposit): Mint(address minter, uint256 mintAmount, uint256 mintTokens)
    pub const MINT: &str = "0x4c209b5fc8ad50758f13e2e1088ba56a560dff690a1c6fef26394f4c03821c4f";
    
    // Redeem (withdraw)
    pub const REDEEM: &str = "0xe5b754fb1abb7f01b499791d0b820ae3b6af3424ac1c59768edb53f4ec31a929";
    
    // AccrueInterest
    pub const ACCRUE_INTEREST: &str = "0x4dec04e750ca11537cabcd8a9eab06494de08da3735bc8871cd41250e190bc04";
    
    // MarketEntered(address mToken, address account) - THE KEY EVENT
    // This is emitted when a user enables collateral for a market
    // Anyone who can be liquidated MUST have entered at least one market
    pub const MARKET_ENTERED: &str = "0x3ab23ab0d51cccc0c3085aec51f99228625aa1a922b3a8ca89a26b0f2027a1a5";
}
