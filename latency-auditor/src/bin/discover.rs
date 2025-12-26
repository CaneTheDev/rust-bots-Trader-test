//! Pool Discovery Tool v2 - "The Golden 50"
//! 
//! 1. Scans top Volatile/Meme/Gaming tokens on Base.
//! 2. Filters for liquidity > $5,000.
//! 3. SORTS by 24h Volume (Volatility Proxy).
//! 4. Outputs exactly the Top 50 pools for your monitor.
//! 
//! Usage: cargo run --bin discover

use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use std::collections::HashSet;

// ============================================================================
// EXPANDED TARGET LIST (Memes, AI, Gaming, DEX Gov)
// ============================================================================
const TARGET_TOKENS: &[&str] = &[
    "0x4200000000000000000000000000000000000006", // WETH (Base)
    "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", // USDC
    "0x532f27101965dd16442E59d40670FaF5eBB142E4", // BRETT (Top Meme)
    "0x4ed4E862860beD51a9570b96d89aF5E1B0Efefed", // DEGEN (Farcaster)
    "0xAC1Bd2486aAf3B5C0fc3Fd868558b082a531B2B4", // TOSHI (Base Face)
    "0x940181a94A35A4569E4529A3CDfB74e38FD98631", // AERO (Aerodrome)
    "0xcdb4db8034b3f0914b1c6778f64f43c394e48b81", // ALB (Alien Base)
    "0x0b3e328455c4059eeb9e3f84b5543f74e24e7e1b", // VIRTUAL (AI Agent)
    "0x111111111117dc0aa78b770fa6a738034120c302", // PRIME (Gaming)
    "0x29219dd400f2bf60e5a23d13be72b486d4038894", // MOG (Meme)
    "0x93980959778166ccbB95Db7Edf5260755D258880", // KEYCAT
    "0x7f12d13b34f5f4f0a9449c16bcd42f0da47af591", // NORMIE
    "0x0578d8d445a43b24666f853697a6e13a027150a2", // HIGHER
    "0xe5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f", // WELL
    "0xB0f67139703352A0123925F45f6C03505299E5C5", // BOOMER
    "0x6921B130D297cc43754afba22e5EAc0FBf8Db75b", // DOGINME
];

const MIN_LIQUIDITY_USD: f64 = 5_000.0;
const TOP_N_POOLS: usize = 50;

