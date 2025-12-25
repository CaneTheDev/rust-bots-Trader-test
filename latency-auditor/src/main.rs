use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

// ============================================================================
// Configuration
// ============================================================================

const PING_INTERVAL_MS: u64 = 100;
const CPU_STRESS_INTERVAL_MS: u64 = 1000;
const SHA256_ITERATIONS: u32 = 10_000;
const TEST_DURATION_SECS: u64 = 300; // 5 minutes for quick test, change to 3600 for 1 hour

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone, Serialize)]
struct PingResult {
    timestamp: DateTime<Utc>,
    rtt_micros: u64,
    block_number: String,
}

#[derive(Debug, Clone, Serialize)]
struct BlockArrival {
    timestamp: DateTime<Utc>,
    block_number: String,
    block_timestamp: Option<u64>,
    local_arrival_micros: u64,
    gap_from_previous_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct CpuStressResult {
    timestamp: DateTime<Utc>,
    duration_micros: u64,
    iterations: u32,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    test_duration_secs: u64,
    ping_stats: LatencyStats,
    block_stats: BlockStats,
    cpu_stats: CpuStats,
    verdict: Verdict,
}

#[derive(Debug, Serialize)]
struct LatencyStats {
    count: usize,
    avg_ms: f64,
    min_ms: f64,
    max_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    std_dev_ms: f64,
}

#[derive(Debug, Serialize)]
struct BlockStats {
    blocks_received: usize,
    avg_gap_ms: f64,
    max_gap_ms: f64,
    missed_blocks: usize,
}

#[derive(Debug, Serialize)]
struct CpuStats {
    count: usize,
    avg_ms: f64,
    baseline_ms: f64,
    max_ms: f64,
    spike_count: usize,
    max_slowdown_percent: f64,
}

#[derive(Debug, Serialize)]
struct Verdict {
    ping_zone: String,
    jitter_zone: String,
    cpu_zone: String,
    overall: String,
    recommendation: String,
}

// JSON-RPC structures
#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: &'static str,
    params: Vec<serde_json::Value>,
    id: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    result: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct SubscriptionResponse {
    params: Option<SubscriptionParams>,
}

#[derive(Deserialize)]
struct SubscriptionParams {
    result: NewHeadResult,
}

#[derive(Deserialize)]
struct NewHeadResult {
    number: String,
    timestamp: Option<String>,
}


// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("latency_auditor=info".parse().unwrap()),
        )
        .json()
        .init();

    dotenv::dotenv().ok();

    let wss_url = std::env::var("RPC_WSS_URL")
        .expect("RPC_WSS_URL must be set (e.g., wss://base-mainnet.g.alchemy.com/v2/YOUR_KEY)");

    info!("🚀 Latency Auditor Starting");
    info!("📡 Target RPC: {}", mask_api_key(&wss_url));
    info!("⏱️  Test Duration: {} seconds", TEST_DURATION_SECS);
    info!("🔄 Ping Interval: {}ms", PING_INTERVAL_MS);

    let running = Arc::new(AtomicBool::new(true));
    let test_start = Instant::now();

    // Channels for collecting results
    let (ping_tx, mut ping_rx) = mpsc::channel::<PingResult>(10000);
    let (block_tx, mut block_rx) = mpsc::channel::<BlockArrival>(1000);
    let (cpu_tx, mut cpu_rx) = mpsc::channel::<CpuStressResult>(5000);

    // Spawn the ping loop
    let ping_running = running.clone();
    let ping_url = wss_url.clone();
    let _ping_handle = tokio::spawn(async move {
        if let Err(e) = run_ping_loop(ping_url, ping_tx, ping_running).await {
            error!("Ping loop error: {}", e);
        }
    });

    // Spawn the block subscription
    let block_running = running.clone();
    let block_url = wss_url.clone();
    let _block_handle = tokio::spawn(async move {
        if let Err(e) = run_block_subscription(block_url, block_tx, block_running).await {
            error!("Block subscription error: {}", e);
        }
    });

    // Spawn the CPU stress test (blocking, runs in dedicated thread)
    let cpu_running = running.clone();
    let _cpu_handle = tokio::task::spawn_blocking(move || {
        run_cpu_stress_test(cpu_tx, cpu_running);
    });

    // Collect results
    let mut ping_results: Vec<PingResult> = Vec::new();
    let mut block_results: Vec<BlockArrival> = Vec::new();
    let mut cpu_results: Vec<CpuStressResult> = Vec::new();

    // Progress reporting
    let mut last_report = Instant::now();
    let report_interval = Duration::from_secs(30);

    loop {
        tokio::select! {
            Some(ping) = ping_rx.recv() => {
                ping_results.push(ping);
            }
            Some(block) = block_rx.recv() => {
                info!("📦 Block {} arrived", block.block_number);
                block_results.push(block);
            }
            Some(cpu) = cpu_rx.recv() => {
                cpu_results.push(cpu);
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Check if test is complete
                if test_start.elapsed() >= Duration::from_secs(TEST_DURATION_SECS) {
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                // Progress report
                if last_report.elapsed() >= report_interval {
                    let elapsed = test_start.elapsed().as_secs();
                    let remaining = TEST_DURATION_SECS.saturating_sub(elapsed);
                    info!(
                        "📊 Progress: {}s elapsed, {}s remaining | Pings: {} | Blocks: {} | CPU samples: {}",
                        elapsed, remaining, ping_results.len(), block_results.len(), cpu_results.len()
                    );
                    last_report = Instant::now();
                }
            }
        }
    }

    info!("⏹️  Test complete, collecting final results...");

    // Give tasks time to finish
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Drain remaining results
    while let Ok(ping) = ping_rx.try_recv() {
        ping_results.push(ping);
    }
    while let Ok(block) = block_rx.try_recv() {
        block_results.push(block);
    }
    while let Ok(cpu) = cpu_rx.try_recv() {
        cpu_results.push(cpu);
    }

    // Generate report
    let report = generate_report(&ping_results, &block_results, &cpu_results);

    // Output final report
    info!("═══════════════════════════════════════════════════════════════");
    info!("                    LATENCY AUDIT REPORT                        ");
    info!("═══════════════════════════════════════════════════════════════");
    
    println!("\n{}", serde_json::to_string_pretty(&report)?);

    // Also log key metrics
    info!("📈 PING STATS:");
    info!("   Average RTT: {:.2}ms", report.ping_stats.avg_ms);
    info!("   P99 RTT: {:.2}ms", report.ping_stats.p99_ms);
    info!("   Std Dev: {:.2}ms", report.ping_stats.std_dev_ms);
    
    info!("📦 BLOCK STATS:");
    info!("   Blocks Received: {}", report.block_stats.blocks_received);
    info!("   Avg Gap: {:.2}ms", report.block_stats.avg_gap_ms);
    
    info!("🖥️  CPU STATS:");
    info!("   Baseline: {:.2}ms", report.cpu_stats.baseline_ms);
    info!("   Max Slowdown: {:.1}%", report.cpu_stats.max_slowdown_percent);
    info!("   Spike Count: {}", report.cpu_stats.spike_count);
    
    info!("═══════════════════════════════════════════════════════════════");
    info!("🎯 VERDICT: {}", report.verdict.overall);
    info!("💡 {}", report.verdict.recommendation);
    info!("═══════════════════════════════════════════════════════════════");

    Ok(())
}


