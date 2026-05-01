use eyre::{Context, Result};
use flashstat_common::Config;
use flashstat_core::FlashMonitor;
use tokio::sync::broadcast;
use tracing::{error, info};

fn init_logging() {
    tracing_subscriber::fmt::init();
}

fn load_configuration() -> Result<Config> {
    let config = Config::load().context(
        "Failed to load configuration. Ensure flashstat.toml exists or env vars are set!",
    )?;
    info!("🏮 Config loaded: WS={}", config.rpc.ws_url);
    Ok(config)
}

fn setup_shutdown_signal() -> broadcast::Sender<()> {
    let (shutdown_tx, _) = broadcast::channel(1);
    let shutdown_tx_signal = shutdown_tx.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl_c");
        info!("👋 Shutdown signal received (Ctrl+C)");
        let _ = shutdown_tx_signal.send(());
    });

    shutdown_tx
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let config = load_configuration()?;
    let shutdown_tx = setup_shutdown_signal();

    let storage = std::sync::Arc::new(flashstat_db::RedbStorage::new(&config.storage.db_path)?);
    let monitor = FlashMonitor::new(config, storage, shutdown_tx.subscribe()).await?;

    if let Err(e) = monitor.run().await {
        error!("Fatal monitor error: {:?}", e);
        return Err(e);
    }

    info!("🏁 FlashStat exited gracefully");
    Ok(())
}
