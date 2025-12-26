//! Moonwell Liquidation Module
//! 
//! Core components:
//! - Config: Bot configuration
//! - LiquidationBot: Main bot logic
//! - Borrower tracking and health factor calculation
//! - Oracle price monitoring

mod config;
mod bot;
mod contracts;
mod types;

pub use config::Config;
pub use bot::LiquidationBot;
pub use types::*;
