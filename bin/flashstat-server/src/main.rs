use flashstat_api::FlashApiServer;
use flashstat_common::{FlashBlock, ReorgEvent, Config};
use flashstat_db::{FlashStorage, RocksStorage};
use ethers::types::H256;
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::types::error::ErrorObjectOwned;
use std::sync::Arc;
use eyre::Context;
use tracing::info;

pub struct FlashServer {
    storage: Arc<dyn FlashStorage>,
}

#[async_trait]
impl FlashApiServer for FlashServer {
    async fn get_confidence(&self, hash: H256) -> RpcResult<f64> {
        match self.storage.get_block(hash).await {
            Ok(Some(block)) => Ok(block.confidence),
            Ok(None) => Err(ErrorObjectOwned::owned(-32602, "Block not found", None::<()>)),
            Err(e) => Err(ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>)),
        }
    }

    async fn get_latest_block(&self) -> RpcResult<Option<FlashBlock>> {
        self.storage.get_latest_block().await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_recent_reorgs(&self, limit: usize) -> RpcResult<Vec<ReorgEvent>> {
        self.storage.get_latest_reorgs(limit).await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_equivocations(&self, limit: usize) -> RpcResult<Vec<ReorgEvent>> {
        self.storage.get_equivocations(limit).await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load().context("Failed to load config")?;
    let storage = Arc::new(RocksStorage::new_readonly(&config.storage.db_path)?);
    
    let server = ServerBuilder::default().build("127.0.0.1:9944").await?;
    let handle = server.start(FlashServer { storage }.into_rpc());

    info!("🏮 FlashStat JSON-RPC Server started at 127.0.0.1:9944");

    handle.stopped().await;
    Ok(())
}
