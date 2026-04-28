use ethers::types::{H256, U256, Bytes};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashBlock {
    pub number: U256,
    pub hash: H256,
    pub parent_hash: H256,
    pub timestamp: DateTime<Utc>,
    pub sequencer_signature: Option<Bytes>,
    pub confidence: f64,
    pub status: BlockStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockStatus {
    Pending,
    Stable,
    Finalized,
    Reorged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgEvent {
    pub block_number: U256,
    pub old_hash: H256,
    pub new_hash: H256,
    pub detected_at: DateTime<Utc>,
    pub severity: ReorgSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReorgSeverity {
    Soft,   // Sub-block hash change (equivocation)
    Deep,   // Multiple blocks replaced
}