// ============================================================================
// Ping Loop - Measures RTT to RPC endpoint
// ============================================================================

async fn run_ping_loop(
    wss_url: String,
    tx: mpsc::Sender<PingResult>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = url::Url::parse(&wss_url)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    let request_id = Arc::new(AtomicU64::new(1));
    let mut interval = interval(Duration::from_millis(PING_INTERVAL_MS));

    info!("✅ Ping loop connected to WebSocket");

    while running.load(Ordering::SeqCst) {
        interval.tick().await;

        let id = request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_blockNumber",
            params: vec![],
            id,
        };

        let start = Instant::now();
        let msg = Message::Text(serde_json::to_string(&request)?);
        
        if let Err(e) = write.send(msg).await {
            warn!("Failed to send ping: {}", e);
            continue;
        }

        // Wait for response with timeout
        let response = tokio::time::timeout(Duration::from_secs(5), read.next()).await;

        let rtt = start.elapsed();

        match response {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&text) {
                    if let Some(result) = resp.result {
                        let block_number = result.as_str().unwrap_or("unknown").to_string();
                        let ping_result = PingResult {
                            timestamp: Utc::now(),
                            rtt_micros: rtt.as_micros() as u64,
                            block_number,
                        };
                        let _ = tx.send(ping_result).await;
                    }
                }
            }
            Ok(Some(Ok(_))) => {} // Ignore non-text messages
            Ok(Some(Err(e))) => warn!("WebSocket error: {}", e),
            Ok(None) => {
                warn!("WebSocket closed");
                break;
            }
            Err(_) => warn!("Ping timeout"),
        }
    }

    Ok(())
}

// ============================================================================
// Block Subscription - Measures block propagation timing
// ============================================================================