#[derive(Debug, Deserialize, Clone)]
struct DexResponse {
    pairs: Option<Vec<Pair>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Pair {
    #[serde(rename = "chainId")]
    chain_id: Option<String>,
    #[serde(rename = "pairAddress")]
    pair_address: String,
    #[serde(rename = "baseToken")]
    base_token: Token,
    #[serde(rename = "quoteToken")]
    quote_token: Token,
    #[serde(rename = "dexId")]
    dex_id: String,
    labels: Option<Vec<String>>,
    liquidity: Option<Liquidity>,
    volume: Option<Volume>,
    #[serde(rename = "priceUsd")]
    price_usd: Option<String>,
    #[serde(rename = "fdv")]
    fdv: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
struct Token {
    address: Option<String>,
    symbol: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Liquidity {
    usd: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
struct Volume {
    h24: Option<f64>,
}

// Processed pool for ranking
#[derive(Debug, Clone)]
struct RankedPool {
    address: String,
    dex: String,
    pair: String,
    base_symbol: String,
    quote_symbol: String,
    liquidity_usd: f64,
    volume_24h: f64,
    fee_tier: String,
    pool_type: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 STARTING GOLDEN 50 POOL DISCOVERY...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut all_pools: Vec<RankedPool> = Vec::new();
    let mut seen_addresses: HashSet<String> = HashSet::new();

    // Query DexScreener for each token (in batches to avoid URL length issues)
    for chunk in TARGET_TOKENS.chunks(5) {
        let token_list = chunk.join(",");
        let url = format!(
            "https://api.dexscreener.com/latest/dex/tokens/{}",
            token_list
        );

        println!("📡 Fetching pools for {} tokens...", chunk.len());
        
        match client.get(&url).send().await {
            Ok(response) => {
                if let Ok(data) = response.json::<DexResponse>().await {
                    if let Some(pairs) = data.pairs {
                        for pair in pairs {
                            // Filter: Base chain only
                            if pair.chain_id.as_deref() != Some("base") {
                                continue;
                            }

                            // Filter: Skip duplicates
                            let addr_lower = pair.pair_address.to_lowercase();
                            if seen_addresses.contains(&addr_lower) {
                                continue;
                            }
                            seen_addresses.insert(addr_lower.clone());

                            // Filter: Minimum liquidity
                            let liquidity = pair.liquidity
                                .as_ref()
                                .and_then(|l| l.usd)
                                .unwrap_or(0.0);
                            
                            if liquidity < MIN_LIQUIDITY_USD {
                                continue;
                            }

                            let volume = pair.volume
                                .as_ref()
                                .and_then(|v| v.h24)
                                .unwrap_or(0.0);

                            // Determine pool type and fee from dex_id
                            let (pool_type, fee_tier) = classify_dex(&pair.dex_id, &pair.labels);

                            let ranked = RankedPool {
                                address: pair.pair_address,
                                dex: pair.dex_id.clone(),
                                pair: format!("{}/{}", pair.base_token.symbol, pair.quote_token.symbol),
                                base_symbol: pair.base_token.symbol,
                                quote_symbol: pair.quote_token.symbol,
                                liquidity_usd: liquidity,
                                volume_24h: volume,
                                fee_tier,
                                pool_type,
                            };

                            all_pools.push(ranked);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("⚠️  API error: {}", e);
            }
        }

        // Rate limit: DexScreener allows ~300 req/min
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    println!();
    println!("📊 DISCOVERY RESULTS:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("   Total pools found: {}", all_pools.len());

    // Sort by 24h volume (highest first) - volatility proxy
    all_pools.sort_by(|a, b| b.volume_24h.partial_cmp(&a.volume_24h).unwrap());

    // Take top N
    let golden_pools: Vec<_> = all_pools.into_iter().take(TOP_N_POOLS).collect();

    println!("   After filtering: {} (Top {} by volume)", golden_pools.len(), TOP_N_POOLS);
    println!();

    // Print the golden pools
    println!("🏆 THE GOLDEN {} POOLS:", golden_pools.len());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    for (i, pool) in golden_pools.iter().enumerate() {
        println!(
            "{:2}. {} | {} | Liq: ${:.0}K | Vol: ${:.0}K | {} | {}",
            i + 1,
            pool.pair,
            pool.dex,
            pool.liquidity_usd / 1000.0,
            pool.volume_24h / 1000.0,
            pool.fee_tier,
            pool.pool_type
        );
    }

    // Generate Rust code for pools_config.rs
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📝 GENERATING CODE...");
    println!();

    let mut output = String::new();
    output.push_str("// ════════════════════════════════════════════════════════════════════\n");
    output.push_str("// AUTO-GENERATED POOL CONFIGS - Golden 50\n");
    output.push_str(&format!("// Generated: {}\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
    output.push_str("// ════════════════════════════════════════════════════════════════════\n\n");

    for pool in &golden_pools {
        let fee_pct = parse_fee_percent(&pool.fee_tier);
        let pool_type_enum = if pool.pool_type == "V3" { "PoolType::UniswapV3" } else { "PoolType::V2Style" };
        
        // Most Base pools have WETH as token0, so invert_price = true
        // Exception: WETH/USDC where we want WETH price directly
        let invert = !pool.pair.starts_with("WETH/");

        output.push_str(&format!(
r#"PoolConfig {{
    address: "{}",
    dex_name: "{} {}",
    pair: "{}",
    fee_percent: {},
    pool_type: {},
    token0_decimals: 18,
    token1_decimals: {},
    invert_price: {},
}},
"#,
            pool.address,
            pool.dex,
            pool.base_symbol,
            pool.pair,
            fee_pct,
            pool_type_enum,
            if pool.quote_symbol == "USDC" || pool.quote_symbol == "USDT" { 6 } else { 18 },
            invert
        ));
    }

    // Write to file
    let mut file = File::create("discovered_pools.txt")?;
    file.write_all(output.as_bytes())?;

    println!("✅ Pool configs written to: discovered_pools.txt");
    println!();
    println!("📋 NEXT STEPS:");
    println!("   1. Review discovered_pools.txt");
    println!("   2. Copy the configs you want into src/bin/pools_config.rs");
    println!("   3. Run: cargo run --bin monitor");
    println!();

    // Also save raw JSON for reference
    let json_output: Vec<_> = golden_pools.iter().map(|p| {
        serde_json::json!({
            "address": p.address,
            "dex": p.dex,
            "pair": p.pair,
            "liquidity_usd": p.liquidity_usd,
            "volume_24h": p.volume_24h,
            "fee_tier": p.fee_tier,
            "pool_type": p.pool_type
        })
    }).collect();

    let mut json_file = File::create("discovered_pools.json")?;
    json_file.write_all(serde_json::to_string_pretty(&json_output)?.as_bytes())?;
    println!("📄 Raw data saved to: discovered_pools.json");

    Ok(())
}

fn classify_dex(dex_id: &str, labels: &Option<Vec<String>>) -> (String, String) {
    let is_v3 = labels.as_ref()
        .map(|l| l.iter().any(|label| label.contains("v3") || label.contains("V3")))
        .unwrap_or(false);

    let pool_type = if is_v3 || dex_id.contains("v3") || dex_id == "uniswap" {
        "V3".to_string()
    } else {
        "V2".to_string()
    };

    // Estimate fee tier based on DEX
    let fee = match dex_id {
        "uniswap" => "1.0%",      // Most meme pools are 1%
        "aerodrome" => "0.30%",
        "baseswap" => "0.30%",
        "sushiswap" => "0.30%",
        "alienbase" => "0.30%",
        "pancakeswap" => "0.25%",
        _ => "0.30%",
    };

    (pool_type, fee.to_string())
}

fn parse_fee_percent(fee_str: &str) -> f64 {
    fee_str.trim_end_matches('%').parse().unwrap_or(0.30)
}
