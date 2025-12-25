use axum::{routing::get, Router, response::Html};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ============================================================================
// Configuration
// ============================================================================

const PING_INTERVAL_MS: u64 = 100;
const CPU_STRESS_INTERVAL_MS: u64 = 1000;
const SHA256_ITERATIONS: u32 = 10_000;
const TEST_DURATION_SECS: u64 = 300; // 5 minutes

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

#[derive(Debug, Clone, Serialize)]
struct AuditReport {
    test_duration_secs: u64,
    ping_stats: LatencyStats,
    block_stats: BlockStats,
    cpu_stats: CpuStats,
    verdict: Verdict,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
struct BlockStats {
    blocks_received: usize,
    avg_gap_ms: f64,
    max_gap_ms: f64,
    missed_blocks: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CpuStats {
    count: usize,
    avg_ms: f64,
    baseline_ms: f64,
    max_ms: f64,
    spike_count: usize,
    max_slowdown_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
struct Verdict {
    ping_zone: String,
    jitter_zone: String,
    cpu_zone: String,
    overall: String,
    recommendation: String,
}

#[derive(Debug, Clone, Serialize, Default)]
struct LiveStatus {
    status: String,
    elapsed_secs: u64,
    ping_count: usize,
    block_count: usize,
    cpu_samples: usize,
    report: Option<AuditReport>,
    logs: Vec<String>,
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

type SharedStatus = Arc<RwLock<LiveStatus>>;


// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    let port: u16 = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string()).parse().unwrap_or(3000);
    let wss_url = std::env::var("RPC_WSS_URL")
        .expect("RPC_WSS_URL must be set");

    // Shared state for HTTP server
    let status: SharedStatus = Arc::new(RwLock::new(LiveStatus {
        status: "Starting...".to_string(),
        ..Default::default()
    }));

    add_log(&status, format!("🚀 Latency Auditor Starting"));
    add_log(&status, format!("📡 Target RPC: {}", mask_api_key(&wss_url)));
    add_log(&status, format!("⏱️  Test Duration: {} seconds", TEST_DURATION_SECS));

    // Start HTTP server
    let http_status = status.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route("/", get({
                let s = http_status.clone();
                move || serve_page(s.clone())
            }))
            .route("/api/status", get({
                let s = http_status.clone();
                move || serve_json(s.clone())
            }))
            .route("/health", get(|| async { "OK" }));

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
        println!("🌐 HTTP server listening on port {}", port);
        axum::serve(listener, app).await.unwrap();
    });

    // Give HTTP server time to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Run the audit
    run_audit(wss_url, status).await?;