async fn run_block_subscription(
    wss_url: String,
    tx: mpsc::Sender<BlockArrival>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = url::Url::parse(&wss_url)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    // Subscribe to newHeads
    let subscribe_request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_subscribe",
        "params": ["newHeads"],
        "id": 1
    });

    write.send(Message::Text(subscribe_request.to_string())).await?;
    info!("✅ Subscribed to newHeads");

    let mut last_block_time: Option<Instant> = None;
    let start = Instant::now();

    while running.load(Ordering::SeqCst) {
        let msg = tokio::time::timeout(Duration::from_secs(30), read.next()).await;

        match msg {
            Ok(Some(Ok(Message::Text(text)))) => {
                let arrival_time = Instant::now();
                let arrival_micros = start.elapsed().as_micros() as u64;

                // Try to parse as subscription notification
                if let Ok(sub_resp) = serde_json::from_str::<SubscriptionResponse>(&text) {
                    if let Some(params) = sub_resp.params {
                        let gap = last_block_time.map(|t| arrival_time.duration_since(t).as_millis() as u64);
                        last_block_time = Some(arrival_time);

                        let block_timestamp = params.result.timestamp.as_ref().and_then(|ts| {
                            u64::from_str_radix(ts.trim_start_matches("0x"), 16).ok()
                        });

                        let block_arrival = BlockArrival {
                            timestamp: Utc::now(),
                            block_number: params.result.number,
                            block_timestamp,
                            local_arrival_micros: arrival_micros,
                            gap_from_previous_ms: gap,
                        };

                        let _ = tx.send(block_arrival).await;
                    }
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(e))) => warn!("Block subscription error: {}", e),
            Ok(None) => {
                warn!("Block subscription closed");
                break;
            }
            Err(_) => {} // Timeout is normal if no blocks
        }
    }

    Ok(())
}


// ============================================================================
// CPU Stress Test - Detects "Noisy Neighbor" throttling
// ============================================================================

fn run_cpu_stress_test(
    tx: mpsc::Sender<CpuStressResult>,
    running: Arc<AtomicBool>,
) {
    info!("✅ CPU stress test started");

    while running.load(Ordering::SeqCst) {
        let start = Instant::now();

        // Perform SHA-256 calculations
        let mut hasher = Sha256::new();
        let mut data = vec![0u8; 64];
        
        for i in 0..SHA256_ITERATIONS {
            data[0..4].copy_from_slice(&i.to_le_bytes());
            hasher.update(&data);
            let result = hasher.finalize_reset();
            data[4..36].copy_from_slice(&result[..32]);
        }

        let duration = start.elapsed();

        let result = CpuStressResult {
            timestamp: Utc::now(),
            duration_micros: duration.as_micros() as u64,
            iterations: SHA256_ITERATIONS,
        };

        // Use blocking send since we're not in async context
        let _ = tx.blocking_send(result);

        // Sleep until next interval
        let sleep_time = Duration::from_millis(CPU_STRESS_INTERVAL_MS)
            .saturating_sub(duration);
        std::thread::sleep(sleep_time);
    }
}

// ============================================================================
// Report Generation & Analysis
// ============================================================================

fn generate_report(
    pings: &[PingResult],
    blocks: &[BlockArrival],
    cpu: &[CpuStressResult],
) -> AuditReport {
    let ping_stats = calculate_ping_stats(pings);
    let block_stats = calculate_block_stats(blocks);
    let cpu_stats = calculate_cpu_stats(cpu);
    let verdict = determine_verdict(&ping_stats, &cpu_stats);

    AuditReport {
        test_duration_secs: TEST_DURATION_SECS,
        ping_stats,
        block_stats,
        cpu_stats,
        verdict,
    }
}

fn calculate_ping_stats(pings: &[PingResult]) -> LatencyStats {
    if pings.is_empty() {
        return LatencyStats {
            count: 0,
            avg_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            std_dev_ms: 0.0,
        };
    }

    let mut rtts: Vec<f64> = pings.iter().map(|p| p.rtt_micros as f64 / 1000.0).collect();
    rtts.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let count = rtts.len();
    let sum: f64 = rtts.iter().sum();
    let avg = sum / count as f64;

    let variance: f64 = rtts.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / count as f64;
    let std_dev = variance.sqrt();

    LatencyStats {
        count,
        avg_ms: avg,
        min_ms: rtts[0],
        max_ms: rtts[count - 1],
        p50_ms: percentile(&rtts, 50.0),
        p95_ms: percentile(&rtts, 95.0),
        p99_ms: percentile(&rtts, 99.0),
        std_dev_ms: std_dev,
    }
}

