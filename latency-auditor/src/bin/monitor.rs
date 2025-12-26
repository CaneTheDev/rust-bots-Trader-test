//! Arbitrage Price Monitor v3.0 - Multi-DEX + Volatile Tokens
//! 
//! Monitors multiple DEXes and token pairs on Base for arbitrage opportunities.
//! 
//! CONFIGURATION: Edit pools_config.rs to add/remove pools and tokens.
//! 
//! Run with: cargo run --release --bin monitor

mod pools_config;

use chrono::Utc;
use futures_util::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::SinkExt;

use pools_config::{PoolConfig, PoolType, get_pool_configs, print_pool_summary, get_monitored_pairs};

// ============================================================================
// Configuration - Adjust these as needed
// ============================================================================

const MONITOR_DURATION_SECS: u64 = 300;
const TRADE_AMOUNT_ETH: f64 = 1.0;
const MIN_PROFIT_USD: f64 = 0.10;
const GAS_UNITS: u64 = 350_000;
const GAS_UNITS_TRIANGULAR: u64 = 450_000;

// ============================================================================
// Data Structures  
// ============================================================================

#[derive(Debug, Clone)]
struct DexPrice {
    dex_name: String,
    pool_address: String,
    price: f64,
    fee_percent: f64,
    pair: String,
    #[allow(dead_code)]
    pool_type: PoolType,
}

#[derive(Debug, Clone, Serialize)]
struct ArbitrageOpportunity {
    block: u64,
    timestamp: String,
    pair: String,
    buy_dex: String,
    buy_price: f64,
    sell_dex: String, 
    sell_price: f64,
    price_diff_percent: f64,
    input_eth: f64,
    input_usd: f64,
    output_usd: f64,
    gross_profit: f64,
    total_fees_usd: f64,
    gas_cost_usd: f64,
    net_profit: f64,
    profitable: bool,
    roi_percent: f64,
}

#[derive(Debug, Default)]
struct Stats {
    blocks_checked: u64,
    opportunities_found: u64,
    profitable_count: u64,
    total_profit: f64,
    best_profit: f64,
    best_opportunity: Option<ArbitrageOpportunity>,
    max_price_diff: f64,
    total_gas_gwei: f64,
    gas_samples: u64,
    pair_max_diffs: HashMap<String, f64>,
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    
    let wss_url = std::env::var("RPC_WSS_URL")
        .expect("RPC_WSS_URL must be set in .env");
    
    let http_url = wss_url
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .replace("/ws", "");

    let pools = get_pool_configs();
    let pairs = get_monitored_pairs();

    println!("===============================================================");
    println!("   MULTI-DEX ARBITRAGE MONITOR v3.0 - Base Mainnet");
    println!("   Targeting VOLATILE tokens for higher spreads");
    println!("===============================================================");
    println!();
    println!("Trade Size:    {} ETH", TRADE_AMOUNT_ETH);
    println!("Min Profit:    ${:.2}", MIN_PROFIT_USD);
    println!("Gas Units:     {} (direct) / {} (triangular)", GAS_UNITS, GAS_UNITS_TRIANGULAR);
    println!("Duration:      {} seconds", MONITOR_DURATION_SECS);
    println!();
    
    print_pool_summary();
    
    println!("Connecting...");

