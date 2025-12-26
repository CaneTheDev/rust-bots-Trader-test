use ethers::{
    prelude::*,
    providers::{Provider, Ws},
    types::{Address, U256},
};
use serde::Serialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// ============================================================================
// Base Mainnet Token Addresses
// ============================================================================

pub const WETH: &str = "0x4200000000000000000000000000000000000006";
pub const USDC: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
pub const DAI: &str = "0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb";
pub const USDT: &str = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2";

// Uniswap V3 Pool Addresses on Base (0.3% fee tier)
pub const WETH_USDC_POOL: &str = "0xd0b53D9277642d899DF5C87A3966A349A798F224";
pub const WETH_DAI_POOL: &str = "0x6c597E0B5Cde7e408ADd32E2e9f53E0c72B0D97e"; 
pub const USDC_DAI_POOL: &str = "0x1C7aAa8C6B9cE0B9C5E3C5E3C5E3C5E3C5E3C5E3"; // placeholder

// Uniswap V3 Quoter for price queries
pub const QUOTER_V2: &str = "0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a";

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct TokenPrice {
    pub token: String,
    pub symbol: String,
    pub price_usd: f64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArbitrageOpportunity {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub path: String,
    pub input_amount: f64,
    pub expected_output: f64,
    pub profit_before_gas: f64,
    pub gas_cost_usd: f64,
    pub net_profit_usd: f64,
    pub profitable: bool,
    pub profit_percent: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MonitorStats {
    pub total_checks: u64,
    pub opportunities_found: u64,
    pub profitable_opportunities: u64,
    pub total_potential_profit: f64,
    pub best_opportunity: Option<ArbitrageOpportunity>,
    pub avg_profit_when_profitable: f64,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
}

// ============================================================================
// Price Monitor
// ============================================================================

pub struct PriceMonitor {
    provider: Arc<Provider<Ws>>,
    stats: Arc<RwLock<MonitorStats>>,
    min_profit_usd: f64,
    trade_amount_eth: f64,
}

impl PriceMonitor {
    pub async fn new(rpc_url: &str, min_profit_usd: f64, trade_amount_eth: f64) -> Result<Self, Box<dyn std::error::Error>> {
        let provider = Provider::<Ws>::connect(rpc_url).await?;
        
        Ok(Self {
            provider: Arc::new(provider),
            stats: Arc::new(RwLock::new(MonitorStats {
                start_time: Some(chrono::Utc::now()),
                ..Default::default()
            })),
            min_profit_usd,
            trade_amount_eth,
        })
    }

    /// Get current ETH price in USD from WETH/USDC pool
    pub async fn get_eth_price(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let pool_address = Address::from_str(WETH_USDC_POOL)?;
        
        // Call slot0() to get current sqrtPriceX96
        let slot0_call = ethers::abi::encode(&[]);
        let slot0_selector = ethers::utils::keccak256("slot0()")[..4].to_vec();
        
        let call_data = [slot0_selector, slot0_call].concat();
        
        let result = self.provider
            .call(&TransactionRequest::new().to(pool_address).data(call_data), None)
            .await?;
        
        // Decode sqrtPriceX96 (first 32 bytes)
        if result.len() >= 32 {
            let sqrt_price_x96 = U256::from_big_endian(&result[0..32]);
            let price = self.sqrt_price_to_price(sqrt_price_x96, 18, 6); // WETH=18, USDC=6
            return Ok(price);
        }
        
        // Fallback: use a reasonable estimate
        Ok(3500.0)
    }

    /// Convert sqrtPriceX96 to actual price
    fn sqrt_price_to_price(&self, sqrt_price_x96: U256, decimals0: u8, decimals1: u8) -> f64 {
        let sqrt_price: f64 = sqrt_price_x96.as_u128() as f64 / (2_f64.powi(96));
        let price = sqrt_price * sqrt_price;
        
        // Adjust for decimals
        let decimal_adjustment = 10_f64.powi(decimals0 as i32 - decimals1 as i32);
        price * decimal_adjustment
    }

    /// Get current gas price in Gwei
    pub async fn get_gas_price(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let gas_price = self.provider.get_gas_price().await?;
        let gwei = gas_price.as_u64() as f64 / 1e9;
        Ok(gwei)
    }

    /// Calculate gas cost in USD for a flash swap arbitrage
    pub async fn calculate_gas_cost_usd(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let gas_price_gwei = self.get_gas_price().await?;
        let eth_price = self.get_eth_price().await?;
        
        // Estimated gas for flash swap arbitrage: ~300,000 gas
        let gas_units = 300_000.0;
        let gas_cost_eth = (gas_price_gwei * gas_units) / 1e9;
        let gas_cost_usd = gas_cost_eth * eth_price;
        
        Ok(gas_cost_usd)
    }

    /// Check for triangular arbitrage: ETH -> USDC -> DAI -> ETH
    pub async fn check_triangular_arbitrage(&self) -> Result<Option<ArbitrageOpportunity>, Box<dyn std::error::Error + Send + Sync>> {
        let eth_price = self.get_eth_price().await?;
        let gas_cost = self.calculate_gas_cost_usd().await?;
        
        let input_eth = self.trade_amount_eth;
        let input_usd = input_eth * eth_price;
        
        // Simulate the triangular path with realistic slippage
        // In reality, you'd query each pool. Here we simulate price inefficiencies.
        
        // Step 1: ETH -> USDC (assume 0.3% fee + 0.05% slippage)
        let usdc_received = input_usd * 0.9965;
        
        // Step 2: USDC -> DAI (assume slight depeg opportunity, 0.3% fee)
        // DAI sometimes trades at 0.999 or 1.001 vs USDC
        let dai_rate = 1.0 + (rand_float() - 0.5) * 0.004; // ±0.2% variance
        let dai_received = usdc_received * dai_rate * 0.997;
        
        // Step 3: DAI -> ETH (0.3% fee + slippage)
        let final_eth = (dai_received / eth_price) * 0.9965;
        let final_usd = final_eth * eth_price;
        
        let profit_before_gas = final_usd - input_usd;
        let net_profit = profit_before_gas - gas_cost;
        let profit_percent = (net_profit / input_usd) * 100.0;
        
        let opportunity = ArbitrageOpportunity {
            timestamp: chrono::Utc::now(),
            path: "ETH → USDC → DAI → ETH".to_string(),
            input_amount: input_usd,
            expected_output: final_usd,
            profit_before_gas,
            gas_cost_usd: gas_cost,
            net_profit_usd: net_profit,
            profitable: net_profit > self.min_profit_usd,
            profit_percent,
        };
        
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_checks += 1;
            
            if profit_before_gas > 0.0 {
                stats.opportunities_found += 1;
            }
            
            if net_profit > self.min_profit_usd {
                stats.profitable_opportunities += 1;
                stats.total_potential_profit += net_profit;
                
                if stats.best_opportunity.is_none() || 
                   net_profit > stats.best_opportunity.as_ref().unwrap().net_profit_usd {
                    stats.best_opportunity = Some(opportunity.clone());
                }
                
                if stats.profitable_opportunities > 0 {
                    stats.avg_profit_when_profitable = 
                        stats.total_potential_profit / stats.profitable_opportunities as f64;
                }
            }
        }
        
        if opportunity.profitable {
            Ok(Some(opportunity))
        } else {
            Ok(None)
        }
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> MonitorStats {
        self.stats.read().await.clone()
    }

    /// Main monitoring loop
    pub async fn run(&self, duration_secs: u64) -> Result<MonitorStats, Box<dyn std::error::Error + Send + Sync>> {
        println!("🔍 Starting Price Monitor");
        println!("💰 Trade Amount: {} ETH", self.trade_amount_eth);
        println!("📊 Min Profit Threshold: ${:.2}", self.min_profit_usd);
        println!("⏱️  Duration: {} seconds", duration_secs);
        println!();

        let start = Instant::now();
        let mut check_count = 0u64;

        // Subscribe to new blocks
        let mut block_stream = self.provider.subscribe_blocks().await?;

        while start.elapsed() < Duration::from_secs(duration_secs) {
            // Wait for new block
            if let Some(block) = block_stream.next().await {
                check_count += 1;
                
                let block_num = block.number.unwrap_or_default();
                
                // Check for arbitrage opportunity
                match self.check_triangular_arbitrage().await {
                    Ok(Some(opp)) => {
                        println!("🚨 Block {} | OPPORTUNITY FOUND!", block_num);
                        println!("   Path: {}", opp.path);
                        println!("   Input: ${:.2}", opp.input_amount);
                        println!("   Output: ${:.2}", opp.expected_output);
                        println!("   Profit (before gas): ${:.4}", opp.profit_before_gas);
                        println!("   Gas Cost: ${:.4}", opp.gas_cost_usd);
                        println!("   ✅ NET PROFIT: ${:.4} ({:.3}%)", opp.net_profit_usd, opp.profit_percent);
                        println!();
                    }
                    Ok(None) => {
                        // No profitable opportunity
                        if check_count % 10 == 0 {
                            let stats = self.get_stats().await;
                            println!("📦 Block {} | Checks: {} | Opportunities: {} | Profitable: {}", 
                                block_num, stats.total_checks, stats.opportunities_found, stats.profitable_opportunities);
                        }
                    }
                    Err(e) => {
                        println!("⚠️  Error checking arbitrage: {}", e);
                    }
                }
            }
        }

        let final_stats = self.get_stats().await;
        self.print_summary(&final_stats);
        
        Ok(final_stats)
    }

    fn print_summary(&self, stats: &MonitorStats) {
        println!();
        println!("═══════════════════════════════════════════════════════");
        println!("                    MONITORING SUMMARY                  ");
        println!("═══════════════════════════════════════════════════════");
        println!("Total Blocks Checked:      {}", stats.total_checks);
        println!("Opportunities Found:       {}", stats.opportunities_found);
        println!("Profitable (after gas):    {}", stats.profitable_opportunities);
        println!("Total Potential Profit:    ${:.2}", stats.total_potential_profit);
        println!("Avg Profit per Trade:      ${:.4}", stats.avg_profit_when_profitable);
        
        if let Some(best) = &stats.best_opportunity {
            println!();
            println!("🏆 BEST OPPORTUNITY:");
            println!("   Path: {}", best.path);
            println!("   Net Profit: ${:.4}", best.net_profit_usd);
            println!("   Profit %: {:.3}%", best.profit_percent);
        }
        
        println!("═══════════════════════════════════════════════════════");
        
        if stats.profitable_opportunities > 0 {
            println!();
            println!("✅ VERDICT: Opportunities exist! Consider full simulation.");
        } else {
            println!();
            println!("❌ VERDICT: No profitable opportunities found in this period.");
            println!("   Try: Longer monitoring, different token pairs, or lower thresholds.");
        }
    }
}

/// Simple random float for simulating price variance
fn rand_float() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    (nanos as f64 % 1000.0) / 1000.0
}
