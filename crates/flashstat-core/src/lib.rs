use flashstat_common::{
    BlockStatus, Config, ConflictAnalysis, DoubleSpendProof, EquivocationEvent, FlashBlock,
    ReorgEvent, ReorgSeverity,
};
pub mod tee;
use chrono::Utc;
use ethers::prelude::*;
use eyre::Result;
use flashstat_db::{FlashStorage, RedbStorage};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tee::TeeVerifier;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info, warn};

pub struct FlashMonitor {
    config: Config,
    storage: Arc<dyn FlashStorage>,
    last_block: Arc<Mutex<Option<FlashBlock>>>,
    shutdown_rx: broadcast::Receiver<()>,
    tee_verifier: TeeVerifier,
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
    provider: Arc<Provider<Http>>,
}

impl FlashMonitor {
    pub async fn new(config: Config, shutdown_rx: broadcast::Receiver<()>) -> Result<Self> {
        let storage = Arc::new(RedbStorage::new(&config.storage.db_path)?);

        let sequencer_address: Address = config.tee.sequencer_address;
        let tee_verifier = TeeVerifier::new(sequencer_address);

        let (block_tx, _) = broadcast::channel(100);
        let (event_tx, _) = broadcast::channel(100);

        let provider = Arc::new(Provider::<Http>::try_from(&config.rpc.http_url)?);

        Ok(Self {
            config,
            storage,
            last_block: Arc::new(Mutex::new(None)),
            shutdown_rx,
            tee_verifier,
            block_tx,
            event_tx,
            provider,
        })
    }

    pub fn block_notifier(&self) -> broadcast::Sender<FlashBlock> {
        self.block_tx.clone()
    }

    pub fn event_notifier(&self) -> broadcast::Sender<ReorgEvent> {
        self.event_tx.clone()
    }

    pub fn storage(&self) -> Arc<dyn FlashStorage> {
        self.storage.clone()
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("🏮 FlashStat Monitor starting with Supervisor pattern");

        let mut shutdown_rx = self.shutdown_rx.resubscribe();

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
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
        info!(
            "🔗 Attempting Unichain WebSocket connection: {}",
            self.config.rpc.ws_url
        );

        let ws_res = Provider::<Ws>::connect(&self.config.rpc.ws_url).await;

        match ws_res {
            Ok(ws_provider) => {
                info!("✅ WebSocket subscription active");
                let mut stream = ws_provider.subscribe_blocks().await?;
                while let Some(block) = stream.next().await {
                    if let Err(e) = self.handle_new_block(block).await {
                        error!("Error processing block: {:?}", e);
                    }
                }
                warn!("⚠️ WebSocket stream disconnected unexpectedly");
            }
            Err(e) => {
                warn!(
                    "❌ WebSocket connection failed: {:?}. Falling back to HTTP polling...",
                    e
                );
                let mut last_polled_block = 0u64;

                loop {
                    match self.provider.get_block_number().await {
                        Ok(num) => {
                            let num_u64 = num.as_u64();
                            if num_u64 > last_polled_block {
                                if let Ok(Some(block)) = self.provider.get_block(num).await {
                                    if let Err(e) = self.handle_new_block(block).await {
                                        error!("Error processing polled block: {:?}", e);
                                    }
                                    last_polled_block = num_u64;
                                }
                            }
                        }
                        Err(e) => error!("HTTP polling error: {:?}", e),
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }

        Ok(())
    }

    async fn handle_new_block(&self, eth_block: Block<H256>) -> Result<()> {
        let hash = eth_block.hash.unwrap_or_default();
        let number: U256 = eth_block.number.unwrap_or_default().as_u64().into();

        let mut last_block_guard = self.last_block.lock().await;

        let mut persistence = 1;
        let mut sequencer_signature = None;
        let mut signer = None;
        let mut tee_valid = false;
        let mut confidence = 0.0;

        // 1. Recover TEE signature and signer identity
        if let Some(sig_bytes) = extract_signature_from_block(&eth_block) {
            match self.tee_verifier.recover_signer(hash, &sig_bytes) {
                Ok(recovered_signer) => {
                    signer = Some(recovered_signer);
                    tee_valid = recovered_signer == self.tee_verifier.expected_sequencer;
                    sequencer_signature = Some(sig_bytes);

                    if tee_valid {
                        confidence = 90.0;
                        info!(
                            "🛡️ TEE Signature Verified for block #{} by expected sequencer",
                            number
                        );

                        // Phase 5: Optional TDX Attestation Check
                        if self.config.tee.attestation_enabled {
                            let quote = extract_quote_from_block(&eth_block);
                            if let Ok(valid) = self.tee_verifier.verify_tdx_attestation(
                                &quote.unwrap_or_default(),
                                self.config.tee.expected_mrenclave.as_deref(),
                            ) {
                                if valid {
                                    confidence = 99.0;
                                    info!("🛡️ TDX Attestation Verified for block #{}", number);
                                } else {
                                    confidence = 45.0;
                                    warn!("⚠️ TEE Signature valid but Attestation FAILED for block #{}", number);
                                }
                            }
                        }
                    } else {
                        warn!(
                            "⚠️ TEE Signature valid but from unexpected signer: {:?}",
                            recovered_signer
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "⚠️ Failed to recover signer from TEE signature for block #{}: {:?}",
                        number, e
                    );
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
                                conflict_analysis: None,
                            });
                            warn!(
                                "🚨 EQUIVOCATION DETECTED at block #{} by signer {:?}!",
                                number, signer1
                            );
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

                    if severity == ReorgSeverity::Equivocation {
                        let storage = self.storage.clone();
                        let provider = self.provider.clone();
                        let event_clone = event.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                analyze_and_update_equivocation(storage, provider, event_clone)
                                    .await
                            {
                                warn!("Failed to analyze conflicts: {:?}", e);
                            }
                        });
                    }

                    self.storage.save_reorg(event.clone()).await?;
                    let _ = self.event_tx.send(event);
                }
            } else if prev.number < number {
                // Approximate persistence from previous confidence
                persistence = ((prev.confidence / 100.0).log(0.5).abs().ceil() as u32).max(1) + 1;
            }
        }

        let base_confidence = (1.0 - 0.5f64.powi(persistence as i32)) * 100.0;
        let final_confidence = if tee_valid {
            (base_confidence + 99.0) / 2.0
        } else {
            base_confidence
        };

        // Use the TEE-specific override if available, otherwise use final_confidence
        let confidence = if confidence > 0.0 {
            confidence
        } else {
            final_confidence
        };

        let flash_block = FlashBlock {
            number,
            hash,
            parent_hash: eth_block.parent_hash,
            timestamp: Utc::now(),
            sequencer_signature,
            signer,
            confidence,
            status: if confidence > 95.0 {
                BlockStatus::Stable
            } else {
                BlockStatus::Pending
            },
        };

        info!(
            "🏮 Block #{} | Confidence: {:.2}% | Hash: {}",
            number, confidence, hash
        );

        self.storage.save_block(flash_block.clone()).await?;
        let _ = self.block_tx.send(flash_block.clone());
        *last_block_guard = Some(flash_block);

        Ok(())
    }
}

