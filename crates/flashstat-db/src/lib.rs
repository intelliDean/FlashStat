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
        let key = format!("reorg:{}:{}", event.block_number, event.detected_at.timestamp_nanos());
        let val = serde_json.to_vec(&event)?;
        self.db.put(key, val)?;
        Ok(())
    }

    async fn get_latest_reorgs(&self, _limit: usize) -> Result<Vec<ReorgEvent>> {
        // Implementation for prefix iteration in RocksDB
        // For brevity, returning empty for now
        Ok(vec![])
    }
}
