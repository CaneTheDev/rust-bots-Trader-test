//! Pool Configuration for Arbitrage Monitor
//! 
//! EDIT THIS FILE TO ADD/REMOVE TOKENS AND POOLS
//! The monitor will automatically pick up changes on restart.
//!
//! IMPORTANT: All Base pools have WETH as token0, so invert_price = true

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PoolType {
    UniswapV3,   // Uses slot0()
    V2Style,     // Uses getReserves()
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub address: &'static str,
    pub dex_name: &'static str,
    pub pair: &'static str,
    pub fee_percent: f64,
    pub pool_type: PoolType,
    pub token0_decimals: u8,
    pub token1_decimals: u8,
    pub invert_price: bool,  // TRUE if WETH is token0 (most Base pools)
}

pub fn get_pool_configs() -> Vec<PoolConfig> {
    vec![
        // ════════════════════════════════════════════════════════════════════
        // VIRTUAL/WETH - HOT! $3.2M daily volume (AI Agent token)
        // NOTE: Aerodrome CL pools use slot0() like Uniswap V3
        // ════════════════════════════════════════════════════════════════════
        PoolConfig {
            address: "0x3f0296BF652e19bca772EC3dF08b32732F93014A",
            dex_name: "Aero VIRTUAL",
            pair: "VIRTUAL/WETH",
            fee_percent: 0.30,
            pool_type: PoolType::UniswapV3,  // CL pool uses slot0()
            token0_decimals: 18,
            token1_decimals: 18,
            invert_price: true,
        },
        // REMOVED: Uni V3 VIRTUAL 1% fee pool - too expensive for arbitrage
        // PoolConfig {
        //     address: "0x9c087Eb773291e50CF6c6a90ef0F4500e349B903",
        //     dex_name: "Uni V3 VIRTUAL",
        //     pair: "VIRTUAL/WETH",
        //     fee_percent: 1.0,
        //     ...
        // },
        PoolConfig {
            address: "0x66660fBCd3829932586A5AF62093eE75faa91F9F",
            dex_name: "PCS VIRTUAL",
            pair: "VIRTUAL/WETH",
            fee_percent: 0.25,
            pool_type: PoolType::UniswapV3,  // PCS V3 uses slot0()
            token0_decimals: 18,
            token1_decimals: 18,
            invert_price: true,
        },
        
        // ════════════════════════════════════════════════════════════════════
        // AERO/WETH - Aerodrome governance, $1M+ volume
        // ════════════════════════════════════════════════════════════════════
        PoolConfig {
            address: "0x82321f3BEB69f503380D6B233857d5C43562e2D0",
            dex_name: "Aero AERO",
            pair: "AERO/WETH",
            fee_percent: 0.30,
            pool_type: PoolType::UniswapV3,  // CL pool uses slot0()
            token0_decimals: 18,
            token1_decimals: 18,
            invert_price: true,
        },
        // REMOVED: Uni V3 AERO 1% fee pool - too expensive for arbitrage
        // PoolConfig {
        //     address: "0x3d5D143381916280ff91407FeBEB52f2b60f33Cf",
        //     dex_name: "Uni V3 AERO",
        //     pair: "AERO/WETH",
        //     fee_percent: 1.0,
        //     ...
        // },
        PoolConfig {
            address: "0x20CB8f872ae894F7c9e32e621C186e5AFCe82Fd0",
            dex_name: "PCS AERO",
            pair: "AERO/WETH",
            fee_percent: 0.25,
            pool_type: PoolType::UniswapV3,  // PCS V3 uses slot0()
            token0_decimals: 18,
            token1_decimals: 18,
            invert_price: true,
        },
        
        // ════════════════════════════════════════════════════════════════════
        // AERO/USDC - High liquidity $33M, good volume
        // NOTE: In these pools, USDC is token0, AERO is token1
        // ════════════════════════════════════════════════════════════════════
        PoolConfig {
            address: "0x6cDcb1C4A4D1C3C6d054b27AC5B77e89eAFb971d",
            dex_name: "Aero AERO/USDC",
            pair: "AERO/USDC",
            fee_percent: 0.30,
            pool_type: PoolType::V2Style,  // This one uses reserve0/reserve1
            token0_decimals: 6,   // USDC is token0
            token1_decimals: 18,  // AERO is token1
            invert_price: false,
        },
        // REMOVED: Uni V3 AERO/USDC 1% fee pool - too expensive for arbitrage
        // PoolConfig {
        //     address: "0xE5B5f522E98B5a2baAe212d4dA66b865B781DB97",
        //     dex_name: "Uni V3 AERO/USDC",
        //     pair: "AERO/USDC",
        //     fee_percent: 1.0,
        //     ...
        // },
        PoolConfig {
            address: "0x29a857629E58C531f351Bb7fbd86aa46Ef6aF0B7",
            dex_name: "PCS AERO/USDC",
            pair: "AERO/USDC",
            fee_percent: 0.25,
            pool_type: PoolType::UniswapV3,  // PCS V3
            token0_decimals: 6,   // USDC is token0
            token1_decimals: 18,  // AERO is token1
            invert_price: false,
        },
        
        // ════════════════════════════════════════════════════════════════════
        // doginme/WETH - REMOVED: Only 1 low-fee pool available
        // Need at least 2 pools for arbitrage
        // ════════════════════════════════════════════════════════════════════
        
        // ════════════════════════════════════════════════════════════════════
        // VIRTUAL/USDC - REMOVED: Only 1 low-fee pool available
        // Need at least 2 pools for arbitrage
        // ════════════════════════════════════════════════════════════════════
        
        // ════════════════════════════════════════════════════════════════════
        // AERO/cbBTC - REMOVED: Only 1 pool available
        // Need at least 2 pools for arbitrage
        // ════════════════════════════════════════════════════════════════════
        
        // ════════════════════════════════════════════════════════════════════
        // Additional VIRTUAL/WETH pools for more arb coverage
        // ════════════════════════════════════════════════════════════════════
        PoolConfig {
            address: "0xC200F21EfE67c7F41B81A854c26F9cdA80593065",
            dex_name: "Aero2 VIRTUAL",
            pair: "VIRTUAL/WETH",
            fee_percent: 0.30,
            pool_type: PoolType::UniswapV3,  // CL pool
            token0_decimals: 18,
            token1_decimals: 18,
            invert_price: true,
        },
        // NOTE: Removed Uni V3-2 VIRTUAL (0xE31c372a7Af875b3B5E0F3713B17ef51556da667) - inactive/incompatible pool
        
        // ════════════════════════════════════════════════════════════════════
        // Additional AERO pools
        // ════════════════════════════════════════════════════════════════════
        PoolConfig {
            address: "0x7f670f78B17dEC44d5Ef68a48740b6f8849cc2e6",
            dex_name: "Aero2 AERO",
            pair: "AERO/WETH",
            fee_percent: 0.30,
            pool_type: PoolType::V2Style,
            token0_decimals: 18,
            token1_decimals: 18,
            invert_price: true,
        },
        // REMOVED: Uni V3-2 AERO 1% fee pool - too expensive for arbitrage
        // PoolConfig {
        //     address: "0x0D5959a52E7004b601f0bE70618D01aC3cDce976",
        //     dex_name: "Uni V3-2 AERO",
        //     pair: "AERO/WETH",
        //     fee_percent: 1.0,
        //     ...
        // },
        
        // ════════════════════════════════════════════════════════════════════
        // WETH/USDC - Keep for ETH price reference (used in triangular arb later)
        // ════════════════════════════════════════════════════════════════════
        PoolConfig {
            address: "0xd0b53D9277642d899DF5C87A3966A349A798F224",
            dex_name: "Uni V3 0.05%",
            pair: "WETH/USDC",
            fee_percent: 0.05,
            pool_type: PoolType::UniswapV3,
            token0_decimals: 18,
            token1_decimals: 6,
            invert_price: false,
        },
    ]
}

pub fn get_monitored_pairs() -> Vec<String> {
    let configs = get_pool_configs();
    let mut pairs: Vec<String> = configs.iter().map(|p| p.pair.to_string()).collect();
    pairs.sort();
    pairs.dedup();
    pairs
}

pub fn print_pool_summary() {
    let configs = get_pool_configs();
    let pairs = get_monitored_pairs();
    
    println!("MONITORED POOLS ({} pools across {} pairs):", configs.len(), pairs.len());
    println!();
    
    for pair in &pairs {
        let pools: Vec<_> = configs.iter().filter(|p| p.pair == pair).collect();
        println!("  {} ({} DEXes):", pair, pools.len());
        for pool in pools {
            let pool_type = match pool.pool_type {
                PoolType::UniswapV3 => "V3",
                PoolType::V2Style => "V2",
            };
            println!("    - {} | {}% fee | {}", pool.dex_name, pool.fee_percent, pool_type);
        }
    }
    println!();
}