    let url = url::Url::parse(&wss_url)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    let subscribe = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_subscribe", 
        "params": ["newHeads"],
        "id": 1
    });
    
    write.send(Message::Text(subscribe.to_string())).await?;
    
    println!("Connected! Scanning {} pairs across {} pools...", pairs.len(), pools.len());
    
    let http_client = reqwest::Client::new();
    
    // Initial price check to verify pools are working
    let initial_prices = fetch_all_prices(&http_client, &http_url, &pools).await;
    println!("✓ Successfully fetched prices from {}/{} pools", initial_prices.len(), pools.len());
    
    // Show which pools are working per pair
    let mut working_by_pair: HashMap<&str, Vec<&str>> = HashMap::new();
    for p in &initial_prices {
        working_by_pair.entry(&p.pair).or_default().push(&p.dex_name);
    }
    for (pair, dexes) in &working_by_pair {
        if dexes.len() >= 2 {
            let fees: Vec<_> = initial_prices.iter()
                .filter(|p| &p.pair == pair)
                .map(|p| format!("{}({:.2}%)", p.dex_name.split_whitespace().next().unwrap_or("?"), p.fee_percent))
                .collect();
            println!("  {} -> {} DEXes: {}", pair, dexes.len(), fees.join(", "));
        }
    }
    
    println!("---------------------------------------------------------------");

    let start = Instant::now();
    let mut stats = Stats::default();

    while start.elapsed() < Duration::from_secs(MONITOR_DURATION_SECS) {
        let msg = tokio::time::timeout(Duration::from_secs(30), read.next()).await;

        if let Ok(Some(Ok(Message::Text(text)))) = msg {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(params) = json.get("params") {
                    if let Some(result) = params.get("result") {
                        let block_hex = result.get("number")
                            .and_then(|n| n.as_str())
                            .unwrap_or("0x0");
                        let block_num = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)
                            .unwrap_or(0);

                        stats.blocks_checked += 1;

                        // Fetch all prices
                        let prices = fetch_all_prices(&http_client, &http_url, &pools).await;
                        
                        // Get gas price
                        let gas_gwei = get_gas_price(&http_client, &http_url).await.unwrap_or(0.01);
                        stats.total_gas_gwei += gas_gwei;
                        stats.gas_samples += 1;

                        // Check direct arbitrage for each pair
                        check_direct_arbitrage(&prices, block_num, gas_gwei, &mut stats);

                        // Progress update every 10 blocks
                        if stats.blocks_checked % 10 == 0 {
                            let elapsed = start.elapsed().as_secs();
                            let gas_cost_usd = (gas_gwei * GAS_UNITS as f64 / 1e9) * 3500.0;
                            
                            print!("Block {} | {}s | Profit: {}/{} | MaxDiff: {:.2}% | Gas: ${:.4}",
                                block_num, elapsed, 
                                stats.profitable_count, stats.opportunities_found,
                                stats.max_price_diff, gas_cost_usd);
                            
                            // Show top 3 pair diffs
                            let mut diffs: Vec<_> = stats.pair_max_diffs.iter().collect();
                            diffs.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
                            if !diffs.is_empty() {
                                print!(" | Top:");
                                for (pair, diff) in diffs.iter().take(3) {
                                    let token = pair.split('/').next().unwrap_or("?");
                                    print!(" {}:{:.2}%", token, diff);
                                }
                            }
                            println!();
                        }
                    }
                }
            }
        }
    }

    print_summary(&stats, start.elapsed());
    Ok(())
}

async fn fetch_all_prices(
    client: &reqwest::Client,
    rpc_url: &str,
    pools: &[PoolConfig],
) -> Vec<DexPrice> {
    let mut prices = Vec::new();
    let mut failed_pools = Vec::new();
    
    for pool in pools {
        let price_result = match pool.pool_type {
            PoolType::UniswapV3 => {
                get_uniswap_v3_price(client, rpc_url, pool.address, pool.token0_decimals, pool.token1_decimals).await
            }
            PoolType::V2Style => {
                get_v2_price(client, rpc_url, pool.address, pool.token0_decimals, pool.token1_decimals).await
            }
        };
        
        match price_result {
            Ok(mut price) => {
                // Invert price if token order is reversed
                if pool.invert_price && price > 0.0 {
                    price = 1.0 / price;
                }
                
                // Sanity check - price should be reasonable
                if price > 0.0 && price < 1e18 {
                    prices.push(DexPrice {
                        dex_name: pool.dex_name.to_string(),
                        pool_address: pool.address.to_string(),
                        price,
                        fee_percent: pool.fee_percent,
                        pair: pool.pair.to_string(),
                        pool_type: pool.pool_type,
                    });
                } else {
                    failed_pools.push((pool.dex_name, "invalid price range"));
                }
            }
            Err(_) => {
                failed_pools.push((pool.dex_name, "RPC error"));
            }
        }
    }
    
    // Log failed pools occasionally (every ~60 seconds based on block time)
    static mut LAST_LOG: u64 = 0;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    unsafe {
        if now - LAST_LOG > 60 && !failed_pools.is_empty() {
            LAST_LOG = now;
            println!("⚠️  Failed to fetch {} pools: {:?}", failed_pools.len(), failed_pools);
        }
    }
    
    prices
}