fn calculate_block_stats(blocks: &[BlockArrival]) -> BlockStats {
    if blocks.is_empty() {
        return BlockStats {
            blocks_received: 0,
            avg_gap_ms: 0.0,
            max_gap_ms: 0.0,
            missed_blocks: 0,
        };
    }

    let gaps: Vec<f64> = blocks
        .iter()
        .filter_map(|b| b.gap_from_previous_ms.map(|g| g as f64))
        .collect();

    let avg_gap = if gaps.is_empty() {
        0.0
    } else {
        gaps.iter().sum::<f64>() / gaps.len() as f64
    };

    let max_gap = gaps.iter().cloned().fold(0.0, f64::max);

    // Estimate missed blocks (gaps > 3x average, assuming ~2s block time for Base)
    let expected_gap = 2000.0; // 2 seconds for Base
    let missed = gaps.iter().filter(|&&g| g > expected_gap * 2.5).count();

    BlockStats {
        blocks_received: blocks.len(),
        avg_gap_ms: avg_gap,
        max_gap_ms: max_gap,
        missed_blocks: missed,
    }
}

fn calculate_cpu_stats(cpu: &[CpuStressResult]) -> CpuStats {
    if cpu.is_empty() {
        return CpuStats {
            count: 0,
            avg_ms: 0.0,
            baseline_ms: 0.0,
            max_ms: 0.0,
            spike_count: 0,
            max_slowdown_percent: 0.0,
        };
    }

    let durations: Vec<f64> = cpu.iter().map(|c| c.duration_micros as f64 / 1000.0).collect();
    
    let count = durations.len();
    let avg = durations.iter().sum::<f64>() / count as f64;
    let max = durations.iter().cloned().fold(0.0, f64::max);

    // Baseline is the 10th percentile (best performance)
    let mut sorted = durations.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let baseline = percentile(&sorted, 10.0);

    // Spikes are > 2x baseline
    let spike_threshold = baseline * 2.0;
    let spike_count = durations.iter().filter(|&&d| d > spike_threshold).count();

    let max_slowdown = if baseline > 0.0 {
        ((max - baseline) / baseline) * 100.0
    } else {
        0.0
    };

    CpuStats {
        count,
        avg_ms: avg,
        baseline_ms: baseline,
        max_ms: max,
        spike_count,
        max_slowdown_percent: max_slowdown,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}


fn determine_verdict(ping: &LatencyStats, cpu: &CpuStats) -> Verdict {
    // Ping zone determination
    let ping_zone = if ping.avg_ms < 10.0 {
        "APEX".to_string()
    } else if ping.avg_ms < 50.0 {
        "VIABLE".to_string()
    } else {
        "FAIL".to_string()
    };

    // Jitter zone (based on std deviation)
    let jitter_zone = if ping.std_dev_ms < 2.0 {
        "APEX".to_string()
    } else if ping.std_dev_ms < 15.0 {
        "VIABLE".to_string()
    } else {
        "FAIL".to_string()
    };

    // CPU zone
    let cpu_zone = if cpu.spike_count == 0 && cpu.max_slowdown_percent < 10.0 {
        "APEX".to_string()
    } else if cpu.max_slowdown_percent < 100.0 {
        "VIABLE".to_string()
    } else {
        "FAIL".to_string()
    };

    // Overall verdict
    let zones = [&ping_zone, &jitter_zone, &cpu_zone];
    let fail_count = zones.iter().filter(|z| **z == "FAIL").count();
    let apex_count = zones.iter().filter(|z| **z == "APEX").count();

    let (overall, recommendation) = if fail_count >= 2 {
        (
            "❌ FAIL - Infrastructure not suitable for HFT".to_string(),
            "Use Limit Order / Laggy Controller strategy. Focus on longer timeframe opportunities.".to_string(),
        )
    } else if fail_count == 1 {
        (
            "⚠️ MARGINAL - Some limitations detected".to_string(),
            "Triangular arbitrage on smaller tokens is viable. Avoid head-to-head races with MEV bots.".to_string(),
        )
    } else if apex_count >= 2 {
        (
            "🏆 APEX - Excellent infrastructure!".to_string(),
            "You can compete with aggressive strategies: Flash Swaps, Liquidity Sniping, Cross-DEX arbitrage.".to_string(),
        )
    } else {
        (
            "✅ VIABLE - Good enough for profitable arbitrage".to_string(),
            "Focus on Triangular Arbitrage and less competitive opportunities. Avoid millisecond races.".to_string(),
        )
    };

    Verdict {
        ping_zone,
        jitter_zone,
        cpu_zone,
        overall,
        recommendation,
    }
}

fn mask_api_key(url: &str) -> String {
    // Mask API key in URL for logging
    if let Some(idx) = url.rfind('/') {
        let key_part = &url[idx + 1..];
        if key_part.len() > 8 {
            format!("{}{}...{}", &url[..idx + 1], &key_part[..4], &key_part[key_part.len() - 4..])
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    }
}