    // Keep server running so results can be viewed
    println!("✅ Test complete! Results available at HTTP endpoint.");
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

async fn serve_page(status: SharedStatus) -> Html<String> {
    let data = status.read().unwrap().clone();
    let json = serde_json::to_string_pretty(&data).unwrap_or_default();
    
    let html = format!(r#"<!DOCTYPE html>
<html>
<head>
    <title>Latency Auditor</title>
    <meta http-equiv="refresh" content="5">
    <style>
        body {{ font-family: monospace; background: #1a1a2e; color: #eee; padding: 20px; }}
        pre {{ background: #16213e; padding: 20px; border-radius: 8px; overflow-x: auto; }}
        h1 {{ color: #e94560; }}
        .status {{ color: #0f3460; background: #e94560; padding: 10px; border-radius: 4px; display: inline-block; }}
    </style>
</head>
<body>
    <h1>🔬 Latency Auditor</h1>
    <p class="status">Status: {} | Elapsed: {}s | Pings: {} | Blocks: {}</p>
    <h2>Live Data (auto-refresh every 5s)</h2>
    <pre>{}</pre>
    <h2>Logs</h2>
    <pre>{}</pre>
</body>
</html>"#,
        data.status,
        data.elapsed_secs,
        data.ping_count,
        data.block_count,
        json,
        data.logs.join("\n")
    );
    
    Html(html)
}

async fn serve_json(status: SharedStatus) -> String {
    let data = status.read().unwrap().clone();
    serde_json::to_string_pretty(&data).unwrap_or_default()
}

fn add_log(status: &SharedStatus, msg: String) {
    println!("{}", msg);
    let _ = std::io::stdout().flush();
    if let Ok(mut s) = status.write() {
        s.logs.push(format!("[{}] {}", Utc::now().format("%H:%M:%S"), msg));
        if s.logs.len() > 100 {
            s.logs.remove(0);
        }
    }
}


async fn run_audit(wss_url: String, status: SharedStatus) -> Result<(), Box<dyn std::error::Error>> {
    let running = Arc::new(AtomicBool::new(true));
    let test_start = Instant::now();

    let (ping_tx, mut ping_rx) = mpsc::channel::<PingResult>(10000);
    let (block_tx, mut block_rx) = mpsc::channel::<BlockArrival>(1000);
    let (cpu_tx, mut cpu_rx) = mpsc::channel::<CpuStressResult>(5000);

    // Spawn ping loop
    let ping_running = running.clone();
    let ping_url = wss_url.clone();
    let ping_status = status.clone();
    tokio::spawn(async move {
        if let Err(e) = run_ping_loop(ping_url, ping_tx, ping_running, ping_status.clone()).await {
            add_log(&ping_status, format!("❌ Ping error: {}", e));
        }
    });

    // Spawn block subscription
    let block_running = running.clone();
    let block_url = wss_url.clone();
    let block_status = status.clone();
    tokio::spawn(async move {
        if let Err(e) = run_block_subscription(block_url, block_tx, block_running, block_status.clone()).await {
            add_log(&block_status, format!("❌ Block sub error: {}", e));
        }
    });

    // Spawn CPU stress test
    let cpu_running = running.clone();
    let cpu_status = status.clone();
    tokio::task::spawn_blocking(move || {
        run_cpu_stress_test(cpu_tx, cpu_running, cpu_status);
    });

    // Update status
    {
        let mut s = status.write().unwrap();
        s.status = "Running tests...".to_string();
    }

    let mut ping_results: Vec<PingResult> = Vec::new();
    let mut block_results: Vec<BlockArrival> = Vec::new();
    let mut cpu_results: Vec<CpuStressResult> = Vec::new();

    loop {
        tokio::select! {
            Some(ping) = ping_rx.recv() => {
                ping_results.push(ping);
            }
            Some(block) = block_rx.recv() => {
                add_log(&status, format!("📦 Block {} arrived", block.block_number));
                block_results.push(block);
            }
            Some(cpu) = cpu_rx.recv() => {
                cpu_results.push(cpu);
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let elapsed = test_start.elapsed().as_secs();
                
                // Update live status
                {
                    let mut s = status.write().unwrap();
                    s.elapsed_secs = elapsed;
                    s.ping_count = ping_results.len();
                    s.block_count = block_results.len();
                    s.cpu_samples = cpu_results.len();
                }

                if elapsed >= TEST_DURATION_SECS {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
    }

    add_log(&status, "⏹️ Test complete, generating report...".to_string());
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Drain remaining
    while let Ok(p) = ping_rx.try_recv() { ping_results.push(p); }
    while let Ok(b) = block_rx.try_recv() { block_results.push(b); }
    while let Ok(c) = cpu_rx.try_recv() { cpu_results.push(c); }

    let report = generate_report(&ping_results, &block_results, &cpu_results);

    // Update final status
    {
        let mut s = status.write().unwrap();
        s.status = "Complete".to_string();
        s.report = Some(report.clone());
    }

    add_log(&status, format!("📈 Avg RTT: {:.2}ms | P99: {:.2}ms", report.ping_stats.avg_ms, report.ping_stats.p99_ms));
    add_log(&status, format!("📦 Blocks: {} | Avg Gap: {:.0}ms", report.block_stats.blocks_received, report.block_stats.avg_gap_ms));
    add_log(&status, format!("🖥️ CPU Baseline: {:.2}ms | Spikes: {}", report.cpu_stats.baseline_ms, report.cpu_stats.spike_count));
    add_log(&status, format!("🎯 VERDICT: {}", report.verdict.overall));
    add_log(&status, format!("💡 {}", report.verdict.recommendation));

    Ok(())
}


// ============================================================================
// Ping Loop
// ============================================================================

async fn run_ping_loop(
    wss_url: String,
    tx: mpsc::Sender<PingResult>,
    running: Arc<AtomicBool>,
    status: SharedStatus,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = url::Url::parse(&wss_url)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    let request_id = Arc::new(AtomicU64::new(1));
    let mut interval = interval(Duration::from_millis(PING_INTERVAL_MS));

    add_log(&status, "✅ Ping loop connected".to_string());

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
        
        if write.send(msg).await.is_err() { continue; }

        let response = tokio::time::timeout(Duration::from_secs(5), read.next()).await;
        let rtt = start.elapsed();

        if let Ok(Some(Ok(Message::Text(text)))) = response {
            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&text) {
                if let Some(result) = resp.result {
                    let _ = tx.send(PingResult {
                        timestamp: Utc::now(),
                        rtt_micros: rtt.as_micros() as u64,
                        block_number: result.as_str().unwrap_or("?").to_string(),
                    }).await;
                }
            }
        }
    }
    Ok(())
}

// ============================================================================
// Block Subscription
// ============================================================================

async fn run_block_subscription(
    wss_url: String,
    tx: mpsc::Sender<BlockArrival>,
    running: Arc<AtomicBool>,
    status: SharedStatus,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    add_log(&status, "✅ Subscribed to newHeads".to_string());

    let mut last_block_time: Option<Instant> = None;
    let start = Instant::now();

    while running.load(Ordering::SeqCst) {
        let msg = tokio::time::timeout(Duration::from_secs(30), read.next()).await;

        if let Ok(Some(Ok(Message::Text(text)))) = msg {
            let arrival_time = Instant::now();
            if let Ok(sub_resp) = serde_json::from_str::<SubscriptionResponse>(&text) {
                if let Some(params) = sub_resp.params {
                    let gap = last_block_time.map(|t| arrival_time.duration_since(t).as_millis() as u64);
                    last_block_time = Some(arrival_time);

                    let _ = tx.send(BlockArrival {
                        timestamp: Utc::now(),
                        block_number: params.result.number,
                        block_timestamp: params.result.timestamp.as_ref().and_then(|ts| {
                            u64::from_str_radix(ts.trim_start_matches("0x"), 16).ok()
                        }),
                        local_arrival_micros: start.elapsed().as_micros() as u64,
                        gap_from_previous_ms: gap,
                    }).await;
                }
            }
        }
    }
    Ok(())
}

// ============================================================================
// CPU Stress Test
// ============================================================================

fn run_cpu_stress_test(
    tx: mpsc::Sender<CpuStressResult>,
    running: Arc<AtomicBool>,
    status: SharedStatus,
) {
    add_log(&status, "✅ CPU stress test started".to_string());

    while running.load(Ordering::SeqCst) {
        let start = Instant::now();

        let mut hasher = Sha256::new();
        let mut data = vec![0u8; 64];
        
        for i in 0..SHA256_ITERATIONS {
            data[0..4].copy_from_slice(&i.to_le_bytes());
            hasher.update(&data);
            let result = hasher.finalize_reset();
            data[4..36].copy_from_slice(&result[..32]);
        }

        let duration = start.elapsed();
        let _ = tx.blocking_send(CpuStressResult {
            timestamp: Utc::now(),
            duration_micros: duration.as_micros() as u64,
            iterations: SHA256_ITERATIONS,
        });

        std::thread::sleep(Duration::from_millis(CPU_STRESS_INTERVAL_MS).saturating_sub(duration));
    }
}


// ============================================================================
// Report Generation
// ============================================================================

fn generate_report(pings: &[PingResult], blocks: &[BlockArrival], cpu: &[CpuStressResult]) -> AuditReport {
    let ping_stats = calculate_ping_stats(pings);
    let block_stats = calculate_block_stats(blocks);
    let cpu_stats = calculate_cpu_stats(cpu);
    let verdict = determine_verdict(&ping_stats, &cpu_stats);

    AuditReport { test_duration_secs: TEST_DURATION_SECS, ping_stats, block_stats, cpu_stats, verdict }
}

fn calculate_ping_stats(pings: &[PingResult]) -> LatencyStats {
    if pings.is_empty() {
        return LatencyStats { count: 0, avg_ms: 0.0, min_ms: 0.0, max_ms: 0.0, p50_ms: 0.0, p95_ms: 0.0, p99_ms: 0.0, std_dev_ms: 0.0 };
    }

    let mut rtts: Vec<f64> = pings.iter().map(|p| p.rtt_micros as f64 / 1000.0).collect();
    rtts.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let count = rtts.len();
    let avg = rtts.iter().sum::<f64>() / count as f64;
    let variance = rtts.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / count as f64;

    LatencyStats {
        count,
        avg_ms: avg,
        min_ms: rtts[0],
        max_ms: rtts[count - 1],
        p50_ms: percentile(&rtts, 50.0),
        p95_ms: percentile(&rtts, 95.0),
        p99_ms: percentile(&rtts, 99.0),
        std_dev_ms: variance.sqrt(),
    }
}

fn calculate_block_stats(blocks: &[BlockArrival]) -> BlockStats {
    if blocks.is_empty() {
        return BlockStats { blocks_received: 0, avg_gap_ms: 0.0, max_gap_ms: 0.0, missed_blocks: 0 };
    }

    let gaps: Vec<f64> = blocks.iter().filter_map(|b| b.gap_from_previous_ms.map(|g| g as f64)).collect();
    let avg_gap = if gaps.is_empty() { 0.0 } else { gaps.iter().sum::<f64>() / gaps.len() as f64 };
    let max_gap = gaps.iter().cloned().fold(0.0, f64::max);
    let missed = gaps.iter().filter(|&&g| g > 5000.0).count();

    BlockStats { blocks_received: blocks.len(), avg_gap_ms: avg_gap, max_gap_ms: max_gap, missed_blocks: missed }
}

fn calculate_cpu_stats(cpu: &[CpuStressResult]) -> CpuStats {
    if cpu.is_empty() {
        return CpuStats { count: 0, avg_ms: 0.0, baseline_ms: 0.0, max_ms: 0.0, spike_count: 0, max_slowdown_percent: 0.0 };
    }

    let durations: Vec<f64> = cpu.iter().map(|c| c.duration_micros as f64 / 1000.0).collect();
    let count = durations.len();
    let avg = durations.iter().sum::<f64>() / count as f64;
    let max = durations.iter().cloned().fold(0.0, f64::max);

    let mut sorted = durations.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let baseline = percentile(&sorted, 10.0);

    let spike_count = durations.iter().filter(|&&d| d > baseline * 2.0).count();
    let max_slowdown = if baseline > 0.0 { ((max - baseline) / baseline) * 100.0 } else { 0.0 };

    CpuStats { count, avg_ms: avg, baseline_ms: baseline, max_ms: max, spike_count, max_slowdown_percent: max_slowdown }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn determine_verdict(ping: &LatencyStats, cpu: &CpuStats) -> Verdict {
    let ping_zone = if ping.avg_ms < 10.0 { "APEX" } else if ping.avg_ms < 50.0 { "VIABLE" } else { "FAIL" }.to_string();
    let jitter_zone = if ping.std_dev_ms < 2.0 { "APEX" } else if ping.std_dev_ms < 15.0 { "VIABLE" } else { "FAIL" }.to_string();
    let cpu_zone = if cpu.spike_count == 0 && cpu.max_slowdown_percent < 10.0 { "APEX" } else if cpu.max_slowdown_percent < 100.0 { "VIABLE" } else { "FAIL" }.to_string();

    let zones = [&ping_zone, &jitter_zone, &cpu_zone];
    let fail_count = zones.iter().filter(|z| **z == "FAIL").count();
    let apex_count = zones.iter().filter(|z| **z == "APEX").count();

    let (overall, recommendation) = if fail_count >= 2 {
        ("❌ FAIL - Infrastructure not suitable for HFT", "Use Limit Order / Laggy Controller strategy.")
    } else if fail_count == 1 {
        ("⚠️ MARGINAL - Some limitations", "Triangular arbitrage viable. Avoid MEV bot races.")
    } else if apex_count >= 2 {
        ("🏆 APEX - Excellent!", "Flash Swaps, Liquidity Sniping, Cross-DEX arbitrage all viable.")
    } else {
        ("✅ VIABLE - Good enough", "Focus on Triangular Arbitrage. Avoid millisecond races.")
    };

    Verdict { ping_zone, jitter_zone, cpu_zone, overall: overall.to_string(), recommendation: recommendation.to_string() }
}

fn mask_api_key(url: &str) -> String {
    if let Some(idx) = url.rfind('=') {
        let key = &url[idx + 1..];
        if key.len() > 8 { format!("{}{}...{}", &url[..idx + 1], &key[..4], &key[key.len() - 4..]) }
        else { url.to_string() }
    } else { url.to_string() }
}
