use async_trait::async_trait;
use ethers::types::H256;
use eyre::Result;
use flashstat_common::{FlashBlock, ReorgEvent};
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
    async fn update_sequencer_stats(&self, stats: flashstat_common::SequencerStats) -> Result<()>;
    async fn get_sequencer_stats(&self, address: ethers::types::Address) -> Result<Option<flashstat_common::SequencerStats>>;
    async fn get_all_sequencer_stats(&self) -> Result<Vec<flashstat_common::SequencerStats>>;
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

    async fn update_sequencer_stats(&self, stats: flashstat_common::SequencerStats) -> Result<()> {
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

    async fn get_sequencer_stats(&self, address: ethers::types::Address) -> Result<Option<flashstat_common::SequencerStats>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SEQUENCERS_TABLE)?;
        let val = table.get(address.as_bytes())?;

        if let Some(bytes) = val {
            Ok(Some(serde_json::from_slice(bytes.value())?))
        } else {
            Ok(None)
        }
    }

    async fn get_all_sequencer_stats(&self) -> Result<Vec<flashstat_common::SequencerStats>> {
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
