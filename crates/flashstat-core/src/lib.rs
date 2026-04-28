use flashstat_common::{FlashBlock, BlockStatus, ReorgEvent, ReorgSeverity};
use flashstat_db::{FlashStorage, RocksStorage};
use ethers::prelude::*;
use eyre::{Result, Context};
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono::Utc;
use tracing::{info, warn, error};

pub struct FlashMonitor {
    provider: Arc<Provider<Ws>>,
    storage: Arc<dyn FlashStorage>,
    last_block: Arc<Mutex<Option<FlashBlock>>>,
}

impl FlashMonitor {
    pub async fn new(rpc_url: &str, storage_path: &str) -> Result<Self> {
        let provider = Provider::<Ws>::connect(rpc_url).await
            .context("Failed to connect to Unichain WebSocket")?;
        
        let storage = Arc::new(RocksStorage::new(storage_path)?);
        
        Ok(Self {
            provider: Arc::new(provider),
            storage,
            last_block: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("🏮 FlashStat Monitor started");
        
        let mut stream = self.provider.subscribe_blocks().await?;
        
        while let Some(block) = stream.next().await {
            if let Err(e) = self.handle_new_block(block).await {
                error!("Error processing block: {:?}", e);
            }
        }
        
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
                    // SOFT REORG DETECTED
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
                    persistence = (prev.confidence / 50.0).log2() as u32 + 1; // Simplified reverse engineering of confidence
                }
            }
        }

        // Confidence Formula: 1 - (0.5 ^ persistence)
        let confidence = (1.0 - 0.5f64.powi(persistence as i32)) * 100.0;
        
        let flash_block = FlashBlock {
            number,
            hash,
            parent_hash: eth_block.parent_hash,
            timestamp: Utc::now(),
            sequencer_signature: None, // TODO: Extract from extra_data if available
            confidence,
            status: if confidence > 95.0 { BlockStatus::Stable } else { BlockStatus::Pending },
        };

        info!("🏮 Block #{} | Confidence: {:.2}% | Hash: {}", number, confidence, hash);
        
        self.storage.save_block(flash_block.clone()).await?;
        *last_block_guard = Some(flash_block);
        
        Ok(())
    }
}
