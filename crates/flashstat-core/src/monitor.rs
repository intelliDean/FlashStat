use crate::{proof, tee::TeeVerifier, wallet};
use chrono::Utc;
use ethers::prelude::*;
use eyre::Result;
use flashstat_common::{
    BlockStatus, Config, ConflictAnalysis, DoubleSpendProof, EquivocationEvent, FlashBlock,
    ReorgEvent, ReorgSeverity, SequencerStats,
};
use flashstat_db::FlashStorage;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
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

    pub async fn run(&self) -> Result<()> {
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
                self.run_http_poll_loop().await;
            }
        }

        Ok(())
    }

    async fn run_http_poll_loop(&self) {
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

    /// Verifies the TEE sequencer signature embedded in the block's extra_data.
    /// Returns `(tee_valid, confidence, signer, sequencer_signature)`.
    fn verify_tee_signature(
        &self,
        eth_block: &Block<H256>,
        hash: H256,
        number: U256,
    ) -> (bool, f64, Option<Address>, Option<Bytes>) {
        let Some(sig_bytes) = extract_signature_from_block(eth_block) else {
            return (false, 0.0, None, None);
        };

        match self.tee_verifier.recover_signer(hash, &sig_bytes) {
            Ok(recovered_signer) => {
                let tee_valid = recovered_signer == self.tee_verifier.expected_sequencer;

                if tee_valid {
                    info!(
                        "🛡️ TEE Signature Verified for block #{} by expected sequencer",
                        number
                    );
                    let confidence = self.check_attestation_confidence(eth_block, number);
                    (true, confidence, Some(recovered_signer), Some(sig_bytes))
                } else {
                    warn!(
                        "⚠️ TEE Signature valid but from unexpected signer: {:?}",
                        recovered_signer
                    );
                    (false, 0.0, Some(recovered_signer), Some(sig_bytes))
                }
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to recover signer from TEE signature for block #{}: {:?}",
                    number, e
                );
                (false, 0.0, None, None)
            }
        }
    }

    /// Checks TDX attestation when enabled and returns the appropriate confidence level.
    /// Assumes TEE signature has already been verified as valid.
    fn check_attestation_confidence(&self, eth_block: &Block<H256>, number: U256) -> f64 {
        if !self.config.tee.attestation_enabled {
            return 90.0;
        }

        let Some(quote) = extract_quote_from_block(eth_block) else {
            warn!(
                "⚠️ Attestation enabled but NO quote found in block #{}",
                number
            );
            return 85.0;
        };

        match self
            .tee_verifier
            .verify_tdx_attestation(&quote, self.config.tee.expected_mrenclave.as_deref())
        {
            Ok(true) => {
                info!("🛡️ TDX Attestation Verified for block #{}", number);
                99.0
            }
            Ok(false) => {
                warn!(
                    "⚠️ TEE Signature valid but Attestation Check FAILED for block #{}",
                    number
                );
                45.0
            }
            Err(e) => {
                warn!(
                    "⚠️ TEE Signature valid but Attestation verification ERROR for block #{}: {:?}",
                    number, e
                );
                70.0
            }
        }
    }

    /// Processes a confirmed hash conflict at a given block number.
    /// Classifies it as a soft reorg or equivocation, emits an event, and
    /// applies the appropriate reputation penalty to the signer.
    async fn process_reorg(
        &self,
        number: U256,
        hash: H256,
        prev: &FlashBlock,
        sequencer_signature: &Option<Bytes>,
        signer: &Option<Address>,
    ) -> Result<()> {
        let (severity, equivocation) = classify_reorg(prev, sequencer_signature, signer, number);

        let event = ReorgEvent {
            block_number: number,
            old_hash: prev.hash,
            new_hash: hash,
            detected_at: Utc::now(),
            severity,
            equivocation,
        };

        if severity == ReorgSeverity::Equivocation {
            spawn_watchtower_task(
                self.storage.clone(),
                self.provider.clone(),
                self.guardian_wallet.clone(),
                event.clone(),
                prev.sequencer_signature.clone().unwrap_or_default(),
                sequencer_signature.clone().unwrap_or_default(),
                signer.unwrap_or_default(),
                prev.hash,
                hash,
                number,
            );
        }

        self.storage.save_reorg(event.clone()).await?;
        let _ = self.event_tx.send(event.clone());

        if let Some(signer_addr) = signer {
            let (soft, equiv) = if severity == ReorgSeverity::Equivocation {
                (0, 1)
            } else {
                (1, 0)
            };
            if let Err(e) = self
                .update_reputation(*signer_addr, 0, soft, equiv, false)
                .await
            {
                error!("Failed to apply reputation penalty: {:?}", e);
            }
        }

        Ok(())
    }

    pub async fn handle_new_block(&self, eth_block: Block<H256>) -> Result<()> {
        let hash = eth_block.hash.unwrap_or_default();
        let number: U256 = eth_block.number.unwrap_or_default().as_u64().into();

        let mut last_block_guard = self.last_block.lock().await;

        let (tee_valid, tee_confidence, signer, sequencer_signature) =
            self.verify_tee_signature(&eth_block, hash, number);

        let mut persistence = 1;
        if let Some(ref prev) = *last_block_guard {
            if prev.number == number {
                if prev.hash != hash {
                    self.process_reorg(number, hash, prev, &sequencer_signature, &signer)
                        .await?;
                }
            } else if prev.number < number {
                persistence = ((prev.confidence / 100.0).log(0.5).abs().ceil() as u32).max(1) + 1;
            }
        }

        let confidence = resolve_confidence(tee_valid, tee_confidence, persistence);

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

        if let Some(signer_addr) = signer {
            let attested = confidence > 95.0;
            if let Err(e) = self.update_reputation(signer_addr, 1, 0, 0, attested).await {
                error!("Failed to update reputation: {:?}", e);
            }
        }

        *last_block_guard = Some(flash_block);
        Ok(())
    }

    pub async fn update_reputation(
        &self,
        address: Address,
        blocks: u64,
        soft_reorgs: u64,
        equivocations: u64,
        attested: bool,
    ) -> Result<()> {
        let mut stats =
            self.storage
                .get_sequencer_stats(address)
                .await?
                .unwrap_or(SequencerStats {
                    address,
                    last_active: Utc::now(),
                    ..Default::default()
                });

        apply_block_rewards(&mut stats, blocks, attested);
        apply_misbehaviour_penalties(&mut stats, soft_reorgs, equivocations);

        stats.last_active = Utc::now();
        stats.reputation_score = calculate_reputation_score(&stats);

        self.storage.update_sequencer_stats(stats).await?;
        Ok(())
    }
}

