pub mod monitor;
pub mod proof;
pub mod tee;
pub mod wallet;

pub use monitor::FlashMonitor;

#[cfg(test)]
mod tests {
    use super::monitor::*;
    use crate::monitor::{extract_quote_from_block, extract_signature_from_block};
    use ethers::prelude::*;
    use eyre::Result;
    use flashstat_common::*;
    use flashstat_db::{FlashStorage, RedbStorage};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::broadcast;

    // ── Helpers ──────────────────────────────────────────────────────────────

    async fn make_monitor(storage: Arc<dyn FlashStorage>) -> FlashMonitor {
        let dir = tempdir().unwrap();
        let config = Config {
            rpc: RpcConfig {
                ws_url: "ws://localhost:8545".into(),
                http_url: "http://localhost:8545".into(),
            },
            storage: StorageConfig {
                db_path: dir.path().join("test.db").to_str().unwrap().into(),
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
        FlashMonitor::new(config, storage, rx).await.unwrap()
    }

    fn make_plain_block(number: u64, hash: H256) -> Block<H256> {
        Block {
            number: Some(number.into()),
            hash: Some(hash),
            parent_hash: H256::random(),
            timestamp: U256::from(0u64),
            ..Default::default()
        }
    }

    // ── Reputation Scoring ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_reputation_scoring() -> Result<()> {
        let dir = tempdir()?;
        let storage = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;

        let address = Address::random();

        // 1. Reward: 100 blocks + attested
        monitor.update_reputation(address, 100, 0, 0, true).await?;
        let stats = storage.get_sequencer_stats(address).await?.unwrap();
        // Base(100) + Attestation(100) + Streak(10) = 210
        assert_eq!(stats.reputation_score, 210);
        assert_eq!(stats.current_streak, 100);

        // 2. Penalty: Equivocation resets streak and applies heavy penalty
        monitor.update_reputation(address, 0, 0, 1, false).await?;
        let stats = storage.get_sequencer_stats(address).await?.unwrap();
        // Base(100) + Attest(100) + Streak(0) - 1000 = -800
        assert_eq!(stats.reputation_score, -800);
        assert_eq!(stats.current_streak, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_soft_reorg_penalty_is_lighter_than_equivocation() -> Result<()> {
        let dir = tempdir()?;
        let storage = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;
        let addr_soft = Address::random();
        let addr_equiv = Address::random();

        monitor.update_reputation(addr_soft, 0, 1, 0, false).await?;
        monitor
            .update_reputation(addr_equiv, 0, 0, 1, false)
            .await?;

        let soft_score = storage
            .get_sequencer_stats(addr_soft)
            .await?
            .unwrap()
            .reputation_score;
        let equiv_score = storage
            .get_sequencer_stats(addr_equiv)
            .await?
            .unwrap()
            .reputation_score;

        assert!(
            soft_score > equiv_score,
            "equivocation penalty should be far harsher than soft reorg"
        );
        Ok(())
    }

    // ── Block Ingestion ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_handle_new_block_persists_to_storage() -> Result<()> {
        let dir = tempdir()?;
        let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;

        let hash = H256::random();
        let block = make_plain_block(1, hash);

        monitor.handle_new_block(block).await?;

        let stored = storage.get_block(hash).await?.unwrap();
        assert_eq!(stored.hash, hash);
        assert_eq!(stored.number, U256::from(1u64));

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_new_block_updates_latest_block() -> Result<()> {
        let dir = tempdir()?;
        let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;

        monitor
            .handle_new_block(make_plain_block(1, H256::random()))
            .await?;
        let hash_2 = H256::random();
        monitor
            .handle_new_block(make_plain_block(2, hash_2))
            .await?;

        let latest = storage.get_latest_block().await?.unwrap();
        assert_eq!(latest.hash, hash_2);

        Ok(())
    }

    #[tokio::test]
    async fn test_handle_new_block_broadcasts_on_channel() -> Result<()> {
        let dir = tempdir()?;
        let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;

        let mut block_rx = monitor.block_notifier().subscribe();
        let hash = H256::random();

        monitor.handle_new_block(make_plain_block(42, hash)).await?;

        let received = block_rx.try_recv().expect("expected block on channel");
        assert_eq!(received.hash, hash);

        Ok(())
    }

    // ── Reorg Detection ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_soft_reorg_detected_on_conflicting_hashes() -> Result<()> {
        let dir = tempdir()?;
        let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;
        let mut event_rx = monitor.event_notifier().subscribe();

        // First block at height 100
        monitor
            .handle_new_block(make_plain_block(100, H256::random()))
            .await?;
        // Conflicting block at same height
        monitor
            .handle_new_block(make_plain_block(100, H256::random()))
            .await?;

        let event = event_rx.try_recv().expect("expected a reorg event");
        assert_eq!(event.block_number, U256::from(100u64));
        assert_eq!(event.severity, ReorgSeverity::Soft);

        Ok(())
    }

    #[tokio::test]
    async fn test_no_reorg_event_for_sequential_blocks() -> Result<()> {
        let dir = tempdir()?;
        let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;
        let mut event_rx = monitor.event_notifier().subscribe();

        monitor
            .handle_new_block(make_plain_block(1, H256::random()))
            .await?;
        monitor
            .handle_new_block(make_plain_block(2, H256::random()))
            .await?;

        assert!(
            event_rx.try_recv().is_err(),
            "no reorg expected for sequential blocks"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_soft_reorg_persisted_to_storage() -> Result<()> {
        let dir = tempdir()?;
        let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(
            dir.path().join("test.db").to_str().unwrap(),
        )?);
        let monitor = make_monitor(storage.clone()).await;

        monitor
            .handle_new_block(make_plain_block(5, H256::random()))
            .await?;
        monitor
            .handle_new_block(make_plain_block(5, H256::random()))
            .await?;

        let reorgs = storage.get_latest_reorgs(10).await?;
        assert_eq!(reorgs.len(), 1, "reorg event should be persisted");
        assert_eq!(reorgs[0].severity, ReorgSeverity::Soft);

        Ok(())
    }

    // ── Extraction Helpers ───────────────────────────────────────────────────

    #[test]
    fn test_extract_signature_returns_last_65_bytes() {
        let mut extra = vec![0u8; 32];
        extra.extend_from_slice(&[0xAB; 65]);

        let block = Block::<H256> {
            extra_data: Bytes::from(extra.clone()),
            ..Default::default()
        };

        let sig = extract_signature_from_block(&block).unwrap();
        assert_eq!(sig.len(), 65);
        assert!(sig.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn test_extract_signature_returns_none_for_short_extra_data() {
        let block = Block::<H256> {
            extra_data: Bytes::from(vec![0u8; 10]),
            ..Default::default()
        };
        assert!(extract_signature_from_block(&block).is_none());
    }

    #[test]
    fn test_extract_quote_returns_bytes_after_97() {
        let mut extra = vec![0u8; 32]; // zero prefix
        extra.extend_from_slice(&[0x11; 65]); // signature
        extra.extend_from_slice(&[0xFF; 200]); // quote payload

        let block = Block::<H256> {
            extra_data: Bytes::from(extra),
            ..Default::default()
        };

        let quote = extract_quote_from_block(&block).unwrap();
        assert_eq!(quote.len(), 200);
        assert!(quote.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn test_extract_quote_returns_none_when_no_quote_present() {
        // Only a signature, no quote trailing
        let mut extra = vec![0u8; 32];
        extra.extend_from_slice(&[0x11; 65]);

        let block = Block::<H256> {
            extra_data: Bytes::from(extra),
            ..Default::default()
        };

        // 97 bytes exactly — no quote payload
        assert!(extract_quote_from_block(&block).is_none());
    }

    // ── Proof Serialization ──────────────────────────────────────────────────

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

    #[test]
    fn test_equivocation_proof_serialization() {
        use crate::proof;
        let bytes = proof::encode_equivocation_proof(
            U256::from(99u64),
            Address::random(),
            vec![0u8; 65].into(),
            vec![1u8; 65].into(),
            H256::random(),
            H256::random(),
        );
        assert!(!bytes.is_empty());
    }
}
