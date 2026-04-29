use flashstat_api::FlashApiServer;
use flashstat_common::{Config, FlashBlock, ReorgEvent};
use flashstat_db::FlashStorage;
use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::types::error::ErrorObjectOwned;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
use tracing::info;

pub struct FlashServer {
    storage: Arc<dyn FlashStorage>,
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
    total_reorgs: AtomicU64,
}

#[async_trait]
impl FlashApiServer for FlashServer {
    async fn get_confidence(&self, hash: ethers::types::H256) -> RpcResult<f64> {
        let block = self
            .storage
            .get_block(hash)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))?;

        match block {
            Some(b) => Ok(b.confidence),
            None => Err(ErrorObjectOwned::owned(-32602, "Block not found", None::<()>)),
        }
    }

    async fn get_latest_block(&self) -> RpcResult<Option<FlashBlock>> {
        self.storage
            .get_latest_block()
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_health(&self) -> RpcResult<flashstat_common::HealthStatus> {
        let db_size = std::fs::metadata(&self.storage.db_path())
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(flashstat_common::HealthStatus {
            status: "healthy".to_string(),
            uptime_secs: 0, // TODO: Track uptime
            total_reorgs: self.total_reorgs.load(Ordering::Relaxed),
            db_size_bytes: db_size,
        })
    }

    async fn get_sequencer_rankings(&self) -> RpcResult<Vec<flashstat_common::SequencerStats>> {
        let mut stats = self
            .storage
            .get_all_sequencer_stats()
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))?;

        // Sort by score descending
        stats.sort_by_key(|s| std::cmp::Reverse(s.reputation_score));
        Ok(stats)
    }

    async fn get_recent_reorgs(&self, limit: usize) -> RpcResult<Vec<ReorgEvent>> {
        self.storage
            .get_latest_reorgs(limit)
            .await
            .map(|events| {
                events
                    .into_iter()
                    .filter(|e| e.severity == flashstat_common::ReorgSeverity::Equivocation)
                    .collect()
            })
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn subscribe_blocks(
        &self,
        pending: jsonrpsee::PendingSubscriptionSink,
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let mut rx = self.block_tx.subscribe();

        tokio::spawn(async move {
            loop {
                let block = match rx.recv().await {
                    Ok(b) => b,
                    Err(_) => break,
                };
                let msg = match serde_json::to_string(&block) {
                    Ok(m) => m,
                    Err(_) => break,
                };
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        Ok(())
    }

    async fn subscribe_events(
        &self,
        pending: jsonrpsee::PendingSubscriptionSink,
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let mut rx = self.event_tx.subscribe();

        tokio::spawn(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(e) => e,
                    Err(_) => break,
                };
                let msg = match serde_json::to_string(&event) {
                    Ok(m) => m,
                    Err(_) => break,
                };
                if sink.send(msg).await.is_err() {
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

    // 2. Initialize Storage
    let storage = std::sync::Arc::new(flashstat_db::RedbStorage::new(&config.storage.db_path)?);

    // 3. Initialize Monitor
    let mut monitor =
        flashstat_core::FlashMonitor::new(config.clone(), storage.clone(), shutdown_tx.subscribe())
            .await?;
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
    let initial_reorgs = storage
        .get_latest_reorgs(1000)
        .await
        .unwrap_or_default()
        .len() as u64;

    let server_struct = FlashServer {
        storage: storage.clone(),
        block_tx,
        event_tx,
        total_reorgs: AtomicU64::new(initial_reorgs),
    };

    let handle = server.start(server_struct.into_rpc());
    info!("🚀 FlashStat JSON-RPC Server running on 127.0.0.1:9944");

    handle.stopped().await;
    Ok(())
}
