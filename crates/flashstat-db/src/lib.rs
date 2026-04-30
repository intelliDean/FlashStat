use ethers::types::{Address, H256, U256};
use eyre::Result;
use flashstat_common::{FlashBlock, ReorgEvent, SequencerStats};
use redb::{Database, TableDefinition, WriteStrategy};
use std::sync::Arc;

const BLOCKS_TABLE: TableDefinition<&[u8; 32], &[u8]> = TableDefinition::new("blocks");
const REORGS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("reorgs");
const SEQUENCERS_TABLE: TableDefinition<&[u8; 20], &[u8]> = TableDefinition::new("sequencers");

#[async_trait::async_trait]
pub trait FlashStorage: Send + Sync {
    async fn save_block(&self, block: FlashBlock) -> Result<()>;
    async fn get_block(&self, hash: H256) -> Result<Option<FlashBlock>>;
    async fn get_latest_block(&self) -> Result<Option<FlashBlock>>;
    async fn get_recent_blocks(&self, limit: usize) -> Result<Vec<FlashBlock>>;

    async fn save_reorg(&self, event: ReorgEvent) -> Result<()>;
    async fn get_latest_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>>;

    async fn update_sequencer_stats(&self, stats: SequencerStats) -> Result<()>;
    async fn get_sequencer_stats(&self, address: Address) -> Result<Option<SequencerStats>>;
    async fn get_all_sequencer_stats(&self) -> Result<Vec<SequencerStats>>;
}

pub struct RedbStorage {
    db: Database,
}

impl RedbStorage {
    pub fn new(path: &str) -> Result<Self> {
        let db = Database::builder()
            .set_write_strategy(WriteStrategy::TwoPhase)
            .create(path)?;

        // Initialize tables
        let write_txn = db.begin_write()?;
        {
            write_txn.open_table(BLOCKS_TABLE)?;
            write_txn.open_table(REORGS_TABLE)?;
            write_txn.open_table(SEQUENCERS_TABLE)?;
        }
        write_txn.commit()?;

        Ok(Self { db })
    }
}

#[async_trait::async_trait]
impl FlashStorage for RedbStorage {
    async fn save_block(&self, block: FlashBlock) -> Result<()> {
        let key = block.hash.as_fixed_bytes();
        let val = serde_json::to_vec(&block)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(BLOCKS_TABLE)?;
            table.insert(key, val.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    async fn get_block(&self, hash: H256) -> Result<Option<FlashBlock>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(BLOCKS_TABLE)?;
        let val = table.get(hash.as_fixed_bytes())?;

        if let Some(bytes) = val {
            Ok(Some(serde_json::from_slice(bytes.value())?))
        } else {
            Ok(None)
        }
    }

    async fn get_latest_block(&self) -> Result<Option<FlashBlock>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(BLOCKS_TABLE)?;
        let mut iter = table.iter()?;

        // redb doesn't have an easy "last" for random keys, we'd need a secondary index
        // For now, we take the one with the highest block number if we had one,
        // but since hashes are random, we just return the last from iterator as a placeholder
        if let Some(res) = iter.next_back() {
            let (_key, val) = res?;
            Ok(Some(serde_json::from_slice(val.value())?))
        } else {
            Ok(None)
        }
    }

    async fn get_recent_blocks(&self, limit: usize) -> Result<Vec<FlashBlock>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(BLOCKS_TABLE)?;
        let iter = table.iter()?;

        let mut results = Vec::new();
        for item in iter.take(limit) {
            let (_key, value) = item?;
            results.push(serde_json::from_slice(value.value())?);
        }
        Ok(results)
    }

    async fn save_reorg(&self, event: ReorgEvent) -> Result<()> {
        let key = event.block_number.as_u64();
        let val = serde_json::to_vec(&event)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(REORGS_TABLE)?;
            table.insert(key, val.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    async fn get_latest_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(REORGS_TABLE)?;
        let iter = table.iter()?;

        let mut results = Vec::new();
        for item in iter.rev().take(limit) {
            let (_key, value) = item?;
            results.push(serde_json::from_slice(value.value())?);
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

    async fn get_sequencer_stats(&self, address: Address) -> Result<Option<SequencerStats>> {
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