// ── Free-standing helpers ─────────────────────────────────────────────────────

/// Extracts the TEE sequencer signature from a block's extra_data.
/// In Unichain/OP-Stack, signatures are the last 65 bytes of extra_data.
pub(crate) fn extract_signature_from_block(block: &Block<H256>) -> Option<Bytes> {
    let extra_data = &block.extra_data;
    if extra_data.len() >= 65 {
        let sig = &extra_data[extra_data.len() - 65..];
        Some(Bytes::from(sig.to_vec()))
    } else {
        None
    }
}

/// Extracts the TEE attestation quote from a block's extra_data.
/// OP-Stack structure: [32-byte zero prefix][65-byte signature][optional quote]
pub(crate) fn extract_quote_from_block(block: &Block<H256>) -> Option<Bytes> {
    let extra_data = &block.extra_data;

    if extra_data.len() > 97 {
        let quote = &extra_data[97..];
        Some(Bytes::from(quote.to_vec()))
    } else {
        // Fallback: check if extra_data is an RLP list containing the quote
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

/// Classifies a block-height conflict as either a soft reorg or an equivocation.
/// Returns the severity and, if applicable, the `EquivocationEvent`.
fn classify_reorg(
    prev: &FlashBlock,
    sequencer_signature: &Option<Bytes>,
    signer: &Option<Address>,
    number: U256,
) -> (ReorgSeverity, Option<EquivocationEvent>) {
    let equivocation_check = (
        &prev.sequencer_signature,
        sequencer_signature,
        &prev.signer,
        signer,
    );

    if let (Some(sig1), Some(sig2), Some(signer1), Some(signer2)) = equivocation_check {
        if signer1 == signer2 {
            warn!(
                "🚨 EQUIVOCATION DETECTED at block #{} by signer {:?}!",
                number, signer1
            );
            return (
                ReorgSeverity::Equivocation,
                Some(EquivocationEvent {
                    signer: *signer1,
                    signature_1: sig1.clone(),
                    signature_2: sig2.clone(),
                    conflict_analysis: None,
                }),
            );
        }
    }

    warn!("🚨 Soft Reorg detected at block #{}!", number);
    (ReorgSeverity::Soft, None)
}

/// Spawns the background task that performs conflict analysis and submits
/// the on-chain equivocation proof via the guardian wallet if configured.
#[allow(clippy::too_many_arguments)]
fn spawn_watchtower_task(
    storage: Arc<dyn FlashStorage>,
    provider: Arc<Provider<Http>>,
    guardian: Option<Arc<wallet::GuardianWallet>>,
    event: ReorgEvent,
    sig1: Bytes,
    sig2: Bytes,
    signer_addr: Address,
    old_hash: H256,
    new_hash: H256,
    block_number: U256,
) {
    tokio::spawn(async move {
        if let Err(e) = analyze_and_update_equivocation(storage, provider, event).await {
            warn!("Failed to analyze conflicts: {:?}", e);
        }

        if let Some(wallet) = guardian {
            info!("🗼 Watchtower: Generating on-chain equivocation proof...");
            let encoded_proof = proof::encode_equivocation_proof(
                block_number,
                signer_addr,
                sig1,
                sig2,
                old_hash,
                new_hash,
            );
            match wallet.submit_equivocation_proof(encoded_proof).await {
                Ok(tx_hash) => info!(
                    "🚀 ACTIVE PROTECTION: Slashing proof submitted! TX: {:?}",
                    tx_hash
                ),
                Err(e) => error!("❌ Watchtower FAILED to submit proof: {:?}", e),
            }
        }
    });
}

/// Resolves the final block confidence from TEE and persistence signals.
/// TEE confidence takes priority when non-zero; otherwise falls back to
/// the persistence-based estimate.
fn resolve_confidence(tee_valid: bool, tee_confidence: f64, persistence: u32) -> f64 {
    if tee_confidence > 0.0 {
        return tee_confidence;
    }
    let base = (1.0 - 0.5f64.powi(persistence as i32)) * 100.0;
    if tee_valid { (base + 99.0) / 2.0 } else { base }
}

/// Applies block production rewards to a sequencer's stats in place.
fn apply_block_rewards(stats: &mut SequencerStats, blocks: u64, attested: bool) {
    if blocks > 0 {
        stats.total_blocks_signed += blocks;
        stats.current_streak += blocks;
        if attested {
            stats.total_attested_blocks += blocks;
        }
    }
}

/// Applies misbehaviour penalties to a sequencer's stats in place.
/// Any infraction resets the signing streak to zero.
fn apply_misbehaviour_penalties(stats: &mut SequencerStats, soft_reorgs: u64, equivocations: u64) {
    if soft_reorgs > 0 || equivocations > 0 {
        stats.total_soft_reorgs += soft_reorgs;
        stats.total_equivocations += equivocations;
        stats.current_streak = 0;
    }
}

/// Calculates the final reputation score from a sequencer's cumulative stats.
/// Formula: base_blocks + attestation_bonus + streak_bonus - penalties
pub(crate) fn calculate_reputation_score(stats: &SequencerStats) -> i64 {
    let base_score = stats.total_blocks_signed as i64;
    let attestation_bonus = stats.total_attested_blocks as i64;
    let streak_bonus = (stats.current_streak / 100) as i64 * 10;
    let penalty = (stats.total_soft_reorgs as i64 * 50) + (stats.total_equivocations as i64 * 1000);

    base_score + attestation_bonus + streak_bonus - penalty
}

/// Fetches both conflicting blocks and performs transaction-level conflict analysis,
/// then persists the enriched equivocation event to storage.
pub(crate) async fn analyze_and_update_equivocation(
    storage: Arc<dyn FlashStorage>,
    provider: Arc<Provider<Http>>,
    mut event: ReorgEvent,
) -> Result<()> {
    let Some(mut equivocation) = event.equivocation.take() else {
        return Ok(());
    };

    let (Some(old_block), Some(new_block)) = futures_util::try_join!(
        provider.get_block_with_txs(event.old_hash),
        provider.get_block_with_txs(event.new_hash)
    )?
    else {
        return Ok(());
    };

    equivocation.conflict_analysis = Some(build_conflict_analysis(&old_block, &new_block));
    event.equivocation = Some(equivocation);

    storage.save_reorg(event).await?;
    Ok(())
}

/// Diffs two conflicting blocks and returns the transaction-level conflict analysis:
/// which transactions were dropped and which represent potential double-spends.
fn build_conflict_analysis(
    old_block: &Block<Transaction>,
    new_block: &Block<Transaction>,
) -> ConflictAnalysis {
    let new_tx_map: HashMap<(Address, U256), H256> = new_block
        .transactions
        .iter()
        .map(|tx| ((tx.from, tx.nonce), tx.hash))
        .collect();

    let new_tx_hashes: std::collections::HashSet<H256> =
        new_block.transactions.iter().map(|tx| tx.hash).collect();

    let mut dropped_txs = Vec::new();
    let mut double_spend_txs = Vec::new();

    for old_tx in &old_block.transactions {
        if !new_tx_hashes.contains(&old_tx.hash) {
            dropped_txs.push(old_tx.hash);

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

    ConflictAnalysis {
        dropped_txs,
        double_spend_txs,
    }
}
