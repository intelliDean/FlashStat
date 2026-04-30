use ethers::prelude::*;
use eyre::{Context, Result, eyre};
use flashstat_common::*;
use flashstat_db::FlashStorage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

pub mod proof;
pub mod tee;
pub mod wallet;

use crate::tee::TeeVerifier;
use crate::wallet::GuardianWallet;

pub struct FlashMonitor {
    config: Config,
    storage: Arc<dyn FlashStorage>,
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
    shutdown_rx: broadcast::Receiver<()>,
    tee_verifier: TeeVerifier,
    guardian_wallet: Option<GuardianWallet>,
}

impl FlashMonitor {
    pub async fn new(
        config: Config,
        storage: Arc<dyn FlashStorage>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<Self> {
        let (block_tx, _) = broadcast::channel(100);
        let (event_tx, _) = broadcast::channel(100);

        let tee_verifier = TeeVerifier::new(config.tee.sequencer_address);

        let guardian_wallet =
            match GuardianWallet::new(&config.guardian, &config.rpc.http_url).await {
                Ok(w) => Some(w),
                Err(e) => {
                    warn!("Guardian wallet not initialized: {}. Watchtower will operate in monitor-only mode.", e);
                    None
                }
            };

        Ok(Self {
            config,
            storage,
            block_tx,
            event_tx,
            shutdown_rx,
            tee_verifier,
            guardian_wallet,
        })
    }

    pub fn block_notifier(&self) -> broadcast::Sender<FlashBlock> {
        self.block_tx.clone()
    }

    pub fn event_notifier(&self) -> broadcast::Sender<ReorgEvent> {
        self.event_tx.clone()
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("🏮 FlashStat Monitor started");

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.check_new_blocks().await {
                        error!("Error checking blocks: {:?}", e);
                    }
                }
                _ = self.shutdown_rx.recv() => {
                    info!("Monitor received shutdown signal");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn check_new_blocks(&mut self) -> Result<()> {
        // In a real implementation, this would subscribe to Ethereum WS
        // For simulation, we poll the RPC
        Ok(())
    }

    /// Primary entry point for block ingestion.
    /// Performs TEE verification and equivocation checks.
    pub async fn process_block(&self, block: Block<Transaction>) -> Result<()> {
        let mut flash_block = FlashBlock {
            number: block.number.unwrap_or_default(),
            hash: block.hash.unwrap_or_default(),
            parent_hash: block.parent_hash,
            timestamp: chrono::Utc::now(),
            sequencer_signature: None,
            signer: None,
            confidence: 0.0,
            status: BlockStatus::Pending,
        };

        // 1. TEE Signature Verification
        let mut attestation_valid = false;
        if let Some(sig_bytes) = block.other.get("sequencer_signature") {
            let sig_hex = sig_bytes.as_str().unwrap_or_default();
            let sig = hex::decode(sig_hex.trim_start_matches("0x"))?;
            flash_block.sequencer_signature = Some(sig.clone().into());

            if let Ok(true) = self
                .tee_verifier
                .verify_sequencer_signature(flash_block.hash, &sig)
            {
                flash_block.signer = Some(self.config.tee.sequencer_address);
                flash_block.confidence += 0.5; // 50% confidence from hardware signature
            }

            // 2. TDX Quote Verification (if enabled)
            if self.config.tee.attestation_enabled {
                if let Some(quote_bytes) = block.other.get("tee_quote") {
                    let quote_hex = quote_bytes.as_str().unwrap_or_default();
                    let quote = hex::decode(quote_hex.trim_start_matches("0x"))?;

                    if let Ok(true) = self.tee_verifier.verify_tdx_attestation(
                        &quote,
                        self.config.tee.expected_mrenclave.as_deref(),
                    ) {
                        attestation_valid = true;
                        flash_block.confidence += 0.45; // Additional 45% for hardware attestation
                    }
                }
            }
        }

        // 3. Consistency/Stability Check
        if flash_block.confidence >= 0.9 {
            flash_block.status = BlockStatus::Stable;
        }

        // 4. Update Reputation
        self.update_reputation(
            self.config.tee.sequencer_address,
            1,
            0,
            0,
            attestation_valid,
        )
        .await?;

        // 5. Detect Equivocation (Compare with siblings at same height)
        if let Some(prev_block) = self.storage.get_block(flash_block.hash).await? {
            if prev_block.hash != flash_block.hash {
                // Different hash at same height! Equivocation detected.
                self.handle_equivocation(prev_block, flash_block.clone())
                    .await?;
            }
        }

        // 6. Persistence
        self.storage.save_block(flash_block.clone()).await?;
        let _ = self.block_tx.send(flash_block);

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
        let mut stats = self
            .storage
            .get_sequencer_stats(address)
            .await?
            .unwrap_or_default();

        stats.address = address;
        stats.total_blocks_signed += blocks;
        stats.total_soft_reorgs += soft_reorgs;
        stats.total_equivocations += equivocations;
        stats.last_active = chrono::Utc::now();

        if attested {
            stats.total_attested_blocks += blocks;
        }

        // Scoring Logic:
        // +1 per block
        // +1 bonus for attested blocks
        // +10 per 100 streak
        // -100 per soft reorg
        // -1000 per equivocation
        let mut score: i64 = (stats.total_blocks_signed as i64) * 1;
        score += (stats.total_attested_blocks as i64) * 1;
        score -= (stats.total_soft_reorgs as i64) * 100;
        score -= (stats.total_equivocations as i64) * 1000;

        if equivocations > 0 {
            stats.current_streak = 0;
        } else {
            stats.current_streak += blocks;
        }
        score += (stats.current_streak / 10) as i64; // Small bonus for reliability

        stats.reputation_score = score;
        self.storage.update_sequencer_stats(stats).await?;

        Ok(())
    }

    async fn handle_equivocation(&self, block1: FlashBlock, block2: FlashBlock) -> Result<()> {
        warn!(
            "🚨 EQUIVOCATION DETECTED at height #{}!",
            block1.number
        );
        error!("  Hash 1: {:?}", block1.hash);
        error!("  Hash 2: {:?}", block2.hash);

        let event = ReorgEvent {
            block_number: block1.number,
            old_hash: block1.hash,
            new_hash: block2.hash,
            detected_at: chrono::Utc::now(),
            severity: ReorgSeverity::Equivocation,
            equivocation: Some(EquivocationEvent {
                signer: block1.signer.unwrap_or_default(),
                signature_1: block1.sequencer_signature.unwrap_or_default(),
                signature_2: block2.sequencer_signature.unwrap_or_default(),
                conflict_analysis: None,
            }),
        };

        // 1. Save event
        self.storage.save_reorg(event.clone()).await?;

        // 2. Update reputation penalty
        if let Some(signer) = block1.signer {
            self.update_reputation(signer, 0, 0, 1, false).await?;
        }

        // 3. Autonomous Slashing Submission
        if let Some(wallet) = &self.guardian_wallet {
            info!("⚖️  Guardian Wallet: Generating and submitting fraud proof to L1...");

            let proof_bytes = proof::encode_equivocation_proof(
                block1.number,
                block1.signer.unwrap_or_default(),
                block1.sequencer_signature.unwrap_or_default(),
                block2.sequencer_signature.unwrap_or_default(),
                block1.hash,
                block2.hash,
            );

            match wallet.submit_equivocation_proof(proof_bytes).await {
                Ok(tx_hash) => info!("✅ Slashing proof submitted! L1 TX: {:?}", tx_hash),
                Err(e) => error!("❌ Failed to submit slashing proof: {:?}", e),
            }
        }

        let _ = self.event_tx.send(event);
        Ok(())
    }
}

pub async fn analyze_reorg_conflicts(
    storage: Arc<dyn FlashStorage>,
    event_ts: i64,
    old_txs: Vec<Transaction>,
    new_txs: Vec<Transaction>,
) -> Result<()> {
    // 1. Identify dropped transactions
    let new_tx_hashes: std::collections::HashSet<H256> =
        new_txs.iter().map(|tx| tx.hash).collect();
    let dropped_txs: Vec<H256> = old_txs
        .iter()
        .filter(|tx| !new_tx_hashes.contains(&tx.hash))
        .map(|tx| tx.hash)
        .collect();

    // 2. Identify double spends (same sender + nonce, different hash)
    let mut new_tx_map = HashMap::new();
    for tx in &new_txs {
        new_tx_map.insert((tx.from, tx.nonce), tx.hash);
    }

    let mut double_spend_txs = Vec::new();
    for old_tx in &old_txs {
        if !new_tx_hashes.contains(&old_tx.hash) {
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

    // 3. Find and update the event in storage
    let recent_events = storage.get_latest_reorgs(10).await?;
    if let Some(mut event) = recent_events
        .into_iter()
        .find(|e| e.detected_at.timestamp() == event_ts)
    {
        let mut equivocation = event.equivocation.clone().unwrap_or(EquivocationEvent {
            signer: Address::zero(),
            signature_1: Bytes::default(),
            signature_2: Bytes::default(),
            conflict_analysis: None,
        });

        equivocation.conflict_analysis = Some(ConflictAnalysis {
            dropped_txs,
            double_spend_txs,
        });
        event.equivocation = Some(equivocation);

        storage.save_reorg(event).await?;
    }

    Ok(())
}
