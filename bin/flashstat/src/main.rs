use flashstat_core::FlashMonitor;
use eyre::Result;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured logging
    tracing_subscriber::fmt::init();

    let rpc_url = "wss://sepolia.unichain.org"; // Mainnet or Testnet WS
    let storage_path = "./data/flashstat_db";

    let monitor = FlashMonitor::new(rpc_url, storage_path).await?;
    monitor.run().await?;

    Ok(())
}