/// Helper to simulate extracting the TEE signature from a block.
/// In Unichain, this is typically found in the extra_data or a custom header.
fn extract_signature_from_block(block: &Block<H256>) -> Option<Bytes> {
    let extra_data = &block.extra_data;
    if extra_data.len() >= 65 {
        // Unichain/OP-Stack sequencer signatures are typically the last 65 bytes of extra_data
        let sig = &extra_data[extra_data.len() - 65..];
        Some(Bytes::from(sig.to_vec()))
    } else {
        None
    }
}

/// Helper to extract the TEE attestation quote from a block.
fn extract_quote_from_block(_block: &Block<H256>) -> Option<Bytes> {
    // TODO: Implement actual extraction logic for Unichain (e.g. from RLP-encoded extra data)
    None
}

async fn analyze_and_update_equivocation(
    storage: Arc<dyn FlashStorage>,
    provider: Arc<Provider<Http>>,
    mut event: ReorgEvent,
) -> Result<()> {
    use ethers::prelude::*;
    use std::collections::HashMap;

    let Some(mut equivocation) = event.equivocation.take() else {
        return Ok(());
    };

    // Fetch full blocks with transactions
    let (Some(old_block), Some(new_block)) = futures_util::try_join!(
        provider.get_block_with_txs(event.old_hash),
        provider.get_block_with_txs(event.new_hash)
    )?
    else {
        return Ok(());
    };

    let mut dropped_txs = Vec::new();
    let mut double_spend_txs = Vec::new();

    // Map new block transactions by sender:nonce for quick lookup
    let mut new_tx_map = HashMap::new();
    for tx in &new_block.transactions {
        new_tx_map.insert((tx.from, tx.nonce), tx.hash);
    }

    for old_tx in &old_block.transactions {
        if !new_block
            .transactions
            .iter()
            .any(|tx| tx.hash == old_tx.hash)
        {
            // Transaction was dropped
            dropped_txs.push(old_tx.hash);

            // Check if it was replaced by another transaction with same nonce (Double-Spend)
            if let Some(&new_hash) = new_tx_map.get(&(old_tx.from, old_tx.nonce)) {
                double_spend_txs.push(DoubleSpendProof {
                    tx_hash_1: old_tx.hash,
                    tx_hash_2: new_hash,
                    sender: old_tx.from,
                    nonce: old_tx.nonce,
                });
            }
        }
    }

    equivocation.conflict_analysis = Some(ConflictAnalysis {
        dropped_txs,
        double_spend_txs,
    });
    event.equivocation = Some(equivocation);

    // Update the reorg event in storage
    storage.save_reorg(event).await?;

    Ok(())
}
