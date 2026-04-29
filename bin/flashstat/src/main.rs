use eyre::{Context, Result};
use flashstat_common::Config;
use flashstat_core::FlashMonitor;
use tokio::sync::broadcast;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize Logging
    tracing_subscriber::fmt::init();

    // 2. Load Configuration
    let config = Config::load().context(
        "Failed to load configuration. Ensure flashstat.toml exists or env vars are set.",
    )?;
    info!("🏮 Config loaded: WS={}", config.rpc.ws_url);

    // 3. Setup Shutdown Coordination
    let (shutdown_tx, _) = broadcast::channel(1);
    let shutdown_tx_signal = shutdown_tx.clone();

    // 4. Handle OS Signals
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
        info!("👋 Shutdown signal received (Ctrl+C)");
        let _ = shutdown_tx_signal.send(());
    });

    // 5. Run Monitor
    let mut monitor = FlashMonitor::new(config, shutdown_tx.subscribe()).await?;

    if let Err(e) = monitor.run().await {
        error!("Fatal monitor error: {:?}", e);
        return Err(e);
    }

    info!("🏁 FlashStat exited gracefully");
    Ok(())
}
