use ethers::types::{H256, U256, Bytes, Address};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use config::{Config as ConfigLoader, ConfigError, File};

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
    Soft,
    Deep,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub rpc: RpcConfig,
    pub storage: StorageConfig,
    pub tee: TeeConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TeeConfig {
    pub sequencer_address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcConfig {
    pub ws_url: String,
    pub http_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub db_path: String,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let s = ConfigLoader::builder()
            .add_source(File::with_name("flashstat").required(false))
            .add_source(config::Environment::with_prefix("FLASHSTAT").separator("__"))
            .build()?;

        s.try_deserialize()
    }
}
