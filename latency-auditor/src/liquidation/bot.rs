//! Main liquidation bot logic

use crate::liquidation::{Config, Borrower, Market, LiquidationOpportunity, BotStats};
use crate::liquidation::contracts::{self, markets, selectors};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::path::Path;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Persistent borrower data saved to disk
#[derive(Serialize, Deserialize, Default)]
struct BorrowerCache {
    last_scanned_block: u64,
    borrowers: Vec<String>,
    last_updated: String,
}

const BORROWER_CACHE_FILE: &str = "borrowers_cache.json";
const BLOCKS_PER_BATCH: u64 = 2_000;  // 2k blocks - Base RPC limit is ~5k
const MOONWELL_LAUNCH_BLOCK: u64 = 2_000_000;  // Approx Moonwell launch on Base
const BASE_PUBLIC_RPC: &str = "https://mainnet.base.org";  // Use for eth_getLogs

pub struct LiquidationBot {
    config: Config,
    http_client: reqwest::Client,
    markets: HashMap<String, Market>,
    borrowers: HashMap<String, Borrower>,
    prices: HashMap<String, f64>,
    stats: BotStats,
    last_scanned_block: u64,
}

impl LiquidationBot {
    pub async fn new(config: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = reqwest::Client::new();
        
        let mut bot = Self {
            config,
            http_client,
            markets: HashMap::new(),
            borrowers: HashMap::new(),
            prices: HashMap::new(),
            stats: BotStats::default(),
            last_scanned_block: 0,
        };
        
        bot.load_borrower_cache();
        bot.load_markets().await?;
        
        Ok(bot)
    }
    
    fn load_borrower_cache(&mut self) {
        if Path::new(BORROWER_CACHE_FILE).exists() {
            if let Ok(data) = std::fs::read_to_string(BORROWER_CACHE_FILE) {
                if let Ok(cache) = serde_json::from_str::<BorrowerCache>(&data) {
                    println!("📂 Loading {} cached accounts from disk...", cache.borrowers.len());
                    println!("   Last scanned block: {}", cache.last_scanned_block);
                    
                    for addr in cache.borrowers {
                        self.borrowers.insert(addr.to_lowercase(), Borrower::new(addr));
                    }
                    self.last_scanned_block = cache.last_scanned_block;
                }
            }
        }
    }
    
    fn save_borrower_cache(&self) {
        let cache = BorrowerCache {
            last_scanned_block: self.last_scanned_block,
            borrowers: self.borrowers.keys().cloned().collect(),
            last_updated: Utc::now().to_rfc3339(),
        };
        
        if let Ok(json) = serde_json::to_string_pretty(&cache) {
            let _ = std::fs::write(BORROWER_CACHE_FILE, json);
        }
    }
    
    async fn load_markets(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Loading Moonwell markets...");
        
        for market_addr in markets::all() {
            let symbol = markets::symbol(market_addr);
            let collateral_factor = self.get_collateral_factor(market_addr).await.unwrap_or(0.0);
            let exchange_rate = self.get_exchange_rate(market_addr).await.unwrap_or(1.0);
            let price = self.get_underlying_price(market_addr).await.unwrap_or(0.0);
            self.prices.insert(market_addr.to_lowercase(), price);
            
            let market = Market {
                address: market_addr.to_string(),
                symbol: symbol.to_string(),
                underlying: String::new(),
                underlying_decimals: 18,
                collateral_factor,
                liquidation_incentive: 1.08,
                exchange_rate,
                borrow_rate: 0.0,
            };
            
            if price > 0.0 {
                println!("  ✓ {} | CF: {:.0}% | Price: ${:.2}", symbol, collateral_factor * 100.0, price);
            }
            
            self.markets.insert(market_addr.to_lowercase(), market);
        }
        
        println!("Loaded {} markets", self.markets.len());
        Ok(())
    }

