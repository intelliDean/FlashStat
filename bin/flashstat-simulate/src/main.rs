use clap::Parser;
use ethers::types::{Address, Bytes, H256, U256};
use eyre::Result;
use flashstat_common::{
    ConflictAnalysis, DoubleSpendProof, EquivocationEvent, ReorgEvent, ReorgSeverity,
};
use flashstat_db::{FlashStorage, RedbStorage};
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(author, version, about = "🏮 FlashStat Forensic Simulation Tool", long_about = None)]
struct Args {
    #[arg(short, long, default_value = "flashstat.db")]
    db_path: String,

    #[arg(short, long, default_value_t = 1)]
    count: usize,

    #[arg(short, long, default_value = "equivocation")]
    severity: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let storage: Arc<dyn FlashStorage> = Arc::new(RedbStorage::new(&args.db_path)?);

    println!(
        "🏮 Injecting {} synthetic {} events into {}...",
        args.count, args.severity, args.db_path
    );

    for i in 0..args.count {
        let block_number = 50_000_000 + i as u64;
        let severity = match args.severity.as_str() {
            "soft" => ReorgSeverity::Soft,
            "deep" => ReorgSeverity::Deep,
            _ => ReorgSeverity::Equivocation,
        };

        let event = if severity == ReorgSeverity::Equivocation {
            // Create a detailed equivocation with double-spend data
            let ds_tx = DoubleSpendProof {
                tx_hash_1: H256::random(),
                tx_hash_2: H256::random(),
                sender: Address::random(),
                nonce: U256::from(1),
            };

            let conflict = ConflictAnalysis {
                dropped_txs: vec![H256::random(), H256::random()],
                double_spend_txs: vec![ds_tx],
            };

            let equivocation = EquivocationEvent {
                signer: Address::random(),
                signature_1: Bytes::from(vec![1, 2, 3]),
                signature_2: Bytes::from(vec![4, 5, 6]),
                conflict_analysis: Some(conflict),
            };

            ReorgEvent {
                block_number: U256::from(block_number),
                old_hash: H256::random(),
                new_hash: H256::random(),
                detected_at: chrono::Utc::now(),
                severity,
                equivocation: Some(equivocation),
            }
        } else {
            ReorgEvent {
                block_number: U256::from(block_number),
                old_hash: H256::random(),
                new_hash: H256::random(),
                detected_at: chrono::Utc::now(),
                severity,
                equivocation: None,
            }
        };

        storage.save_reorg(event).await?;
        println!("  ✅ Injected alert at block #{}", block_number);
    }

    println!("🎉 Done!");
    Ok(())
}