fn check_direct_arbitrage(prices: &[DexPrice], block: u64, gas_gwei: f64, stats: &mut Stats) {
    // Group prices by pair
    let mut by_pair: HashMap<&str, Vec<&DexPrice>> = HashMap::new();
    for p in prices {
        by_pair.entry(&p.pair).or_default().push(p);
    }
    
    // Debug: show all prices once per 20 blocks
    let show_debug = stats.blocks_checked % 20 == 1;
    
    for (pair, pair_prices) in by_pair {
        if pair_prices.len() < 2 {
            continue;
        }
        
        // FIXED: Find the BEST combination considering both price AND fees
        // Try all pairs and find the one with maximum net profit
        let mut best_net_profit = f64::MIN;
        let mut best_buy: Option<&DexPrice> = None;
        let mut best_sell: Option<&DexPrice> = None;
        let mut best_tokens = 0.0;
        let mut best_eth_out = 0.0;
        
        let input_eth = TRADE_AMOUNT_ETH;
        let eth_price_usd = 3500.0;
        let input_usd = input_eth * eth_price_usd;
        let gas_cost_usd = (gas_gwei * GAS_UNITS as f64 / 1e9) * eth_price_usd;
        
        for buy in &pair_prices {
            for sell in &pair_prices {
                if buy.pool_address == sell.pool_address {
                    continue;
                }
                
                // Calculate actual profit for this combination
                let tokens_bought = (input_eth / buy.price) * (1.0 - buy.fee_percent / 100.0);
                let eth_received = (tokens_bought * sell.price) * (1.0 - sell.fee_percent / 100.0);
                let output_usd = eth_received * eth_price_usd;
                let net_profit = output_usd - input_usd - gas_cost_usd;
                
                if net_profit > best_net_profit {
                    best_net_profit = net_profit;
                    best_buy = Some(buy);
                    best_sell = Some(sell);
                    best_tokens = tokens_bought;
                    best_eth_out = eth_received;
                }
            }
        }
        
        let (best_buy, best_sell) = match (best_buy, best_sell) {
            (Some(b), Some(s)) => (b, s),
            _ => continue,
        };
        
        let price_diff_percent = ((best_sell.price - best_buy.price) / best_buy.price) * 100.0;
        
        // Track max diff per pair
        let entry = stats.pair_max_diffs.entry(pair.to_string()).or_insert(0.0);
        if price_diff_percent.abs() > entry.abs() {
            *entry = price_diff_percent;
        }
        
        if price_diff_percent > stats.max_price_diff {
            stats.max_price_diff = price_diff_percent;
        }
        
        stats.opportunities_found += 1;
        
        let output_usd = best_eth_out * eth_price_usd;
        let gross_profit = output_usd - input_usd;
        let net_profit = best_net_profit;
        
        // DEBUG: Show detailed breakdown for any spread > 0.1%
        if show_debug && price_diff_percent.abs() > 0.1 {
            println!();
            println!("📊 DEBUG {} | Spread: {:.3}%", pair, price_diff_percent);
            println!("   Best Route: {} ({:.2}%) -> {} ({:.2}%)", 
                best_buy.dex_name, best_buy.fee_percent, best_sell.dex_name, best_sell.fee_percent);
            println!("   Prices: {:.10} -> {:.10}", best_buy.price, best_sell.price);
            println!("   Total Fees: {:.2}%", best_buy.fee_percent + best_sell.fee_percent);
            println!("   Trade: {} ETH -> {:.2} tokens -> {:.6} ETH", input_eth, best_tokens, best_eth_out);
            println!("   P&L: ${:.2} in -> ${:.2} out | Gross: ${:.4} | Gas: ${:.4} | Net: ${:.4}",
                input_usd, output_usd, gross_profit, gas_cost_usd, net_profit);
            
            // Show breakeven analysis
            let total_fee_pct = best_buy.fee_percent + best_sell.fee_percent;
            let breakeven_spread = total_fee_pct + (gas_cost_usd / input_usd * 100.0);
            if net_profit <= MIN_PROFIT_USD {
                println!("   ❌ NOT PROFITABLE: Need {:.3}% spread to break even (have {:.3}%)", 
                    breakeven_spread, price_diff_percent);
            } else {
                println!("   ✅ PROFITABLE!");
            }
        }
        
        if net_profit > MIN_PROFIT_USD {
            stats.profitable_count += 1;
            stats.total_profit += net_profit;
            
            let opp = ArbitrageOpportunity {
                block,
                timestamp: Utc::now().format("%H:%M:%S").to_string(),
                pair: pair.to_string(),
                buy_dex: best_buy.dex_name.clone(),
                buy_price: best_buy.price,
                sell_dex: best_sell.dex_name.clone(),
                sell_price: best_sell.price,
                price_diff_percent,
                input_eth,
                input_usd,
                output_usd,
                gross_profit,
                total_fees_usd: input_usd * (best_buy.fee_percent + best_sell.fee_percent) / 100.0,
                gas_cost_usd,
                net_profit,
                profitable: true,
                roi_percent: (net_profit / input_usd) * 100.0,
            };
            
            if net_profit > stats.best_profit {
                stats.best_profit = net_profit;
                stats.best_opportunity = Some(opp.clone());
            }
            
            println!();
            println!("💰 PROFITABLE! Block {} | {} | {:.2}% spread", block, pair, price_diff_percent);
            println!("   Buy {} @ {:.8} -> Sell {} @ {:.8}", 
                best_buy.dex_name, best_buy.price, best_sell.dex_name, best_sell.price);
            println!("   Input: ${:.2} | Output: ${:.2} | Net Profit: ${:.4}", input_usd, output_usd, net_profit);
        }
    }
}

