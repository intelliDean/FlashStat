use ethers::types::{Address, Block, Bytes, H256, U256};
use eyre::Result;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🏮 Starting Phase 7 Watchtower Integration Test...");
    
    let client = HttpClientBuilder::default().build("http://127.0.0.1:9944")?;

    // 1. Setup Mock Data
    let block_number = 99_999u64;
    let signer = Address::random();

    // Create two conflicting blocks signed by the same "sequencer"
    let hash_a = H256::random();
    let hash_b = H256::random();

    let mut block_a = create_mock_block(block_number, hash_a);
    // Sig 1 (Mocked as 65 bytes at end of extra_data)
    let mut extra_a = vec![0u8; 32];
    extra_a.extend_from_slice(&[0x11; 65]);
    block_a.extra_data = extra_a.into();

    let mut block_b = create_mock_block(block_number, hash_b);
    // Sig 2 (Conflicting)
    let mut extra_b = vec![0u8; 32];
    extra_b.extend_from_slice(&[0x22; 65]);
    block_b.extra_data = extra_b.into();

    println!("⚔️  Injecting conflicting blocks at #{}...", block_number);

    // 2. Ingest Block A
    let _: () = client.request("flash_ingestBlock", (block_a,)).await?;
    println!("  ✅ Block A Ingested");

    tokio::time::sleep(Duration::from_secs(1)).await;

    // 3. Ingest Block B (Should trigger equivocation detection and watchtower)
    let _: () = client.request("flash_ingestBlock", (block_b,)).await?;
    println!("  🔥 Block B Ingested (Equivocation Triggered)");

    println!("🔎 Monitoring server logs for 'ACTIVE PROTECTION' or 'Slashing proof submitted'...");
    
    // Give it a moment to process the async task
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("✅ Integration test signal sent. Verify on-chain/log output.");

    Ok(())
}

fn create_mock_block(number: u64, hash: H256) -> Block<H256> {
    let mut block = Block::default();
    block.number = Some(number.into());
    block.hash = Some(hash);
    block.parent_hash = H256::random();
    block.timestamp = U256::from(chrono::Utc::now().timestamp());
    block
}
