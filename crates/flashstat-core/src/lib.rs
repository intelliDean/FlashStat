use flashstat_common::{FlashBlock, BlockStatus, ReorgEvent, ReorgSeverity, Config};
pub mod tee;
use tee::TeeVerifier;
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
    tee_verifier: TeeVerifier,
}

impl FlashMonitor {
    pub async fn new(config: Config, shutdown_rx: broadcast::Receiver<()>) -> Result<Self> {
        let storage = Arc::new(RocksStorage::new(&config.storage.db_path)?);
        
        let sequencer_address: Address = config.tee.sequencer_address.parse()
            .context("Invalid sequencer address in config")?;
        let tee_verifier = TeeVerifier::new(sequencer_address);

        Ok(Self {
            config,
            storage,
            last_block: Arc::new(Mutex::new(None)),
            shutdown_rx,
            tee_verifier,
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
        let mut sequencer_signature = None;
        let mut signer = None;
        let mut tee_valid = false;

        // 1. Recover TEE signature and signer identity
        if let Some(sig_bytes) = extract_signature_from_block(&eth_block) {
            match self.tee_verifier.recover_signer(hash, &sig_bytes) {
                Ok(recovered_signer) => {
                    signer = Some(recovered_signer);
                    tee_valid = recovered_signer == self.tee_verifier.expected_sequencer;
                    sequencer_signature = Some(sig_bytes);
                    
                    if tee_valid {
                        info!("🛡️ TEE Signature Verified for block #{} by expected sequencer", number);
                    } else {
                        warn!("⚠️ TEE Signature valid but from unexpected signer: {:?}", recovered_signer);
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to recover signer from TEE signature for block #{}: {:?}", number, e);
                }
            }
        }

        // 2. Detection of Reorgs and Equivocations
        if let Some(ref prev) = *last_block_guard {
            if prev.number == number {
                if prev.hash != hash {
                    let mut severity = ReorgSeverity::Soft;
                    let mut equivocation = None;

                    // Detect Equivocation: Same block number, different hash, same signer
                    if let (Some(sig1), Some(sig2), Some(signer1), Some(signer2)) = (
                        &prev.sequencer_signature,
                        &sequencer_signature,
                        &prev.signer,
                        &signer,
                    ) {
                        if signer1 == signer2 {
                            severity = ReorgSeverity::Equivocation;
                            equivocation = Some(EquivocationEvent {
                                signer: *signer1,
                                signature_1: sig1.clone(),
                                signature_2: sig2.clone(),
                            });
                            warn!("🚨 EQUIVOCATION DETECTED at block #{} by signer {:?}!", number, signer1);
                        }
                    }

                    if severity == ReorgSeverity::Soft {
                        warn!("🚨 Soft Reorg detected at block #{}!", number);
                    }

                    let event = ReorgEvent {
                        block_number: number,
                        old_hash: prev.hash,
                        new_hash: hash,
                        detected_at: Utc::now(),
                        severity,
                        equivocation,
                    };
                    self.storage.save_reorg(event).await?;
                } else {
                    // Approximate persistence from previous confidence
                    persistence = ((prev.confidence / 100.0).log(0.5).abs().ceil() as u32).max(1) + 1;
                }
            }
        }

        // 3. Confidence Scoring
        let base_confidence = (1.0 - 0.5f64.powi(persistence as i32)) * 100.0;
        let confidence = if tee_valid {
            // TEE verification significantly accelerates "Soft Finality"
            (base_confidence + 99.0) / 2.0 
        } else {
            base_confidence
        };
        
        let flash_block = FlashBlock {
            number,
            hash,
            parent_hash: eth_block.parent_hash,
            timestamp: Utc::now(),
            sequencer_signature,
            signer,
            confidence,
            status: if confidence > 95.0 { BlockStatus::Stable } else { BlockStatus::Pending },
        };

        info!("🏮 Block #{} | Confidence: {:.2}% | Hash: {}", number, confidence, hash);
        
        self.storage.save_block(flash_block.clone()).await?;
        *last_block_guard = Some(flash_block);
        
        Ok(())
    }
}

/// Helper to simulate extracting the TEE signature from a block.
/// In Unichain, this is typically found in the extra_data or a custom header.
fn extract_signature_from_block(_block: &Block<H256>) -> Option<Bytes> {
    // TODO: Implement actual extraction logic for Unichain
    None
}