async fn get_uniswap_v3_price(
    client: &reqwest::Client,
    rpc_url: &str,
    pool_address: &str,
    token0_decimals: u8,
    token1_decimals: u8,
) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": pool_address, "data": "0x3850c7bd"}, "latest"],
        "id": 1
    });
    
    let response = client.post(rpc_url).json(&request).send().await?
        .json::<serde_json::Value>().await?;
    
    if let Some(result) = response.get("result").and_then(|r| r.as_str()) {
        if result.len() >= 66 {
            let sqrt_price_hex = &result[2..66];
            let sqrt_price_x96 = u128::from_str_radix(sqrt_price_hex, 16).unwrap_or(0);
            let sqrt_price = sqrt_price_x96 as f64 / (2_f64.powi(96));
            let price = sqrt_price * sqrt_price;
            let decimal_adj = 10_f64.powi(token0_decimals as i32 - token1_decimals as i32);
            return Ok(price * decimal_adj);
        }
    }
    Err("Failed to get V3 price".into())
}

async fn get_v2_price(
    client: &reqwest::Client,
    rpc_url: &str,
    pool_address: &str,
    token0_decimals: u8,
    token1_decimals: u8,
) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    // Try standard getReserves() first
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": pool_address, "data": "0x0902f1ac"}, "latest"],
        "id": 1
    });
    
    let response = client.post(rpc_url).json(&request).send().await?
        .json::<serde_json::Value>().await?;
    
    if let Some(result) = response.get("result").and_then(|r| r.as_str()) {
        if result.len() >= 130 && !result.starts_with("0x0000000000000000000000000000000000000000000000000000000000000000") {
            let reserve0_hex = &result[2..66];
            let reserve1_hex = &result[66..130];
            let reserve0 = u128::from_str_radix(reserve0_hex, 16).unwrap_or(1);
            let reserve1 = u128::from_str_radix(reserve1_hex, 16).unwrap_or(1);
            
            if reserve0 > 0 && reserve1 > 0 {
                let decimal_adj = 10_f64.powi(token0_decimals as i32 - token1_decimals as i32);
                let price = (reserve1 as f64 / reserve0 as f64) * decimal_adj;
                return Ok(price);
            }
        }
    }
    
    // Fallback: Try Aerodrome/Velodrome style with separate reserve0() and reserve1()
    // reserve0() = 0x443cb4bc, reserve1() = 0x5a76f25e
    let req0 = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": pool_address, "data": "0x443cb4bc"}, "latest"],
        "id": 1
    });
    let req1 = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": pool_address, "data": "0x5a76f25e"}, "latest"],
        "id": 2
    });
    
    let (res0, res1) = tokio::join!(
        client.post(rpc_url).json(&req0).send(),
        client.post(rpc_url).json(&req1).send()
    );
    
    let res0 = res0?.json::<serde_json::Value>().await?;
    let res1 = res1?.json::<serde_json::Value>().await?;
    
    if let (Some(r0), Some(r1)) = (
        res0.get("result").and_then(|r| r.as_str()),
        res1.get("result").and_then(|r| r.as_str())
    ) {
        if r0.len() >= 66 && r1.len() >= 66 {
            let reserve0 = u128::from_str_radix(&r0[2..], 16).unwrap_or(1);
            let reserve1 = u128::from_str_radix(&r1[2..], 16).unwrap_or(1);
            
            if reserve0 > 0 && reserve1 > 0 {
                let decimal_adj = 10_f64.powi(token0_decimals as i32 - token1_decimals as i32);
                let price = (reserve1 as f64 / reserve0 as f64) * decimal_adj;
                return Ok(price);
            }
        }
    }
    
    Err("Failed to get V2 price".into())
}

