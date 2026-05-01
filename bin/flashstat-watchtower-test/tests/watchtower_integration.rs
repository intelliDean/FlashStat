//! Integration tests for the FlashStat Watchtower pipeline.
//!
//! These tests exercise the full path from block ingestion through
//! reorg/equivocation detection and storage persistence — entirely
//! in-process with a temporary database, no live RPC node required.

use ethers::types::{Address, Block, Bytes, H256, U256};
use eyre::Result;
use flashstat_common::*;
use flashstat_core::FlashMonitor;
use flashstat_db::{FlashStorage, RedbStorage};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::broadcast;

// ── Test Fixture ─────────────────────────────────────────────────────────────

struct Fixture {
    monitor: FlashMonitor,
    storage: Arc<dyn FlashStorage>,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempdir().unwrap();
        let storage: Arc<dyn FlashStorage> = Arc::new(
            RedbStorage::new(dir.path().to_owned().join("test.db").to_str().unwrap()).unwrap()
        );

        let config = Config {
            rpc: RpcConfig {
                ws_url: "ws://localhost:8545".into(),
                http_url: "http://localhost:8545".into(),
            },
            storage: StorageConfig {
                db_path: "/tmp/unused".into(),
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

        let (_tx, rx) = broadcast::channel(16);
        let monitor = FlashMonitor::new(config, storage.clone(), rx)
            .await
            .unwrap();

        Self { monitor, storage }
    }
}

fn plain_block(number: u64, hash: H256) -> Block<H256> {
    Block {
        number: Some(number.into()),
        hash: Some(hash),
        parent_hash: H256::random(),
        timestamp: U256::from(0u64),
        ..Default::default()
    }
}

/// Builds a block whose extra_data carries a mock 65-byte OP-Stack signature
/// in the last 65 bytes, as `extract_signature_from_block` expects.
fn signed_block(number: u64, hash: H256, sig_byte: u8) -> Block<H256> {
    let mut extra: Vec<u8> = vec![0u8; 32]; // zero prefix
    extra.extend_from_slice(&[sig_byte; 65]); // signature marker

    Block {
        number: Some(number.into()),
        hash: Some(hash),
        parent_hash: H256::random(),
        timestamp: U256::from(0u64),
        extra_data: Bytes::from(extra),
        ..Default::default()
    }
}

// ── Block Ingestion ──────────────────────────────────────────────────────────

#[tokio::test]
async fn integration_single_block_ingested_and_retrievable() -> Result<()> {
    let f = Fixture::new().await;

    let hash = H256::random();
    f.monitor.handle_new_block(plain_block(1, hash)).await?;

    let block = f
        .storage
        .get_block(hash)
        .await?
        .expect("block should be stored");
    assert_eq!(block.hash, hash);
    assert_eq!(block.number, U256::from(1u64));

    Ok(())
}

#[tokio::test]
async fn integration_sequential_blocks_no_reorg_events() -> Result<()> {
    let f = Fixture::new().await;
    let mut event_rx = f.monitor.event_notifier().subscribe();

    f.monitor
        .handle_new_block(plain_block(1, H256::random()))
        .await?;
    f.monitor
        .handle_new_block(plain_block(2, H256::random()))
        .await?;
    f.monitor
        .handle_new_block(plain_block(3, H256::random()))
        .await?;

    assert!(
        event_rx.try_recv().is_err(),
        "sequential blocks should produce no reorg events"
    );

    Ok(())
}

#[tokio::test]
async fn integration_latest_block_reflects_most_recent() -> Result<()> {
    let f = Fixture::new().await;

    f.monitor
        .handle_new_block(plain_block(1, H256::random()))
        .await?;
    let hash_3 = H256::random();
    f.monitor
        .handle_new_block(plain_block(2, H256::random()))
        .await?;
    f.monitor.handle_new_block(plain_block(3, hash_3)).await?;

    let latest = f
        .storage
        .get_latest_block()
        .await?
        .expect("should have a latest block");
    assert_eq!(latest.hash, hash_3, "latest block should be block #3");

    Ok(())
}

// ── Soft Reorg Detection ─────────────────────────────────────────────────────

#[tokio::test]
async fn integration_soft_reorg_detected_on_hash_conflict() -> Result<()> {
    let f = Fixture::new().await;
    let mut event_rx = f.monitor.event_notifier().subscribe();

    f.monitor
        .handle_new_block(plain_block(100, H256::random()))
        .await?;
    f.monitor
        .handle_new_block(plain_block(100, H256::random()))
        .await?;

    let event = event_rx
        .try_recv()
        .expect("soft reorg event should be emitted");
    assert_eq!(event.block_number, U256::from(100u64));
    assert_eq!(event.severity, ReorgSeverity::Soft);
    assert!(
        event.equivocation.is_none(),
        "soft reorg should have no equivocation data"
    );

    Ok(())
}

#[tokio::test]
async fn integration_soft_reorg_is_persisted_to_storage() -> Result<()> {
    let f = Fixture::new().await;

    f.monitor
        .handle_new_block(plain_block(50, H256::random()))
        .await?;
    f.monitor
        .handle_new_block(plain_block(50, H256::random()))
        .await?;

    let reorgs = f.storage.get_latest_reorgs(10).await?;
    assert_eq!(reorgs.len(), 1);
    assert_eq!(reorgs[0].severity, ReorgSeverity::Soft);

    Ok(())
}

#[tokio::test]
async fn integration_duplicate_same_hash_at_same_height_not_a_reorg() -> Result<()> {
    let f = Fixture::new().await;
    let mut event_rx = f.monitor.event_notifier().subscribe();

    let hash = H256::random();
    f.monitor.handle_new_block(plain_block(10, hash)).await?;
    f.monitor.handle_new_block(plain_block(10, hash)).await?; // same hash, not a conflict

    assert!(
        event_rx.try_recv().is_err(),
        "identical block re-ingestion should not produce a reorg event"
    );

    Ok(())
}

// ── Reputation Pipeline ──────────────────────────────────────────────────────

#[tokio::test]
async fn integration_reputation_increases_with_block_production() -> Result<()> {
    let f = Fixture::new().await;
    let address = Address::random();

    f.monitor
        .update_reputation(address, 50, 0, 0, false)
        .await?;

    let stats = f
        .storage
        .get_sequencer_stats(address)
        .await?
        .expect("stats should exist");

    assert_eq!(stats.total_blocks_signed, 50);
    assert!(stats.reputation_score > 0);

    Ok(())
}

#[tokio::test]
async fn integration_equivocation_reputation_penalty_wipes_score() -> Result<()> {
    let f = Fixture::new().await;
    let address = Address::random();

    // Build up a good reputation first
    f.monitor
        .update_reputation(address, 200, 0, 0, true)
        .await?;
    let before = f
        .storage
        .get_sequencer_stats(address)
        .await?
        .unwrap()
        .reputation_score;
    assert!(before > 0);

    // Single equivocation should crush it
    f.monitor.update_reputation(address, 0, 0, 1, false).await?;
    let after = f
        .storage
        .get_sequencer_stats(address)
        .await?
        .unwrap()
        .reputation_score;

    assert!(
        after < 0,
        "equivocation should result in a negative score: got {}",
        after
    );

    Ok(())
}

#[tokio::test]
async fn integration_streak_bonus_accumulates_correctly() -> Result<()> {
    let f = Fixture::new().await;
    let address = Address::random();

    // 200 blocks → streak bonus at 100-block intervals = +20
    f.monitor
        .update_reputation(address, 200, 0, 0, false)
        .await?;
    let stats = f.storage.get_sequencer_stats(address).await?.unwrap();

    // Base(200) + Streak(200/100 * 10 = 20) = 220
    assert_eq!(stats.reputation_score, 220);

    Ok(())
}

// ── Block broadcast ──────────────────────────────────────────────────────────

#[tokio::test]
async fn integration_block_broadcast_fires_for_each_ingested_block() -> Result<()> {
    let f = Fixture::new().await;
    let mut rx = f.monitor.block_notifier().subscribe();

    let hashes = [H256::random(), H256::random(), H256::random()];
    for (i, &hash) in hashes.iter().enumerate() {
        f.monitor
            .handle_new_block(plain_block(i as u64 + 1, hash))
            .await?;
    }

    let mut received = Vec::new();
    while let Ok(block) = rx.try_recv() {
        received.push(block.hash);
    }

    assert_eq!(
        received.len(),
        3,
        "should receive one broadcast per ingested block"
    );
    for hash in &hashes {
        assert!(
            received.contains(hash),
            "hash {:?} should have been broadcast",
            hash
        );
    }

    Ok(())
}

// ── Watchtower Scenario (replaces the old manual binary) ────────────────────

/// This is the full equivocation simulation that the old flashstat-watchtower-test
/// binary ran manually. It is now a deterministic in-process test.
#[tokio::test]
async fn integration_watchtower_equivocation_scenario() -> Result<()> {
    let f = Fixture::new().await;
    let mut event_rx = f.monitor.event_notifier().subscribe();

    let block_number = 99_999u64;

    // Two conflicting blocks at the same height, with different signatures
    // (the signatures are mock bytes — real equivocation detection requires
    // both blocks to carry a recoverable ECDSA sig from the same signer,
    // which requires a live key; here we test the soft-reorg path that
    // `process_reorg` always triggers when hashes differ).
    let block_a = signed_block(block_number, H256::random(), 0x11);
    let block_b = signed_block(block_number, H256::random(), 0x22);

    f.monitor.handle_new_block(block_a).await?;
    f.monitor.handle_new_block(block_b).await?;

    // A reorg event should always fire when the hashes conflict.
    let event = event_rx
        .try_recv()
        .expect("reorg event expected after conflicting blocks");
    assert_eq!(event.block_number, U256::from(block_number));

    // Verify it was persisted
    let reorgs = f.storage.get_latest_reorgs(5).await?;
    assert_eq!(reorgs.len(), 1, "exactly one reorg event should be stored");
    assert_eq!(reorgs[0].block_number, U256::from(block_number));

    Ok(())
}
