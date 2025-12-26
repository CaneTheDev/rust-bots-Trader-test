//! Moonwell Liquidation Bot - Base Mainnet
//! 
//! Monitors Moonwell lending protocol for liquidation opportunities.
//! Uses flash loans for zero-capital liquidations.
//! 
//! Run with: cargo run --release

mod liquidation;

use liquidation::{LiquidationBot, Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    
    let config = Config::from_env()?;
    
    println!("===============================================================");
    println!("   MOONWELL LIQUIDATION BOT v1.0 - Base Mainnet");
    println!("   Zero-Capital Flash Loan Liquidations");
    println!("===============================================================");
    println!();
    println!("Mode:              {}", if config.simulation { "SIMULATION (no real trades)" } else { "LIVE" });
    println!("Min Profit:        ${:.2}", config.min_profit_usd);
    println!("Health Threshold:  {:.2}", config.health_threshold);
    println!();
    
    let mut bot = LiquidationBot::new(config).await?;
    bot.run().await?;
    
    Ok(())
}
