use flashstat_api::FlashApiServer;
use flashstat_common::{FlashBlock, ReorgEvent, Config};
use flashstat_db::{FlashStorage, RocksStorage};
use ethers::types::H256;
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::core::async_trait;
use std::sync::Arc;
use eyre::{Result, Context};
use tracing::info;

pub struct FlashServer {
    storage: Arc<dyn FlashStorage>,
}

#[async_trait]
impl FlashApiServer for FlashServer {
    async fn get_confidence(&self, hash: H256) -> Result<f64, jsonrpsee::core::Error> {
        match self.storage.get_block(hash).await {
            Ok(Some(block)) => Ok(block.confidence),
            Ok(None) => Err(jsonrpsee::core::Error::Custom("Block not found".to_string())),
            Err(e) => Err(jsonrpsee::core::Error::Custom(e.to_string())),
        }
    }

    async fn get_latest_block(&self) -> Result<Option<FlashBlock>, jsonrpsee::core::Error> {
        // For simplicity, this requires a more advanced storage implementation
        // or a cache in the server. Returning None for now.
        Ok(None)
    }

    async fn get_recent_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>, jsonrpsee::core::Error> {
        self.storage.get_latest_reorgs(limit).await
            .map_err(|e| jsonrpsee::core::Error::Custom(e.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load().context("Failed to load config")?;
    let storage = Arc::new(RocksStorage::new_readonly(&config.storage.db_path)?);
    
    let server = ServerBuilder::default().build("127.0.0.1:9944").await?;
    let handle = server.start(FlashServer { storage }.into_rpc());

    info!("🏮 FlashStat JSON-RPC Server started at 127.0.0.1:9944");

    handle.stopped().await;
    Ok(())
}