async fn get_gas_price(
    client: &reqwest::Client,
    rpc_url: &str,
) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });
    
    let response = client.post(rpc_url).json(&request).send().await?
        .json::<serde_json::Value>().await?;
    
    if let Some(result) = response.get("result").and_then(|r| r.as_str()) {
        let gas_wei = u64::from_str_radix(result.trim_start_matches("0x"), 16).unwrap_or(0);
        return Ok(gas_wei as f64 / 1e9);
    }
    Ok(0.01)
}

fn print_summary(stats: &Stats, elapsed: Duration) {
    let avg_gas = if stats.gas_samples > 0 { 
        stats.total_gas_gwei / stats.gas_samples as f64 
    } else { 0.01 };
    let eth_price = 3500.0;
    let avg_gas_cost = (avg_gas * GAS_UNITS as f64 / 1e9) * eth_price;

    println!();
    println!("===============================================================");
    println!("                    MONITORING SUMMARY v3.0");
    println!("===============================================================");
    println!("Duration:              {} seconds", elapsed.as_secs());
    println!("Blocks Analyzed:       {}", stats.blocks_checked);
    println!();
    println!("DIRECT ARBITRAGE:");
    println!("  Opportunities:       {}", stats.opportunities_found);
    println!("  Profitable:          {}", stats.profitable_count);
    println!("  Max Price Diff:      {:.4}%", stats.max_price_diff);
    println!("  Total Profit:        ${:.4}", stats.total_profit);
    println!("  Best Trade:          ${:.4}", stats.best_profit);
    println!();
    println!("GAS:");
    println!("  Avg Gas Price:       {:.4} gwei", avg_gas);
    println!("  Avg Gas Cost:        ${:.6}", avg_gas_cost);
    println!();
    println!("PER-PAIR MAX SPREADS:");
    
    // Sort by spread descending
    let mut diffs: Vec<_> = stats.pair_max_diffs.iter().collect();
    diffs.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
    
    for (pair, diff) in &diffs {
        let status = if **diff > 1.0 { "🔥" } else if **diff > 0.5 { "⚡" } else { "  " };
        println!("  {} {}: {:.4}%", status, pair, diff);
    }
    println!("===============================================================");
    
    if let Some(best) = &stats.best_opportunity {
        println!();
        println!("💰 BEST OPPORTUNITY:");
        println!("  Pair: {}", best.pair);
        println!("  {} @ {:.8} -> {} @ {:.8}", 
            best.buy_dex, best.buy_price, best.sell_dex, best.sell_price);
        println!("  Spread: {:.2}%", best.price_diff_percent);
        println!("  Net Profit: ${:.4} ({:.3}% ROI)", best.net_profit, best.roi_percent);
    }
    
    if stats.profitable_count > 0 {
        let avg = stats.total_profit / stats.profitable_count as f64;
        let hourly_rate = (stats.profitable_count as f64 / elapsed.as_secs() as f64) * 3600.0;
        println!();
        println!("PROJECTED EARNINGS:");
        println!("  Avg Profit/Trade:   ${:.4}", avg);
        println!("  Trades/Hour:        {:.1}", hourly_rate);
        println!("  Hourly Profit:      ${:.2}", hourly_rate * avg);
        println!("  Daily Profit:       ${:.2}", hourly_rate * avg * 24.0);
        println!();
        println!("✅ VERDICT: Opportunities exist! Deploy to capture them.");
    } else {
        println!();
        println!("No profitable opportunities found yet.");
        println!("Max spread was {:.2}% - volatile tokens can hit 1-5%.", stats.max_price_diff);
        println!();
        println!("NEXT STEPS:");
        println!("  1. Verify pool addresses in pools_config.rs");
        println!("  2. Add more DEX pools for each token");
        println!("  3. Monitor during high volatility (news, launches)");
        println!("  4. Try smaller/newer meme coins");
    }
}
