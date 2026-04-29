use flashstat_common::{FlashBlock, ReorgEvent};
use eyre::Result;
use async_trait::async_trait;
use ethers::types::{H256, U256};
use rocksdb::{DB, Options};
use std::sync::Arc;
use serde_json;

#[async_trait]
pub trait FlashStorage: Send + Sync {
    async fn save_block(&self, block: FlashBlock) -> Result<()>;
    async fn get_block(&self, hash: H256) -> Result<Option<FlashBlock>>;
    async fn save_reorg(&self, event: ReorgEvent) -> Result<()>;
    async fn get_latest_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>>;
    async fn get_equivocations(&self, limit: usize) -> Result<Vec<ReorgEvent>>;
    async fn get_latest_block(&self) -> Result<Option<FlashBlock>>;
}

pub struct RocksStorage {
    db: Arc<DB>,
}

impl RocksStorage {
    pub fn new(path: &str) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, path)?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn new_readonly(path: &str) -> Result<Self> {
        let mut opts = Options::default();
        let db = DB::open_for_read_only(&opts, path, false)?;
        Ok(Self { db: Arc::new(db) })
    }
}

#[async_trait]
impl FlashStorage for RocksStorage {
    async fn save_block(&self, block: FlashBlock) -> Result<()> {
        let key = format!("block:{}", block.hash);
        let val = serde_json::to_vec(&block)?;
        self.db.put(key, val)?;
        Ok(())
    }

    async fn get_block(&self, hash: H256) -> Result<Option<FlashBlock>> {
        let key = format!("block:{}", hash);
        if let Some(bytes) = self.db.get(key)? {
            let block = serde_json::from_slice(&bytes)?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    async fn save_reorg(&self, event: ReorgEvent) -> Result<()> {
        let desc_ts = u64::MAX - event.detected_at.timestamp_nanos() as u64;
        let key = format!("reorg:{:020}:{}", desc_ts, event.block_number);
        let val = serde_json.to_vec(&event)?;
        self.db.put(key, val)?;
        Ok(())
    }

    async fn get_latest_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>> {
        use rocksdb::IteratorMode;
        
        let mut results = Vec::new();
        let prefix = "reorg:";
        let iter = self.db.iterator(IteratorMode::From(prefix.as_bytes(), rocksdb::Direction::Forward));

        for item in iter {
            let (key, value) = item?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            let event: ReorgEvent = serde_json::from_slice(&value)?;
            results.push(event);
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    async fn get_equivocations(&self, limit: usize) -> Result<Vec<ReorgEvent>> {
        use rocksdb::IteratorMode;
        use flashstat_common::ReorgSeverity;
        
        let mut results = Vec::new();
        let prefix = "reorg:";
        let iter = self.db.iterator(IteratorMode::From(prefix.as_bytes(), rocksdb::Direction::Forward));

        for item in iter {
            let (key, value) = item?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            let event: ReorgEvent = serde_json::from_slice(&value)?;
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
        use rocksdb::IteratorMode;
        let prefix = "block:";
        // Blocks are keyed by hash, which doesn't sort by number.
        // In a real implementation, we would keep a 'latest' key.
        // For now, we'll return the first one found or None.
        let mut iter = self.db.iterator(IteratorMode::Start);
        if let Some(item) = iter.next() {
            let (key, value) = item?;
            if key.starts_with(prefix.as_bytes()) {
                return Ok(Some(serde_json::from_slice(&value)?));
            }
        }
        Ok(None)
    }
}