    async fn get_collateral_factor(&self, market: &str) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let data = format!("{}000000000000000000000000{}", selectors::MARKETS, &market[2..]);
        let result = self.eth_call(contracts::COMPTROLLER, &data).await?;
        
        if result.len() >= 130 {
            let cf_hex = &result[66..130];
            let cf_raw = u128::from_str_radix(cf_hex, 16).unwrap_or(0);
            return Ok(cf_raw as f64 / 1e18);
        }
        Ok(0.0)
    }
    
    async fn get_exchange_rate(&self, market: &str) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let result = self.eth_call(market, selectors::EXCHANGE_RATE_STORED).await?;
        if result.len() >= 66 {
            let rate_hex = &result[2..66];
            let rate_raw = u128::from_str_radix(rate_hex, 16).unwrap_or(0);
            return Ok(rate_raw as f64 / 1e18);
        }
        Ok(1.0)
    }
    
    async fn get_underlying_price(&self, market: &str) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let data = format!("{}000000000000000000000000{}", selectors::GET_UNDERLYING_PRICE, &market[2..]);
        let result = self.eth_call(contracts::ORACLE, &data).await?;
        
        if result.len() >= 66 {
            let price_hex = &result[2..66];
            let price_raw = u128::from_str_radix(price_hex, 16).unwrap_or(0);
            
            let market_lower = market.to_lowercase();
            let scale: i32 = if market_lower == markets::MW_USDC.to_lowercase() 
                || market_lower == markets::MW_EURC.to_lowercase() {
                30
            } else if market_lower == markets::MW_CBBTC.to_lowercase() {
                28
            } else {
                18
            };
            
            return Ok(price_raw as f64 / 10_f64.powi(scale));
        }
        Ok(0.0)
    }
    
    async fn get_account_liquidity(&self, user: &str) -> Result<(f64, f64), Box<dyn std::error::Error + Send + Sync>> {
        let data = format!("{}000000000000000000000000{}", selectors::ACCOUNT_LIQUIDITY, &user[2..]);
        let result = self.eth_call(contracts::COMPTROLLER, &data).await?;
        
        if result.len() >= 194 {
            let liquidity_hex = &result[66..130];
            let shortfall_hex = &result[130..194];
            let liquidity = u128::from_str_radix(liquidity_hex, 16).unwrap_or(0) as f64 / 1e18;
            let shortfall = u128::from_str_radix(shortfall_hex, 16).unwrap_or(0) as f64 / 1e18;
            return Ok((liquidity, shortfall));
        }
        Ok((0.0, 0.0))
    }
    
    async fn eth_call(&self, to: &str, data: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": to, "data": data}, "latest"],
            "id": 1
        });
        
        let response = self.http_client
            .post(&self.config.rpc_http_url)
            .json(&request)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        
        if let Some(result) = response.get("result").and_then(|r| r.as_str()) {
            return Ok(result.to_string());
        }
        Err("RPC call failed".into())
    }
    
    async fn get_gas_price(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_gasPrice",
            "params": [],
            "id": 1
        });
        
        let response = self.http_client
            .post(&self.config.rpc_http_url)
            .json(&request)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        
        if let Some(result) = response.get("result").and_then(|r| r.as_str()) {
            let gas_wei = u64::from_str_radix(result.trim_start_matches("0x"), 16).unwrap_or(0);
            return Ok(gas_wei as f64 / 1e9);
        }
        Ok(0.01)
    }

    /// Main bot loop
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!();
        println!("Scanning for accounts that can be liquidated...");
        println!("---------------------------------------------------------------");
        
        self.scan_for_borrowers().await?;
        
        println!();
        println!("Starting real-time monitoring...");
        println!("Watching for: Oracle updates, New positions, Liquidation opportunities");
        println!("---------------------------------------------------------------");
        
        let url = url::Url::parse(&self.config.rpc_wss_url)?;
        let (ws_stream, _) = connect_async(url).await?;
        let (mut write, mut read) = ws_stream.split();
        
        let subscribe = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_subscribe",
            "params": ["newHeads"],
            "id": 1
        });
        write.send(Message::Text(subscribe.to_string())).await?;
        
        println!("✓ Connected to Base mainnet via WebSocket");
        
        let start = Instant::now();
        let mut last_price_update = Instant::now();
        let mut last_stats_print = Instant::now();
        let mut last_cache_save = Instant::now();
        let mut last_scan = Instant::now();
        
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(30), read.next()).await;
            
            if let Ok(Some(Ok(Message::Text(text)))) = msg {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(params) = json.get("params") {
                        if let Some(result) = params.get("result") {
                            let block_hex = result.get("number").and_then(|n| n.as_str()).unwrap_or("0x0");
                            let block_num = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16).unwrap_or(0);
                            
                            self.stats.blocks_processed += 1;
                            
                            if last_price_update.elapsed() > Duration::from_secs(30) {
                                self.update_prices().await;
                                last_price_update = Instant::now();
                            }
                            
                            if last_scan.elapsed() > Duration::from_secs(300) {
                                self.scan_recent_events(block_num).await;
                                last_scan = Instant::now();
                            }
                            
                            self.check_borrowers_health().await;
                            
                            let opportunities = self.find_opportunities().await;
                            for opp in opportunities {
                                self.handle_opportunity(&opp, block_num).await;
                            }
                            
                            if last_cache_save.elapsed() > Duration::from_secs(600) {
                                self.save_borrower_cache();
                                last_cache_save = Instant::now();
                            }
                            
                            if last_stats_print.elapsed() > Duration::from_secs(60) {
                                self.print_stats(block_num, start.elapsed());
                                last_stats_print = Instant::now();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Scan MarketEntered events on Comptroller - THE EFFICIENT APPROACH
    /// Only 1 contract, 1 event type = fast indexing of ALL potential liquidation targets
    async fn scan_for_borrowers(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });
        
        let response = self.http_client
            .post(&self.config.rpc_http_url)
            .json(&request)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        
        let current_block = response.get("result")
            .and_then(|r| r.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        
        let initial_count = self.borrowers.len();
        
        let start_block = if self.last_scanned_block > MOONWELL_LAUNCH_BLOCK {
            self.last_scanned_block + 1
        } else {
            MOONWELL_LAUNCH_BLOCK
        };
        
        if start_block >= current_block {
            println!("✓ Account cache is up to date ({} accounts)", self.borrowers.len());
            self.stats.borrowers_tracked = self.borrowers.len();
            return Ok(());
        }
        
        let total_blocks = current_block - start_block;
        let batches = (total_blocks / BLOCKS_PER_BATCH) + 1;
        let estimated_time = (batches as f64 * 0.25) / 60.0;  // ~250ms per batch
        
        println!("📊 Scanning MarketEntered events on Comptroller...");
        println!("   Strategy: 1 contract, 1 event = fast & efficient");
        println!("   From block {} to {} ({} blocks, {} batches)", start_block, current_block, total_blocks, batches);
        println!("   Estimated time: {:.1} minutes", estimated_time);
        println!("   Using RPC: {}", BASE_PUBLIC_RPC);
        println!();
        
        let mut batch_num = 0;
        let mut from_block = start_block;
        let mut total_events = 0;
        let mut consecutive_errors = 0;
        let scan_start = Instant::now();
        
        while from_block < current_block {
            batch_num += 1;
            let to_block = (from_block + BLOCKS_PER_BATCH).min(current_block);
            
            let (events, error) = self.scan_market_entered(from_block, to_block).await;
            
            if let Some(err_msg) = error {
                consecutive_errors += 1;
                println!("   ❌ Batch {}: blocks {}-{} FAILED: {}", batch_num, from_block, to_block, err_msg);
                
                if consecutive_errors >= 5 {
                    println!("   ⚠️  Too many consecutive errors, pausing 5 seconds...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    consecutive_errors = 0;
                }
            } else {
                consecutive_errors = 0;
                total_events += events;
                
                // Log every 50 batches or when we find events
                if batch_num % 50 == 0 || events > 0 {
                    let elapsed = scan_start.elapsed().as_secs_f64();
                    let progress = (batch_num as f64 / batches as f64) * 100.0;
                    let eta = if batch_num > 0 {
                        (elapsed / batch_num as f64) * (batches - batch_num) as f64
                    } else { 0.0 };
                    
                    println!("   ✓ Batch {}/{} ({:.1}%) | blocks {}-{} | {} events this batch | {} total accounts | ETA: {:.0}s", 
                        batch_num, batches, progress, from_block, to_block, events, self.borrowers.len(), eta);
                }
            }
            
            self.last_scanned_block = to_block;
            
            // Save cache every 100 batches
            if batch_num % 100 == 0 {
                self.save_borrower_cache();
                println!("   💾 Cache saved ({} accounts)", self.borrowers.len());
            }
            
            // Rate limit: 200ms between requests
            tokio::time::sleep(Duration::from_millis(200)).await;
            from_block = to_block + 1;
        }
        
        println!();
        self.save_borrower_cache();
        
        let elapsed = scan_start.elapsed();
        let new_count = self.borrowers.len() - initial_count;
        println!("✅ SCAN COMPLETE in {:.1} minutes", elapsed.as_secs_f64() / 60.0);
        println!("   Total MarketEntered events: {}", total_events);
        println!("   New accounts found: {}", new_count);
        println!("   Total accounts tracked: {}", self.borrowers.len());
        self.stats.borrowers_tracked = self.borrowers.len();
        
        if self.borrowers.is_empty() {
            println!("   ⚠️  WARNING: No accounts found! RPC may have issues.");
        } else {
            println!();
            println!("Checking health of {} accounts...", self.borrowers.len());
            self.check_borrowers_health().await;
        }
        
        Ok(())
    }
    
    /// Scan MarketEntered(address mToken, address account) events
    /// Returns (event_count, optional_error_message)
    async fn scan_market_entered(&mut self, from_block: u64, to_block: u64) -> (usize, Option<String>) {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getLogs",
            "params": [{
                "address": contracts::COMPTROLLER,
                "topics": [contracts::events::MARKET_ENTERED],
                "fromBlock": format!("0x{:x}", from_block),
                "toBlock": format!("0x{:x}", to_block)
            }],
            "id": 1
        });
        
        // Use Base public RPC for logs - dRPC has issues with eth_getLogs
        let response = self.http_client
            .post(BASE_PUBLIC_RPC)
            .json(&request)
            .send()
            .await;
        
        match response {
            Ok(resp) => {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        // Check for RPC error
                        if let Some(error) = json.get("error") {
                            let msg = error.get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("unknown error");
                            return (0, Some(msg.to_string()));
                        }
                        
                        if let Some(logs) = json.get("result").and_then(|r| r.as_array()) {
                            let count = logs.len();
                            for log in logs {
                                if let Some(data) = log.get("data").and_then(|d| d.as_str()) {
                                    if data.len() >= 130 {
                                        let account = format!("0x{}", &data[90..130]);
                                        let account_lower = account.to_lowercase();
                                        if !self.borrowers.contains_key(&account_lower) {
                                            self.borrowers.insert(account_lower, Borrower::new(account));
                                        }
                                    }
                                }
                            }
                            return (count, None);
                        }
                        (0, Some("no result in response".to_string()))
                    }
                    Err(e) => (0, Some(format!("JSON parse error: {}", e)))
                }
            }
            Err(e) => (0, Some(format!("HTTP error: {}", e)))
        }
    }
    
    /// Incremental scan for new accounts
    async fn scan_recent_events(&mut self, current_block: u64) {
        let from_block = self.last_scanned_block + 1;
        if from_block >= current_block { return; }
        
        let initial = self.borrowers.len();
        let (events, error) = self.scan_market_entered(from_block, current_block).await;
        self.last_scanned_block = current_block;
        
        if let Some(err) = error {
            println!("   ⚠️  Scan error: {}", err);
            return;
        }
        
        let new_count = self.borrowers.len() - initial;
        if new_count > 0 {
            println!("   📥 +{} accounts from {} events (total: {})", new_count, events, self.borrowers.len());
            self.stats.borrowers_tracked = self.borrowers.len();
        }
    }

    async fn update_prices(&mut self) {
        for market_addr in markets::all() {
            if let Ok(price) = self.get_underlying_price(market_addr).await {
                if price > 0.0 {
                    self.prices.insert(market_addr.to_lowercase(), price);
                }
            }
        }
    }
    
    async fn check_borrowers_health(&mut self) {
        let addrs: Vec<String> = self.borrowers.keys().cloned().collect();
        let total = addrs.len();
        
        if total == 0 {
            return;
        }
        
        println!("   Using Multicall3 to batch health checks...");
        
        let mut risky = 0;
        let mut active = 0;
        let batch_size = 200;  // 200 accounts per multicall
        let batches = (total + batch_size - 1) / batch_size;
        
        for (batch_idx, chunk) in addrs.chunks(batch_size).enumerate() {
            // Build multicall data
            let calls: Vec<serde_json::Value> = chunk.iter().map(|addr| {
                let call_data = format!("{}000000000000000000000000{}", 
                    selectors::ACCOUNT_LIQUIDITY, 
                    &addr[2..]
                );
                serde_json::json!({
                    "target": contracts::COMPTROLLER,
                    "allowFailure": true,
                    "callData": call_data
                })
            }).collect();
            
            // Encode multicall: aggregate3(Call3[] calldata calls)
            // Function selector: 0x82ad56cb
            let calls_encoded = self.encode_multicall_data(&calls);
            let multicall_data = format!("0x82ad56cb{}", calls_encoded);
            
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_call",
                "params": [{
                    "to": contracts::MULTICALL3,
                    "data": multicall_data
                }, "latest"],
                "id": 1
            });
            
            if let Ok(response) = self.http_client
                .post(&self.config.rpc_http_url)
                .json(&request)
                .send()
                .await
            {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    if let Some(result) = json.get("result").and_then(|r| r.as_str()) {
                        // Parse multicall results
                        let results = self.parse_multicall_results(result, chunk.len());
                        
                        for (i, (success, liquidity, shortfall)) in results.iter().enumerate() {
                            if let Some(addr) = chunk.get(i) {
                                if let Some(borrower) = self.borrowers.get_mut(addr) {
                                    if *success {
                                        if *shortfall > 0.0 {
                                            borrower.health_factor = 0.0;
                                            borrower.total_borrow_usd = *shortfall;
                                            risky += 1;
                                            active += 1;
                                        } else if *liquidity > 0.0 {
                                            borrower.health_factor = 1.0 + (*liquidity / 1000.0).min(10.0);
                                            borrower.total_collateral_usd = *liquidity;
                                            active += 1;
                                            if borrower.health_factor < self.config.health_threshold {
                                                risky += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Progress every 10 batches
            if (batch_idx + 1) % 10 == 0 || batch_idx == batches - 1 {
                let progress = ((batch_idx + 1) as f64 / batches as f64) * 100.0;
                println!("   Batch {}/{} ({:.1}%) | {} active | {} risky", 
                    batch_idx + 1, batches, progress, active, risky);
            }
            
            // Small delay between batches
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        
        self.stats.risky_borrowers = risky;
        self.stats.borrowers_tracked = active;
        println!("   ✓ Health check complete: {} active accounts, {} risky", active, risky);
    }
    
    /// Encode calls for Multicall3 aggregate3
    fn encode_multicall_data(&self, calls: &[serde_json::Value]) -> String {
        // ABI encode: array offset (32) + array length + each call struct
        let mut result = String::new();
        
        // Offset to array data (always 0x20 = 32 for single dynamic param)
        result.push_str("0000000000000000000000000000000000000000000000000000000000000020");
        
        // Array length
        result.push_str(&format!("{:064x}", calls.len()));
        
        // Offset to each call's data (relative to array start)
        let header_size = calls.len() * 32;
        let mut data_offset = header_size;
        let mut offsets = Vec::new();
        let mut call_data_parts = Vec::new();
        
        for call in calls {
            offsets.push(format!("{:064x}", data_offset));
            
            let target = call["target"].as_str().unwrap_or("");
            let allow_failure = call["allowFailure"].as_bool().unwrap_or(true);
            let call_data = call["callData"].as_str().unwrap_or("0x");
            let call_data_bytes = &call_data[2..];  // Remove 0x
            let call_data_len = call_data_bytes.len() / 2;
            
            // Each call struct: target (32) + allowFailure (32) + callData offset (32) + callData length (32) + callData (padded)
            let mut call_encoded = String::new();
            // target (address padded to 32 bytes)
            call_encoded.push_str(&format!("000000000000000000000000{}", &target[2..]));
            // allowFailure (bool as uint256)
            call_encoded.push_str(&format!("{:064x}", if allow_failure { 1 } else { 0 }));
            // callData offset (always 0x60 = 96 for this struct layout)
            call_encoded.push_str("0000000000000000000000000000000000000000000000000000000000000060");
            // callData length
            call_encoded.push_str(&format!("{:064x}", call_data_len));
            // callData (padded to 32 bytes)
            call_encoded.push_str(call_data_bytes);
            let padding = (32 - (call_data_len % 32)) % 32;
            call_encoded.push_str(&"0".repeat(padding * 2));
            
            data_offset += call_encoded.len() / 2;
            call_data_parts.push(call_encoded);
        }
        
        // Write offsets
        for offset in offsets {
            result.push_str(&offset);
        }
        
        // Write call data
        for part in call_data_parts {
            result.push_str(&part);
        }
        
        result
    }
    
    /// Parse Multicall3 aggregate3 results
    fn parse_multicall_results(&self, result: &str, expected_count: usize) -> Vec<(bool, f64, f64)> {
        let mut results = Vec::new();
        let data = result.trim_start_matches("0x");
        
        if data.len() < 64 {
            return vec![(false, 0.0, 0.0); expected_count];
        }
        
        // Skip array offset (32 bytes) and get array length
        let array_len = usize::from_str_radix(&data[64..128], 16).unwrap_or(0);
        
        if array_len == 0 {
            return vec![(false, 0.0, 0.0); expected_count];
        }
        
        // Each result is: success (bool) + returnData (bytes)
        // Results start after: offset (32) + length (32) + offsets (32 * array_len)
        let offsets_end = 128 + (array_len * 64);
        
        for i in 0..array_len.min(expected_count) {
            // Get offset for this result
            let offset_pos = 128 + (i * 64);
            if offset_pos + 64 > data.len() {
                results.push((false, 0.0, 0.0));
                continue;
            }
            
            let offset = usize::from_str_radix(&data[offset_pos..offset_pos + 64], 16).unwrap_or(0);
            let result_start = 64 + (offset * 2);  // Convert byte offset to hex char offset
            
            if result_start + 64 > data.len() {
                results.push((false, 0.0, 0.0));
                continue;
            }
            
            // Parse success bool
            let success = &data[result_start..result_start + 64];
            let is_success = success.ends_with("1");
            
            if !is_success {
                results.push((false, 0.0, 0.0));
                continue;
            }
            
            // Parse return data (getAccountLiquidity returns: error, liquidity, shortfall)
            // Skip success (32) + data offset (32) + data length (32) = 96 bytes = 192 chars
            let return_data_start = result_start + 192;
            
            if return_data_start + 192 > data.len() {
                results.push((true, 0.0, 0.0));
                continue;
            }
            
            // Skip error code (32 bytes), get liquidity and shortfall
            let liquidity_hex = &data[return_data_start + 64..return_data_start + 128];
            let shortfall_hex = &data[return_data_start + 128..return_data_start + 192];
            
            let liquidity = u128::from_str_radix(liquidity_hex, 16).unwrap_or(0) as f64 / 1e18;
            let shortfall = u128::from_str_radix(shortfall_hex, 16).unwrap_or(0) as f64 / 1e18;
            
            results.push((true, liquidity, shortfall));
        }
        
        // Pad with failures if we didn't get enough results
        while results.len() < expected_count {
            results.push((false, 0.0, 0.0));
        }
        
        results
    }
    
    async fn find_opportunities(&self) -> Vec<LiquidationOpportunity> {
        let mut opps = Vec::new();
        
        for (addr, borrower) in &self.borrowers {
            if !borrower.is_liquidatable() { continue; }
            
            let (_, shortfall) = self.get_account_liquidity(addr).await.unwrap_or((0.0, 0.0));
            if shortfall <= 0.0 { continue; }
            
            let gas_gwei = self.get_gas_price().await.unwrap_or(0.01);
            let eth_price = self.prices.get(&markets::MW_WETH.to_lowercase()).unwrap_or(&3500.0);
            let gas_cost = (gas_gwei * 500_000.0 / 1e9) * eth_price;
            
            let max_repay = shortfall * 0.5;
            let seize_value = max_repay * 1.08;
            let net_profit = (seize_value - max_repay) - gas_cost;
            
            if net_profit > self.config.min_profit_usd {
                opps.push(LiquidationOpportunity {
                    borrower: addr.clone(),
                    debt_market: markets::MW_WETH.to_string(),
                    debt_symbol: "WETH".to_string(),
                    collateral_market: markets::MW_USDC.to_string(),
                    collateral_symbol: "USDC".to_string(),
                    repay_amount: max_repay,
                    seize_amount: seize_value,
                    profit_usd: net_profit,
                    health_factor: borrower.health_factor,
                    gas_estimate_usd: gas_cost,
                });
            }
        }
        opps
    }
    
    async fn handle_opportunity(&mut self, opp: &LiquidationOpportunity, block: u64) {
        self.stats.opportunities_found += 1;
        let ts = Utc::now().format("%H:%M:%S").to_string();
        
        println!();
        println!("🎯 LIQUIDATION OPPORTUNITY!");
        println!("   Block: {} | Time: {}", block, ts);
        println!("   Target: {}", &opp.borrower[..10]);
        println!("   HF: {:.4} | Repay: ${:.2} -> Seize: ${:.2}", opp.health_factor, opp.repay_amount, opp.seize_amount);
        println!("   Gas: ${:.4} | Net Profit: ${:.2}", opp.gas_estimate_usd, opp.profit_usd);
        
        if self.config.simulation {
            println!("   📊 SIMULATION - Would execute");
            self.stats.simulated_liquidations += 1;
            self.stats.simulated_profit += opp.profit_usd;
        } else {
            println!("   ⚡ LIVE - Executing...");
            self.stats.live_liquidations += 1;
            self.stats.live_profit += opp.profit_usd;
        }
    }
    
    fn print_stats(&self, block: u64, elapsed: Duration) {
        println!();
        println!("📊 STATS | Block {} | Uptime: {}s", block, elapsed.as_secs());
        println!("   Accounts: {} tracked | {} risky (HF < {:.1})", 
            self.stats.borrowers_tracked, self.stats.risky_borrowers, self.config.health_threshold);
        println!("   Opportunities: {} found", self.stats.opportunities_found);
        
        if self.config.simulation {
            println!("   Simulated: {} | ${:.2} profit", self.stats.simulated_liquidations, self.stats.simulated_profit);
        } else {
            println!("   Live: {} | ${:.2} profit", self.stats.live_liquidations, self.stats.live_profit);
        }
    }
}
