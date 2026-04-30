use ethers::types::H256;
use eyre::Context;
use flashstat_api::FlashApiServer;
use flashstat_common::{
    Config, FlashBlock, ReorgEvent, ReorgSeverity, SequencerStats, SystemHealth,
};
use flashstat_db::FlashStorage;
use jsonrpsee::core::{RpcResult, async_trait};
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
    monitor: Arc<flashstat_core::FlashMonitor>,
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

        Ok(flashstat_common::SystemHealth {
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

    async fn ingest_block(
        &self,
        block: ethers::types::Block<ethers::types::H256>,
    ) -> RpcResult<()> {
        self.monitor
            .handle_new_block(block)
            .await
            .map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))
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

fn init_logging() {
    tracing_subscriber::fmt::init();
}

fn load_configuration() -> eyre::Result<Config> {
    Config::load().context("Failed to load config")
}

fn start_monitor(monitor: Arc<flashstat_core::FlashMonitor>) {
    tokio::spawn(async move {
        if let Err(e) = monitor.run().await {
            tracing::error!("Monitor error: {:?}", e);
        }
    });
}

fn start_stats_tracking(
    server_state: &FlashServer,
    mut block_rx: broadcast::Receiver<FlashBlock>,
    mut event_rx: broadcast::Receiver<ReorgEvent>,
) {
    let server_blocks = server_state.clone();
    let server_events = server_state.clone();

    tokio::spawn(async move {
        while block_rx.recv().await.is_ok() {
            server_blocks.total_blocks.fetch_add(1, Ordering::Relaxed);
        }
    });

    tokio::spawn(async move {
        while event_rx.recv().await.is_ok() {
            server_events.total_reorgs.fetch_add(1, Ordering::Relaxed);
        }
    });
}

async fn build_server_state(
    config: &Config,
    storage: Arc<flashstat_db::RedbStorage>,
    monitor: Arc<flashstat_core::FlashMonitor>,
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
) -> eyre::Result<FlashServer> {
    let initial_reorgs = storage
        .get_latest_reorgs(1000)
        .await
        .unwrap_or_default()
        .len() as u64;

    Ok(FlashServer {
        storage,
        block_tx,
        event_tx,
        start_time: Instant::now(),
        total_blocks: Arc::new(AtomicU64::new(0)),
        total_reorgs: Arc::new(AtomicU64::new(initial_reorgs)),
        db_path: config.storage.db_path.clone(),
        monitor,
    })
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    init_logging();
    let config = load_configuration()?;

    let (shutdown_tx, _) = broadcast::channel(1);
    let storage = Arc::new(flashstat_db::RedbStorage::new(&config.storage.db_path)?);

    let monitor = Arc::new(
        flashstat_core::FlashMonitor::new(config.clone(), storage.clone(), shutdown_tx.subscribe())
            .await?,
    );

    let block_tx = monitor.block_notifier();
    let event_tx = monitor.event_notifier();

    start_monitor(monitor.clone());

    let server_state = build_server_state(
        &config,
        storage,
        monitor,
        block_tx.clone(),
        event_tx.clone(),
    )
    .await?;

    start_stats_tracking(&server_state, block_tx.subscribe(), event_tx.subscribe());

    let server = ServerBuilder::default().build("127.0.0.1:9944").await?;
    let handle = server.start(server_state.into_rpc());

    info!("🏮 FlashStat JSON-RPC Server (with Pub/Sub) started at 127.0.0.1:9944");

    handle.stopped().await;
    Ok(())
}
