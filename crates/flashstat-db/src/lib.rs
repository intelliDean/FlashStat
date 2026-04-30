use async_trait::async_trait;
use ethers::types::H256;
use eyre::Result;
use flashstat_common::{FlashBlock, ReorgEvent, SequencerStats};
use redb::{Database, ReadableTable, TableDefinition};
use std::sync::Arc;

const BLOCKS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("blocks");
const REORGS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("reorgs");
const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const BLOCK_NUMBERS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("block_numbers");
const SEQUENCERS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("sequencer_stats");
const LATEST_BLOCK_KEY: &str = "latest_block_hash";

#[async_trait]
pub trait FlashStorage: Send + Sync {
    async fn save_block(&self, block: FlashBlock) -> Result<()>;
    async fn get_block(&self, hash: H256) -> Result<Option<FlashBlock>>;
    async fn save_reorg(&self, event: ReorgEvent) -> Result<()>;
    async fn get_latest_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>>;
    async fn get_equivocations(&self, limit: usize) -> Result<Vec<ReorgEvent>>;
    async fn get_latest_block(&self) -> Result<Option<FlashBlock>>;
    async fn get_recent_blocks(&self, limit: usize) -> Result<Vec<FlashBlock>>;
    async fn update_sequencer_stats(&self, stats: SequencerStats) -> Result<()>;
    async fn get_sequencer_stats(
        &self,
        address: ethers::types::Address,
    ) -> Result<Option<SequencerStats>>;
    async fn get_all_sequencer_stats(&self) -> Result<Vec<SequencerStats>>;
}

pub struct RedbStorage {
    db: Arc<Database>,
}

impl RedbStorage {
    pub fn new(path: &str) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Database::builder().create(path)?;

