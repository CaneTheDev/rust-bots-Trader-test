//! Configuration for the liquidation bot

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub rpc_wss_url: String,
    pub rpc_http_url: String,
    pub private_key: Option<String>,
    pub simulation: bool,
    pub min_profit_usd: f64,
    pub health_threshold: f64,  // Monitor users below this (e.g., 1.1)
    pub liquidation_threshold: f64,  // Liquidate when below this (1.0)
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let rpc_wss_url = env::var("RPC_WSS_URL")
            .expect("RPC_WSS_URL must be set in .env");
        
        let rpc_http_url = rpc_wss_url
            .replace("wss://", "https://")
            .replace("ws://", "http://")
            .replace("/ws", "");
        
        let private_key = env::var("PRIVATE_KEY").ok();
        
        // Default to simulation mode if no private key
        let simulation = private_key.is_none() || 
            env::var("SIMULATION").map(|v| v == "true" || v == "1").unwrap_or(true);
        
        let min_profit_usd = env::var("MIN_PROFIT_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
        
        let health_threshold = env::var("HEALTH_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.1);
        
        Ok(Config {
            rpc_wss_url,
            rpc_http_url,
            private_key,
            simulation,
            min_profit_usd,
            health_threshold,
            liquidation_threshold: 1.0,
        })
    }
}
