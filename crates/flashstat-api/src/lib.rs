use ethers::types::H256;
use flashstat_api::FlashApiServer;
use flashstat_common::FlashBlock;
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;

#[rpc(server, client, namespace = "flash")]
pub trait FlashApi {
    #[method(name = "getConfidence")]
    async fn get_confidence(&self, hash: H256) -> RpcResult<f64>;

    #[method(name = "getLatestBlock")]
    async fn get_latest_block(&self) -> RpcResult<Option<FlashBlock>>;

    #[method(name = "getRecentBlocks")]
    async fn get_recent_blocks(&self, limit: usize) -> RpcResult<Vec<FlashBlock>>;

    #[method(name = "getRecentReorgs")]
    async fn get_recent_reorgs(&self, limit: usize) -> RpcResult<Vec<flashstat_common::ReorgEvent>>;

    #[method(name = "getEquivocations")]
    async fn get_equivocations(&self, limit: usize) -> RpcResult<Vec<flashstat_common::ReorgEvent>>;

    #[method(name = "getHealth")]
    async fn get_health(&self) -> RpcResult<flashstat_common::SystemHealth>;

    #[method(name = "getSequencerRankings")]
    async fn get_sequencer_rankings(&self) -> RpcResult<Vec<flashstat_common::SequencerStats>>;

    #[method(name = "ingestBlock")]
    async fn ingest_block(&self, block: ethers::types::Block<ethers::types::H256>) -> RpcResult<()>;

    #[subscription(name = "subscribeBlocks", item = FlashBlock)]
    async fn subscribe_blocks(&self) -> jsonrpsee::core::SubscriptionResult;

    #[subscription(name = "subscribeEvents", item = flashstat_common::ReorgEvent)]
    async fn subscribe_events(&self) -> jsonrpsee::core::SubscriptionResult;
}
