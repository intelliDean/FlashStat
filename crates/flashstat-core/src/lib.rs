pub mod monitor;
pub mod proof;
pub mod tee;
pub mod wallet;

pub use monitor::FlashMonitor;

#[cfg(test)]
mod tests {
    use super::monitor::*;
    use ethers::prelude::*;
    use eyre::Result;
    use flashstat_common::*;
    use flashstat_db::{FlashStorage, RedbStorage};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_reputation_scoring() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("test.db");
        let storage = Arc::new(RedbStorage::new(db_path.to_str().unwrap())?);

        // Mock config
        let config = Config {
            rpc: RpcConfig {
                ws_url: "http://localhost:8545".into(),
                http_url: "http://localhost:8545".into(),
            },
            storage: StorageConfig {
                db_path: db_path.to_str().unwrap().into(),
            },
            tee: TeeConfig {
                sequencer_address: Address::random(),
                attestation_enabled: false,
                expected_mrenclave: None,
            },
            guardian: GuardianConfig {
                private_key: None,
                keystore_path: None,
                slashing_contract: Address::random(),
            },
        };

        let (_tx, rx) = broadcast::channel(1);
        let monitor = FlashMonitor::new(config, storage.clone(), rx).await?;

        let address = Address::random();

        // 1. Reward: 100 blocks + attested
        monitor.update_reputation(address, 100, 0, 0, true).await?;
        let stats = storage.get_sequencer_stats(address).await?.unwrap();
        // Base(100) + Attestation(100) + Streak(10) = 210
        assert_eq!(stats.reputation_score, 210);
        assert_eq!(stats.current_streak, 100);

        // 2. Penalty: Equivocation
        monitor.update_reputation(address, 0, 0, 1, false).await?;
        let stats = storage.get_sequencer_stats(address).await?.unwrap();
        // Base(100) + Attest(100) + Streak(0) - 1000 = -800
        assert_eq!(stats.reputation_score, -800);
        assert_eq!(stats.current_streak, 0);

        Ok(())
    }

    #[test]
    fn test_proof_serialization() {
        use crate::proof;
        let ds_proof = DoubleSpendProof {
            tx_hash_1: H256::random(),
            tx_hash_2: H256::random(),
            sender: Address::random(),
            nonce: U256::from(42),
        };

        let rlp_bytes = proof::encode_double_spend_proof(ds_proof);
        assert!(!rlp_bytes.is_empty());
    }
}