        // Initialize tables
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(BLOCKS_TABLE)?;
            let _ = write_txn.open_table(REORGS_TABLE)?;
            let _ = write_txn.open_table(META_TABLE)?;
            let _ = write_txn.open_table(BLOCK_NUMBERS_TABLE)?;
            let _ = write_txn.open_table(SEQUENCERS_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    pub fn new_readonly(path: &str) -> Result<Self> {
        // Redb doesn't have a specific "readonly" open mode in the same way,
        // but we can open it normally and only use read transactions.
        Self::new(path)
    }
}

#[async_trait]
impl FlashStorage for RedbStorage {
    async fn save_block(&self, block: FlashBlock) -> Result<()> {
        let key = block.hash.as_bytes();
        let val = serde_json::to_vec(&block)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(BLOCKS_TABLE)?;
            table.insert(key, val.as_slice())?;

            let mut meta = write_txn.open_table(META_TABLE)?;
            meta.insert(LATEST_BLOCK_KEY, key)?;

            let mut numbers = write_txn.open_table(BLOCK_NUMBERS_TABLE)?;
            numbers.insert(block.number.as_u64(), key)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    async fn get_block(&self, hash: H256) -> Result<Option<FlashBlock>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(BLOCKS_TABLE)?;
        let val = table.get(hash.as_bytes())?;

        if let Some(bytes) = val {
            let block = serde_json::from_slice(bytes.value())?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    async fn save_reorg(&self, event: ReorgEvent) -> Result<()> {
        let desc_ts = u64::MAX - event.detected_at.timestamp_nanos_opt().unwrap_or(0) as u64;
        let key = format!("{:020}:{}", desc_ts, event.block_number).into_bytes();
        let val = serde_json::to_vec(&event)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(REORGS_TABLE)?;
            table.insert(key.as_slice(), val.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    async fn get_latest_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(REORGS_TABLE)?;

        let mut results = Vec::new();
        // Redb iterators are sorted by key. Our keys are already descending timestamp.
        for item in table.iter()? {
            let (_key, value) = item?;
            let event: ReorgEvent = serde_json::from_slice(value.value())?;
            results.push(event);
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    async fn get_equivocations(&self, limit: usize) -> Result<Vec<ReorgEvent>> {
        use flashstat_common::ReorgSeverity;
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(REORGS_TABLE)?;

        let mut results = Vec::new();
        for item in table.iter()? {
            let (_key, value) = item?;
            let event: ReorgEvent = serde_json::from_slice(value.value())?;
            if event.severity == ReorgSeverity::Equivocation {
                results.push(event);
            }
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    async fn get_latest_block(&self) -> Result<Option<FlashBlock>> {
        let read_txn = self.db.begin_read()?;
        let meta = read_txn.open_table(META_TABLE)?;

        if let Some(hash_val) = meta.get(LATEST_BLOCK_KEY)? {
            let hash_bytes = hash_val.value();
            let table = read_txn.open_table(BLOCKS_TABLE)?;
            if let Some(block_val) = table.get(hash_bytes)? {
                return Ok(Some(serde_json::from_slice(block_val.value())?));
            }
        }

        Ok(None)
    }

    async fn get_recent_blocks(&self, limit: usize) -> Result<Vec<FlashBlock>> {
        let read_txn = self.db.begin_read()?;
        let numbers = read_txn.open_table(BLOCK_NUMBERS_TABLE)?;
        let blocks_table = read_txn.open_table(BLOCKS_TABLE)?;

        let mut results = Vec::new();
        // Iterate backwards from the largest block number
        for item in numbers.iter()?.rev() {
            let (_number, hash_bytes) = item?;
            if let Some(block_val) = blocks_table.get(hash_bytes.value())? {
                results.push(serde_json::from_slice(block_val.value())?);
            }
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    async fn update_sequencer_stats(&self, stats: SequencerStats) -> Result<()> {
        let key = stats.address.as_bytes();
        let val = serde_json::to_vec(&stats)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SEQUENCERS_TABLE)?;
            table.insert(key, val.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    async fn get_sequencer_stats(
        &self,
        address: ethers::types::Address,
    ) -> Result<Option<SequencerStats>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SEQUENCERS_TABLE)?;
        let val = table.get(address.as_bytes())?;

        if let Some(bytes) = val {
            Ok(Some(serde_json::from_slice(bytes.value())?))
        } else {
            Ok(None)
        }
    }

    async fn get_all_sequencer_stats(&self) -> Result<Vec<SequencerStats>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SEQUENCERS_TABLE)?;

        let mut results = Vec::new();
        for item in table.iter()? {
            let (_key, value) = item?;
            results.push(serde_json::from_slice(value.value())?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ethers::types::{Address, H256, U256};
    use flashstat_common::{
        BlockStatus, EquivocationEvent, FlashBlock, ReorgEvent, ReorgSeverity, SequencerStats,
    };
    use tempfile::tempdir;

    fn make_block(number: u64, hash: H256) -> FlashBlock {
        FlashBlock {
            number: U256::from(number),
            hash,
            parent_hash: H256::random(),
            timestamp: Utc::now(),
            sequencer_signature: None,
            signer: None,
            confidence: 50.0,
            status: BlockStatus::Pending,
        }
    }

    fn make_reorg(block_number: u64, severity: ReorgSeverity) -> ReorgEvent {
        ReorgEvent {
            block_number: U256::from(block_number),
            old_hash: H256::random(),
            new_hash: H256::random(),
            detected_at: Utc::now(),
            severity,
            equivocation: if severity == ReorgSeverity::Equivocation {
                Some(EquivocationEvent {
                    signer: Address::random(),
                    signature_1: vec![0u8; 65].into(),
                    signature_2: vec![1u8; 65].into(),
                    conflict_analysis: None,
                })
            } else {
                None
            },
        }
    }

    fn open_storage() -> RedbStorage {
        let dir = tempdir().unwrap();
        #[allow(deprecated)]
        let path = dir.into_path().join("test.db");
        RedbStorage::new(path.to_str().unwrap()).unwrap()
    }

    // ── Blocks ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_save_and_get_block() {
        let db = open_storage();
        let hash = H256::random();
        let block = make_block(1, hash);

        db.save_block(block.clone()).await.unwrap();

        let retrieved = db.get_block(hash).await.unwrap().unwrap();
        assert_eq!(retrieved.hash, hash);
        assert_eq!(retrieved.number, U256::from(1u64));
    }

    #[tokio::test]
    async fn test_get_block_returns_none_for_unknown_hash() {
        let db = open_storage();
        let result = db.get_block(H256::random()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_block_tracks_most_recent() {
        let db = open_storage();

        let block_1 = make_block(1, H256::random());
        let block_2 = make_block(2, H256::random());
        let hash_2 = block_2.hash;

        db.save_block(block_1).await.unwrap();
        db.save_block(block_2).await.unwrap();

        let latest = db.get_latest_block().await.unwrap().unwrap();
        assert_eq!(latest.hash, hash_2, "latest block should be block 2");
    }

    #[tokio::test]
    async fn test_get_latest_block_returns_none_on_empty_db() {
        let db = open_storage();
        let result = db.get_latest_block().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_recent_blocks_returns_in_descending_order() {
        let db = open_storage();

        for i in 1u64..=5 {
            db.save_block(make_block(i, H256::random())).await.unwrap();
        }

        let blocks = db.get_recent_blocks(3).await.unwrap();
        assert_eq!(blocks.len(), 3);
        // Should be blocks 5, 4, 3 (most recent first)
        assert_eq!(blocks[0].number, U256::from(5u64));
        assert_eq!(blocks[1].number, U256::from(4u64));
        assert_eq!(blocks[2].number, U256::from(3u64));
    }

    #[tokio::test]
    async fn test_get_recent_blocks_respects_limit() {
        let db = open_storage();
        for i in 1u64..=10 {
            db.save_block(make_block(i, H256::random())).await.unwrap();
        }

        let blocks = db.get_recent_blocks(4).await.unwrap();
        assert_eq!(blocks.len(), 4);
    }

    // ── Reorgs ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_save_and_retrieve_soft_reorg() {
        let db = open_storage();
        let event = make_reorg(100, ReorgSeverity::Soft);

        db.save_reorg(event.clone()).await.unwrap();

        let reorgs = db.get_latest_reorgs(10).await.unwrap();
        assert_eq!(reorgs.len(), 1);
        assert_eq!(reorgs[0].block_number, U256::from(100u64));
        assert_eq!(reorgs[0].severity, ReorgSeverity::Soft);
    }

    #[tokio::test]
    async fn test_get_equivocations_filters_correctly() {
        let db = open_storage();

        db.save_reorg(make_reorg(1, ReorgSeverity::Soft))
            .await
            .unwrap();
        db.save_reorg(make_reorg(2, ReorgSeverity::Equivocation))
            .await
            .unwrap();
        db.save_reorg(make_reorg(3, ReorgSeverity::Soft))
            .await
            .unwrap();
        db.save_reorg(make_reorg(4, ReorgSeverity::Equivocation))
            .await
            .unwrap();

        let equivocations = db.get_equivocations(10).await.unwrap();
        assert_eq!(equivocations.len(), 2);
        assert!(
            equivocations
                .iter()
                .all(|e| e.severity == ReorgSeverity::Equivocation)
        );
    }

    #[tokio::test]
    async fn test_get_latest_reorgs_respects_limit() {
        let db = open_storage();
        for i in 0..5 {
            db.save_reorg(make_reorg(i, ReorgSeverity::Soft))
                .await
                .unwrap();
        }

        let reorgs = db.get_latest_reorgs(2).await.unwrap();
        assert_eq!(reorgs.len(), 2);
    }

    // ── Sequencer Stats ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_and_get_sequencer_stats() {
        let db = open_storage();
        let address = Address::random();

        let stats = SequencerStats {
            address,
            total_blocks_signed: 42,
            total_attested_blocks: 10,
            total_soft_reorgs: 1,
            total_equivocations: 0,
            current_streak: 42,
            reputation_score: 500,
            last_active: Utc::now(),
        };

        db.update_sequencer_stats(stats.clone()).await.unwrap();

        let retrieved = db.get_sequencer_stats(address).await.unwrap().unwrap();
        assert_eq!(retrieved.address, address);
        assert_eq!(retrieved.total_blocks_signed, 42);
        assert_eq!(retrieved.reputation_score, 500);
    }

    #[tokio::test]
    async fn test_get_sequencer_stats_returns_none_for_unknown_address() {
        let db = open_storage();
        let result = db.get_sequencer_stats(Address::random()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_all_sequencer_stats() {
        let db = open_storage();

        for i in 0u64..3 {
            let address = Address::random();
            db.update_sequencer_stats(SequencerStats {
                address,
                reputation_score: i as i64 * 100,
                last_active: Utc::now(),
                ..Default::default()
            })
            .await
            .unwrap();
        }

        let all = db.get_all_sequencer_stats().await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_update_sequencer_stats_overwrites_existing() {
        let db = open_storage();
        let address = Address::random();

        db.update_sequencer_stats(SequencerStats {
            address,
            reputation_score: 100,
            last_active: Utc::now(),
            ..Default::default()
        })
        .await
        .unwrap();

        db.update_sequencer_stats(SequencerStats {
            address,
            reputation_score: 999,
            last_active: Utc::now(),
            ..Default::default()
        })
        .await
        .unwrap();

        let stats = db.get_sequencer_stats(address).await.unwrap().unwrap();
        assert_eq!(stats.reputation_score, 999);
    }
}
