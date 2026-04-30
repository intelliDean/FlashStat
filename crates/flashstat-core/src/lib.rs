use ethers::types::{Block, H256, U256};
use eyre::{Context, Result};
use flashstat_common::{Config, FlashBlock, ReorgEvent, ReorgSeverity};
use flashstat_db::FlashStorage;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info};

pub mod proof;
pub mod tee;
pub mod wallet;

use crate::tee::TeeVerifier;

pub struct FlashMonitor {
    config: Config,
    storage: Arc<dyn FlashStorage>,
    last_block: Arc<Mutex<Option<FlashBlock>>>,
    shutdown_rx: broadcast::Receiver<()>,
    tee_verifier: TeeVerifier,
    block_tx: broadcast::Sender<FlashBlock>,
    event_tx: broadcast::Sender<ReorgEvent>,
    provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    guardian_wallet: Option<Arc<wallet::GuardianWallet>>,
}

impl FlashMonitor {
    pub async fn new(
        config: Config,
        storage: Arc<dyn FlashStorage>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<Self> {
        let last_block = Arc::new(Mutex::new(storage.get_latest_block().await?));
        let tee_verifier = TeeVerifier::new(config.clone());

        let (block_tx, _) = broadcast::channel(100);
        let (event_tx, _) = broadcast::channel(100);

        let provider = Arc::new(ethers::providers::Provider::try_from(&config.rpc.http_url)?);

        let guardian_wallet = if let Some(guardian_config) = &config.guardian {
            Some(Arc::new(
                wallet::GuardianWallet::new(guardian_config.clone()).await?,
            ))
        } else {
            None
        };

        Ok(Self {
            config,
            storage,
            last_block,
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
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        }

        info!("🏮 Monitor shutdown complete");
        Ok(())
    }

    async fn supervise_connection(&self) -> Result<()> {
        let ws_url = &self.config.rpc.ws_url;
        info!("🔌 Connecting to Unichain WebSocket: {}", ws_url);

        let provider = ethers::providers::Provider::<ethers::providers::Ws>::connect(ws_url)
            .await
            .context("Failed to connect to WS")?;

        let mut stream = provider
            .subscribe_blocks()
            .await
            .context("Failed to subscribe to blocks")?;

        info!("📡 Subscribed to Flashblocks. Monitoring for soft-finality...");

        while let Some(block) = stream.next().await {
            if let Err(e) = self.handle_new_block(block).await {
                error!("Error handling block: {:?}", e);
            }
        }

        Ok(())
    }

    pub async fn handle_new_block(&self, eth_block: Block<H256>) -> Result<()> {
        let hash = eth_block.hash.unwrap_or_default();
        let number: U256 = eth_block.number.unwrap_or_default().as_u64().into();

        // 1. Verify TEE Signature
        let sequencer_signature = self.tee_verifier.extract_signature(&eth_block)?;
        let is_tee_valid = self
            .tee_verifier
            .verify_sequencer_signature(hash, &sequencer_signature)
            .is_ok();

        // 2. Check for Equivocation (Soft Reorg)
        let mut last_block_guard = self.last_block.lock().await;
        if let Some(last) = &*last_block_guard {
            if last.number == number && last.hash != hash {
                info!("⚠️  EQUIVOCATION DETECTED at block #{}", number);

                let event = ReorgEvent {
                    block_number: number,
                    old_hash: last.hash,
                    new_hash: hash,
                    detected_at: chrono::Utc::now(),
                    severity: ReorgSeverity::Equivocation,
                    equivocation: Some(flashstat_common::EquivocationEvent {
                        signer: self
                            .tee_verifier
                            .recover_signer(hash, &sequencer_signature)?,
                        signature_1: last.sequencer_signature.clone(),
                        signature_2: sequencer_signature.clone(),
                        conflict_analysis: None,
                    }),
                };

                self.storage.save_reorg(event.clone()).await?;
                let _ = self.event_tx.send(event.clone());

                // Active Slashing Protection
                if let Some(guardian) = &self.guardian_wallet {
                    let event_clone = event.clone();
                    let guardian_clone = guardian.clone();
                    tokio::spawn(async move {
                        if let Err(e) = guardian_clone.handle_equivocation(event_clone).await {
                            error!("Slashing submission failed: {:?}", e);
                        }
                    });
                }
            }
        }

        // 3. Calculate Confidence
        let confidence = if is_tee_valid { 75.0 } else { 0.0 };

        let flash_block = FlashBlock {
            number,
            hash,
            parent_hash: eth_block.parent_hash,
            timestamp: eth_block.timestamp,
            confidence,
            sequencer_signature,
            is_tee_attested: is_tee_valid,
            received_at: chrono::Utc::now(),
        };

        // 4. Update Persistence & Reputation
        self.storage.save_block(flash_block.clone()).await?;
        self.update_reputation(&flash_block).await?;

        *last_block_guard = Some(flash_block.clone());
        let _ = self.block_tx.send(flash_block);

        info!(
            "📦 Block #{} | Confidence: {:.2}% | Hash: {:?}",
            number, confidence, hash
        );

        Ok(())
    }

    async fn update_reputation(&self, block: &FlashBlock) -> Result<()> {
        let signer = self
            .tee_verifier
            .recover_signer(block.hash, &block.sequencer_signature)?;
        let mut stats = self.storage.get_sequencer_stats(signer).await?.unwrap_or(
            flashstat_common::SequencerStats {
                address: signer,
                total_blocks: 0,
                attested_blocks: 0,
                equivocations: 0,
                reputation_score: 100,
                last_seen: chrono::Utc::now(),
            },
        );

        stats.total_blocks += 1;
        if block.is_tee_attested {
            stats.attested_blocks += 1;
            stats.reputation_score = (stats.reputation_score + 1).min(100);
        }
        stats.last_seen = chrono::Utc::now();

        self.storage.save_sequencer_stats(stats).await?;
        Ok(())
    }
}
