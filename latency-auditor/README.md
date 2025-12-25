# Latency Auditor 🔬

Infrastructure benchmark for blockchain arbitrage viability. Measures network RTT, block propagation, and CPU stability to determine if your deployment can compete in DeFi.

## Quick Start

### 1. Get an RPC Endpoint

Sign up at [DRPC](https://drpc.org) (easy GitHub/Google login, no CAPTCHA hassle).

**Recommended:** Use Base (Coinbase L2) - optimized OP Stack with fast block times.

### 2. Local Testing

```bash
cp .env.example .env
# Edit .env with your RPC_WSS_URL

cargo run --release
```

### 3. Deploy to Railway

```bash
# Install Railway CLI
npm install -g @railway/cli

# Login and deploy
railway login
railway init
railway up

# Set environment variable
railway variables set RPC_WSS_URL=wss://base-mainnet.g.alchemy.com/v2/YOUR_KEY
```

## What It Measures

| Test | Purpose |
|------|---------|
| **Ping Loop** | RTT to RPC endpoint (100ms intervals) |
| **Block Subscription** | newHeads arrival timing & consistency |
| **CPU Stress** | 10K SHA-256 hashes to detect throttling |

## Interpreting Results

### Scorecard

| Metric | 🏆 APEX | ✅ VIABLE | ❌ FAIL |
|--------|---------|-----------|---------|
| Ping (RTT) | < 10ms | 10-50ms | > 100ms |
| Jitter (StdDev) | < 2ms | < 15ms | > 50ms |
| CPU Spike | None | < 100% slowdown | > 100% slowdown |

### Strategy Recommendations

- **APEX Zone**: Flash Swaps, Liquidity Sniping, Cross-DEX arbitrage
- **VIABLE Zone**: Triangular arbitrage, less competitive opportunities
- **FAIL Zone**: Limit orders, longer timeframe strategies

## Configuration

Edit `src/main.rs` constants:

```rust
const PING_INTERVAL_MS: u64 = 100;      // How often to ping
const TEST_DURATION_SECS: u64 = 300;    // 5 min default, use 3600 for 1 hour
const SHA256_ITERATIONS: u32 = 10_000;  // CPU stress intensity
```

## Output

JSON report with full statistics + human-readable verdict:

```json
{
  "ping_stats": {
    "avg_ms": 12.5,
    "p99_ms": 25.3,
    "std_dev_ms": 3.2
  },
  "verdict": {
    "overall": "✅ VIABLE",
    "recommendation": "Focus on Triangular Arbitrage..."
  }
}
```
