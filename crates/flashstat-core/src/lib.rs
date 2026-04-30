use flashstat_common::{
    BlockStatus, Config, ConflictAnalysis, DoubleSpendProof, EquivocationEvent, FlashBlock,
    ReorgEvent, ReorgSeverity,
};
pub mod proof;
pub mod tee;
pub mod wallet;
use chrono::Utc;
use ethers::prelude::*;
use eyre::Result;
use flashstat_db::FlashStorage;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tee::TeeVerifier;
use tokio::sync::{Mutex, broadcast};
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
    guardian_wallet: Option<Arc<wallet::GuardianWallet>>,
}

impl FlashMonitor {
    pub async fn new(
        config: Config,
        storage: Arc<dyn FlashStorage>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<Self> {
        let sequencer_address: Address = config.tee.sequencer_address;
        let tee_verifier = TeeVerifier::new(sequencer_address);

        let (block_tx, _) = broadcast::channel(100);
        let (event_tx, _) = broadcast::channel(100);

        let provider = Arc::new(Provider::<Http>::try_from(&config.rpc.http_url)?);

        let guardian_wallet =
            if config.guardian.private_key.is_some() || config.guardian.keystore_path.is_some() {
                Some(Arc::new(
                    wallet::GuardianWallet::new(&config.guardian, &config.rpc.http_url).await?,
                ))
            } else {
                None
            };

        Ok(Self {
            config,
            storage,
            last_block: Arc::new(Mutex::new(None)),
            shutdown_rx,
            tee_verifier,
            block_tx,
            event_tx,
            provider,
            guardian_wallet,
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
                                #[allow(clippy::collapsible_if)]
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

                        if self.config.tee.attestation_enabled {
                            if let Some(quote) = extract_quote_from_block(&eth_block) {
                                match self.tee_verifier.verify_tdx_attestation(
                                    &quote,
                                    self.config.tee.expected_mrenclave.as_deref(),
                                ) {
                                    Ok(true) => {
                                        confidence = 99.0;
                                        info!("🛡️ TDX Attestation Verified for block #{}", number);
                                    }
                                    Ok(false) => {
                                        confidence = 45.0;
                                        warn!(
                                            "⚠️ TEE Signature valid but Attestation Check FAILED for block #{}",
                                            number
                                        );
                                    }
                                    Err(e) => {
                                        confidence = 70.0;
                                        warn!(
                                            "⚠️ TEE Signature valid but Attestation verification ERROR for block #{}: {:?}",
                                            number, e
                                        );
                                    }
                                }
                            } else {
                                confidence = 85.0;
                                warn!(
                                    "⚠️ Attestation enabled but NO quote found in block #{}",
                                    number
                                );
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
                    let equivocation_check = (
                        &prev.sequencer_signature,
                        &sequencer_signature,
                        &prev.signer,
                        &signer,
                    );
                    if let (Some(sig1), Some(sig2), Some(signer1), Some(signer2)) =
                        equivocation_check
                    {
                        #[allow(clippy::collapsible_if)]
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
                        let guardian = self.guardian_wallet.clone();
                        let sig1 = prev.sequencer_signature.clone().unwrap_or_default();
                        let sig2 = sequencer_signature.clone().unwrap_or_default();
                        let signer_addr = signer.unwrap_or_default();
                        let old_hash = prev.hash;
                        let new_hash = hash;
                        let block_number = number;

                        tokio::spawn(async move {
                            if let Err(e) =
                                analyze_and_update_equivocation(storage, provider, event_clone)
                                    .await
                            {
                                warn!("Failed to analyze conflicts: {:?}", e);
                            }

                            // Active Fraud Proof Submission
                            if let Some(wallet) = guardian {
                                info!("🗼 Watchtower: Generating on-chain equivocation proof...");
                                let proof = proof::encode_equivocation_proof(
                                    block_number,
                                    signer_addr,
                                    sig1,
                                    sig2,
                                    old_hash,
                                    new_hash,
                                );
                                match wallet.submit_equivocation_proof(proof).await {
                                    Ok(tx_hash) => info!(
                                        "🚀 ACTIVE PROTECTION: Slashing proof submitted! TX: {:?}",
                                        tx_hash
                                    ),
                                    Err(e) => {
                                        error!("❌ Watchtower FAILED to submit proof: {:?}", e)
                                    }
                                }
                            }
                        });
                    }

                    self.storage.save_reorg(event.clone()).await?;
                    let _ = self.event_tx.send(event.clone());

                    // Update Reputation Penalties
                    if let Some(signer_addr) = signer {
                        let (soft, equiv) = if severity == ReorgSeverity::Equivocation {
                            (0, 1)
                        } else {
                            (1, 0)
                        };
                        if let Err(e) = self
                            .update_reputation(signer_addr, 0, soft, equiv, false)
                            .await
                        {
                            error!("Failed to apply reputation penalty: {:?}", e);
                        }
                    }
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
            sequencer_signature: sequencer_signature.clone(),
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

        // Update Reputation
        if let Some(signer_addr) = signer {
            let attested = confidence > 95.0; // Phase 5 threshold
            if let Err(e) = self.update_reputation(signer_addr, 1, 0, 0, attested).await {
                error!("Failed to update reputation: {:?}", e);
            }
        }

        *last_block_guard = Some(flash_block);

        Ok(())
    }

    async fn update_reputation(
        &self,
        address: Address,
        blocks: u64,
        soft_reorgs: u64,
        equivocations: u64,
        attested: bool,
    ) -> Result<()> {
        let mut stats = self.storage.get_sequencer_stats(address).await?.unwrap_or(
            flashstat_common::SequencerStats {
                address,
                last_active: Utc::now(),
                ..Default::default()
            },
        );

        if blocks > 0 {
            stats.total_blocks_signed += blocks;
            stats.current_streak += blocks;
            if attested {
                stats.total_attested_blocks += blocks;
            }
        }

        if soft_reorgs > 0 || equivocations > 0 {
            stats.total_soft_reorgs += soft_reorgs;
            stats.total_equivocations += equivocations;
            stats.current_streak = 0; // Reset streak on any issue
        }

        stats.last_active = Utc::now();

        // Calculate score with Refined Weights
        let base_score = stats.total_blocks_signed as i64;
        let attestation_bonus = stats.total_attested_blocks as i64; // Permanent +1 for each hardware-backed block
        let streak_bonus = (stats.current_streak / 100) as i64 * 10;

        let penalty =
            (stats.total_soft_reorgs as i64 * 50) + (stats.total_equivocations as i64 * 1000);

        stats.reputation_score = base_score + attestation_bonus + streak_bonus - penalty;

        self.storage.update_sequencer_stats(stats).await?;
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
/// In Unichain, the quote may be present in extra_data or a custom header.
fn extract_quote_from_block(block: &Block<H256>) -> Option<Bytes> {
    let extra_data = &block.extra_data;

    // OP-Stack extra_data structure: [32-byte zero prefix] [65-byte signature] [optional quote]
    // If the data is longer than 32 + 65, the remainder might be the quote.
    if extra_data.len() > 97 {
        let quote = &extra_data[97..];
        Some(Bytes::from(quote.to_vec()))
    } else {
        // Fallback: check if the extra_data itself is an RLP list containing the quote
        let rlp = ethers::utils::rlp::Rlp::new(extra_data);
        #[allow(clippy::collapsible_if)]
        if rlp.is_list() && rlp.item_count().unwrap_or(0) >= 2 {
            if let Some(quote_bytes) = rlp
                .at(1)
                .ok()
                .and_then(|item| item.as_val::<Vec<u8>>().ok())
                .filter(|b| b.len() > 128)
            {
                return Some(Bytes::from(quote_bytes));
            }
        }
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use flashstat_common::*;
    use flashstat_db::RedbStorage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_reputation_scoring() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("test.db");
        let storage = Arc::new(RedbStorage::new(db_path.to_str().unwrap())?);

        // Mock config
        let config = Config {
            rpc: RpcConfig {
                ws_url: "http://localhost:8545".into(),
                http_url: "http://localhost:8545".into(),
            },
            storage: StorageConfig {
                db_path: db_path.to_str().unwrap().into(),
            },
            tee: TeeConfig {
                sequencer_address: Address::random(),
                attestation_enabled: false,
                expected_mrenclave: None,
            },
            guardian: GuardianConfig {
                private_key: None,
                keystore_path: None,
                slashing_contract: Address::random(),
            },
        };

        let (_tx, rx) = broadcast::channel(1);
        let monitor = FlashMonitor::new(config, storage.clone(), rx).await?;

        let address = Address::random();

        // 1. Reward: 100 blocks + attested
        monitor.update_reputation(address, 100, 0, 0, true).await?;
        let stats = storage.get_sequencer_stats(address).await?.unwrap();
        // Base(100) + Attestation(100) + Streak(10) = 210
        assert_eq!(stats.reputation_score, 210);
        assert_eq!(stats.current_streak, 100);

        // 2. Penalty: Equivocation
        monitor.update_reputation(address, 0, 0, 1, false).await?;
        let stats = storage.get_sequencer_stats(address).await?.unwrap();
        // Base(100) + Attest(100) + Streak(0) - 1000 = -800
        assert_eq!(stats.reputation_score, -800);
        assert_eq!(stats.current_streak, 0);

        Ok(())
    }

    #[test]
    fn test_proof_serialization() {
        use crate::proof;
        let ds_proof = DoubleSpendProof {
            tx_hash_1: H256::random(),
            tx_hash_2: H256::random(),
            sender: Address::random(),
            nonce: U256::from(42),
        };

        let rlp_bytes = proof::encode_double_spend_proof(ds_proof);
        assert!(!rlp_bytes.is_empty());
    }
}
