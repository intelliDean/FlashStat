use jsonrpsee::proc_macros::rpc;
use flashstat_common::{FlashBlock, ReorgEvent};
use ethers::types::H256;
use eyre::Result;

#[rpc(server, client, namespace = "flash")]
pub trait FlashApi {
    #[method(name = "getConfidence")]
    async fn get_confidence(&self, hash: H256) -> Result<f64, jsonrpsee::core::Error>;

    #[method(name = "getLatestBlock")]
    async fn get_latest_block(&self) -> Result<Option<FlashBlock>, jsonrpsee::core::Error>;

    #[method(name = "getRecentReorgs")]
    async fn get_recent_reorgs(&self, limit: usize) -> Result<Vec<ReorgEvent>, jsonrpsee::core::Error>;
}
