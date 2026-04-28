use flashstat_common::{FlashBlock, BlockStatus, ReorgEvent, ReorgSeverity, Config};
use flashstat_db::{FlashStorage, RocksStorage};
use ethers::prelude::*;
use eyre::{Result, Context};
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};
use chrono::Utc;
use tracing::{info, warn, error};
use futures_util::StreamExt;
use std::time::Duration;

pub struct FlashMonitor {
    config: Config,
    storage: Arc<dyn FlashStorage>,
    last_block: Arc<Mutex<Option<FlashBlock>>>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl FlashMonitor {
    pub async fn new(config: Config, shutdown_rx: broadcast::Receiver<()>) -> Result<Self> {
        let storage = Arc::new(RocksStorage::new(&config.storage.db_path)?);
        
        Ok(Self {
            config,
            storage,
            last_block: Arc::new(Mutex::new(None)),
            shutdown_rx,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("🏮 FlashStat Monitor starting with Supervisor pattern");
        
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    info!("🛑 Monitor received shutdown signal");
                    break;
                }
                res = self.supervise_connection() => {
                    if let Err(e) = res {
                        error!("Supervisor error: {:?}. Retrying in 5s...", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
        
        info!("🏮 Monitor shutdown complete");
        Ok(())
    }

    async fn supervise_connection(&self) -> Result<()> {
        info!("🔗 Connecting to Unichain WebSocket: {}", self.config.rpc.ws_url);
        
        let provider = Provider::<Ws>::connect(&self.config.rpc.ws_url).await
            .context("Failed to connect to Unichain WebSocket")?;
        
        let mut stream = provider.subscribe_blocks().await?;
        info!("✅ WebSocket subscription active");

        while let Some(block) = stream.next().await {
            if let Err(e) = self.handle_new_block(block).await {
                error!("Error processing block: {:?}", e);
            }
        }

        warn!("⚠️ WebSocket stream disconnected unexpectedly");
        Ok(())
    }

    async fn handle_new_block(&self, eth_block: Block<H256>) -> Result<()> {
        let hash = eth_block.hash.unwrap_or_default();
        let number = eth_block.number.unwrap_or_default();
        
        let mut last_block_guard = self.last_block.lock().await;
        
        let mut persistence = 1;
        if let Some(ref prev) = *last_block_guard {
            if prev.number == number {
                if prev.hash != hash {
                    warn!("🚨 Soft Reorg detected at block #{}!", number);
                    let event = ReorgEvent {
                        block_number: number,
                        old_hash: prev.hash,
                        new_hash: hash,
                        detected_at: Utc::now(),
                        severity: ReorgSeverity::Soft,
                    };
                    self.storage.save_reorg(event).await?;
                } else {
                    // Approximate persistence from previous confidence
                    persistence = ((prev.confidence / 100.0).log(0.5).abs().ceil() as u32).max(1) + 1;
                }
            }
        }

        let confidence = (1.0 - 0.5f64.powi(persistence as i32)) * 100.0;
        
        let flash_block = FlashBlock {
            number,
            hash,
            parent_hash: eth_block.parent_hash,
            timestamp: Utc::now(),
            sequencer_signature: None,
            confidence,
            status: if confidence > 95.0 { BlockStatus::Stable } else { BlockStatus::Pending },
        };

        info!("🏮 Block #{} | Confidence: {:.2}% | Hash: {}", number, confidence, hash);
        
        self.storage.save_block(flash_block.clone()).await?;
        *last_block_guard = Some(flash_block);
        
        Ok(())
    }
}
