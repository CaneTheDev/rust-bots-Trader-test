//! Data types for liquidation tracking

use std::collections::HashMap;

/// Moonwell market (mToken) information
#[derive(Debug, Clone)]
pub struct Market {
    pub address: String,
    pub symbol: String,
    pub underlying: String,
    pub underlying_decimals: u8,
    pub collateral_factor: f64,  // e.g., 0.75 = 75%
    pub liquidation_incentive: f64,  // e.g., 1.08 = 8% bonus
    pub exchange_rate: f64,
    pub borrow_rate: f64,
}

/// A user's position in a single market
#[derive(Debug, Clone, Default)]
pub struct UserMarketPosition {
    pub m_token_balance: u128,  // Collateral (in mTokens)
    pub borrow_balance: u128,   // Debt (in underlying)
    pub entered: bool,          // Is this market used as collateral?
}

/// A borrower we're tracking
#[derive(Debug, Clone)]
pub struct Borrower {
    pub address: String,
    pub positions: HashMap<String, UserMarketPosition>,  // market_address -> position
    pub health_factor: f64,
    pub total_collateral_usd: f64,
    pub total_borrow_usd: f64,
    pub last_updated: u64,
}

impl Borrower {
    pub fn new(address: String) -> Self {
        Self {
            address,
            positions: HashMap::new(),
            health_factor: f64::MAX,
            total_collateral_usd: 0.0,
            total_borrow_usd: 0.0,
            last_updated: 0,
        }
    }
    
    pub fn is_liquidatable(&self) -> bool {
        self.health_factor < 1.0 && self.total_borrow_usd > 0.0
    }
}

/// A potential liquidation opportunity
#[derive(Debug, Clone)]
pub struct LiquidationOpportunity {
    pub borrower: String,
    pub debt_market: String,      // Market to repay
    pub debt_symbol: String,
    pub collateral_market: String, // Market to seize
    pub collateral_symbol: String,
    pub repay_amount: f64,        // USD value to repay
    pub seize_amount: f64,        // USD value of collateral seized
    pub profit_usd: f64,          // Expected profit after gas
    pub health_factor: f64,
    pub gas_estimate_usd: f64,
}

/// Oracle price data
#[derive(Debug, Clone)]
pub struct PriceData {
    pub symbol: String,
    pub price_usd: f64,
    pub decimals: u8,
    pub last_updated: u64,
}

/// Statistics for monitoring
#[derive(Debug, Default)]
pub struct BotStats {
    pub blocks_processed: u64,
    pub borrowers_tracked: usize,
    pub risky_borrowers: usize,  // Health < threshold
    pub opportunities_found: u64,
    pub simulated_liquidations: u64,
    pub simulated_profit: f64,
    pub live_liquidations: u64,
    pub live_profit: f64,
}
