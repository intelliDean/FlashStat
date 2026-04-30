use ethers::types::H256;
use eyre::Context;
use flashstat_api::FlashApiServer;
use flashstat_common::{Config, FlashBlock, ReorgEvent, ReorgSeverity, SequencerStats, SystemHealth};
use flashstat_db::FlashStorage;
use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::types::error::ErrorObjectOwned;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Clone)]
pub struct FlashServer {
    storage: Arc<dyn FlashStorage>,
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
    start_time: Instant,
    total_blocks: Arc<AtomicU64>,
    total_reorgs: Arc<AtomicU64>,
    db_path: String,
}

#[async_trait]
impl FlashApiServer for FlashServer {
    async fn get_confidence(&self, hash: H256) -> RpcResult<f64> {
        let block = self
            .storage
            .get_block(hash)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))?;
        Ok(block.map(|b| b.confidence).unwrap_or(0.0))
    }

    async fn get_latest_block(&self) -> RpcResult<Option<FlashBlock>> {
        self.storage
            .get_latest_block()
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_recent_blocks(&self, limit: usize) -> RpcResult<Vec<FlashBlock>> {
        self.storage
            .get_recent_blocks(limit)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_recent_reorgs(&self, limit: usize) -> RpcResult<Vec<ReorgEvent>> {
        self.storage
            .get_latest_reorgs(limit)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_equivocations(&self, limit: usize) -> RpcResult<Vec<ReorgEvent>> {
        self.storage
            .get_latest_reorgs(limit)
            .await
            .map(|events| {
                events
                    .into_iter()
                    .filter(|e| e.severity == ReorgSeverity::Equivocation)
                    .collect()
            })
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
    }

    async fn get_health(&self) -> RpcResult<SystemHealth> {
        let db_size = std::fs::metadata(&self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(SystemHealth {
            uptime_secs: self.start_time.elapsed().as_secs(),
            total_blocks: self.total_blocks.load(Ordering::Relaxed),
            total_reorgs: self.total_reorgs.load(Ordering::Relaxed),
            db_size_bytes: db_size,
        })
    }

    async fn get_sequencer_rankings(&self) -> RpcResult<Vec<SequencerStats>> {
        let mut stats = self
            .storage
            .get_all_sequencer_stats()
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))?;

        // Sort by score descending
        stats.sort_by_key(|s| std::cmp::Reverse(s.reputation_score));
        Ok(stats)
    }

    async fn subscribe_blocks(
        &self,
        pending: jsonrpsee::PendingSubscriptionSink,
    ) -> jsonrpsee::core::SubscriptionResult {
        let mut rx = self.block_tx.subscribe();
        let sink = pending.accept().await?;

        tokio::spawn(async move {
            while let Ok(block) = rx.recv().await {
                let msg = match jsonrpsee::SubscriptionMessage::from_json(&block) {
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
        let mut rx = self.event_tx.subscribe();
        let sink = pending.accept().await?;

        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                let msg = match jsonrpsee::SubscriptionMessage::from_json(&event) {
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
        block_tx: block_tx.clone(),
        event_tx: event_tx.clone(),
        start_time: Instant::now(),
        total_blocks: Arc::new(AtomicU64::new(0)),
        total_reorgs: Arc::new(AtomicU64::new(initial_reorgs)),
        db_path: config.storage.db_path.clone(),
    };

    // Stats listeners
    let mut stats_block_rx = block_tx.subscribe();
    let mut stats_event_rx = event_tx.subscribe();
    let server_stats_1 = server_struct.clone();
    let server_stats_2 = server_struct.clone();

    tokio::spawn(async move {
        while stats_block_rx.recv().await.is_ok() {
            server_stats_1.total_blocks.fetch_add(1, Ordering::Relaxed);
        }
    });

    tokio::spawn(async move {
        while stats_event_rx.recv().await.is_ok() {
            server_stats_2.total_reorgs.fetch_add(1, Ordering::Relaxed);
        }
    });

    let handle = server.start(server_struct.into_rpc());

    info!("🏮 FlashStat JSON-RPC Server (with Pub/Sub) started at 127.0.0.1:9944");

    handle.stopped().await;
    Ok(())
}
