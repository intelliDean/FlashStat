use clap::Parser;
use ethers::types::{Address, Block, H256, Transaction, U256};
use eyre::Result;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about = "🏮 FlashStat Forensic Simulation Tool", long_about = None)]
struct Args {
    #[arg(short, long, default_value = "http://127.0.0.1:9944")]
    url: String,

    #[arg(short, long, default_value_t = 1)]
    count: usize,

    #[arg(short, long, default_value = "equivocation")]
    severity: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = HttpClientBuilder::default().build(&args.url)?;

    println!(
        "🏮 Simulating {} {} event(s) via RPC at {}...",
        args.count, args.severity, args.url
    );

    for i in 0..args.count {
        let block_number = 60_000_000 + i as u64;
        let hash_1 = H256::random();
        let hash_2 = H256::random();

        if args.severity == "equivocation" {
            println!("⚔️  Simulating Equivocation at block #{}", block_number);

            // Block 1
            let mut block_1 = create_mock_block(block_number, hash_1);
            // Mock signature in extra_data (last 65 bytes)
            let mut extra_1 = vec![0u8; 32];
            extra_1.extend_from_slice(&[1u8; 65]);
            block_1.extra_data = extra_1.into();

            // Block 2 (conflicting)
            let mut block_2 = create_mock_block(block_number, hash_2);
            let mut extra_2 = vec![0u8; 32];
            extra_2.extend_from_slice(&[2u8; 65]);
            block_2.extra_data = extra_2.into();

            // Ingest both
            let _: () = client.request("flash_ingestBlock", (block_1,)).await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            let _: () = client.request("flash_ingestBlock", (block_2,)).await?;

        } else {
            println!("📦 Simulating Standard Block #{}", block_number);
            let block = create_mock_block(block_number, hash_1);
            let _: () = client.request("flash_ingestBlock", (block,)).await?;
        }

        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    println!("🎉 Simulation complete!");
    Ok(())
}

fn create_mock_block(number: u64, hash: H256) -> Block<H256> {
    let mut block = Block::default();
    block.number = Some(number.into());
    block.hash = Some(hash);
    block.parent_hash = H256::random();
    block.timestamp = U256::from(chrono::Utc::now().timestamp());
    
    // Add some mock transactions if needed
    let mut tx = Transaction::default();
    tx.hash = H256::random();
    tx.from = Address::random();
    tx.nonce = U256::from(0);
    // block.transactions.push(tx); // Block<H256> only has hashes by default

    block
}
