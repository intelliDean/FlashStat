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
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
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

    async fn subscribe_blocks(&self, pending: jsonrpsee::PendingSubscriptionSink) -> jsonrpsee::core::SubscriptionResult {
        let mut rx = self.block_tx.subscribe();
        let sink = pending.accept().await?;
        
        tokio::spawn(async move {
            while let Ok(block) = rx.recv().await {
                if sink.send(block).await.is_err() {
                    break;
                }
            }
        });
        
        Ok(())
    }

    async fn subscribe_events(&self, pending: jsonrpsee::PendingSubscriptionSink) -> jsonrpsee::core::SubscriptionResult {
        let mut rx = self.event_tx.subscribe();
        let sink = pending.accept().await?;
        
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if sink.send(event).await.is_err() {
                    break;
                }
            }
        });
        
        Ok(())
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load().context("Failed to load config")?;
    
    // 1. Initialize Shutdown Signal
    let (shutdown_tx, _) = broadcast::channel(1);
    
    // 2. Initialize Monitor (which manages storage)
    let mut monitor = flashstat_core::FlashMonitor::new(config.clone(), shutdown_tx.subscribe()).await?;
    let storage = monitor.storage();
    let block_tx = monitor.block_notifier();
    let event_tx = monitor.event_notifier();

    // 3. Start Monitor in background
    tokio::spawn(async move {
        if let Err(e) = monitor.run().await {
            tracing::error!("Monitor error: {:?}", e);
        }
    });
    
    // 4. Start JSON-RPC Server with Pub/Sub support
    let server = ServerBuilder::default().build("127.0.0.1:9944").await?;
    let handle = server.start(FlashServer { 
        storage,
        block_tx,
        event_tx 
    }.into_rpc());

    info!("🏮 FlashStat JSON-RPC Server (with Pub/Sub) started at 127.0.0.1:9944");

    handle.stopped().await;
    Ok(())
}
