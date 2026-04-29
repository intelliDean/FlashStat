use ethers::prelude::*;
use eyre::{eyre, Result};
use flashstat_common::GuardianConfig;
use std::sync::Arc;

abigen!(
    SlashingManager,
    r#"[
        function submitEquivocationProof(bytes calldata proof) external
        function submitDoubleSpendProof(bytes calldata proof) external
    ]"#
);

pub struct GuardianWallet {
    contract: SlashingManager<SignerMiddleware<Provider<Http>, LocalWallet>>,
}

impl GuardianWallet {
    pub async fn new(config: &GuardianConfig, http_url: &str) -> Result<Self> {
        let provider = Provider::<Http>::try_from(http_url)?;
        let chain_id = provider.get_chainid().await?.as_u64();

        let wallet = if let Some(pk) = &config.private_key {
            pk.parse::<LocalWallet>()?.with_chain_id(chain_id)
        } else if let Some(path) = &config.keystore_path {
            let password = std::env::var("FLASHSTAT__GUARDIAN__PASSWORD").map_err(|_| {
                eyre!("Keystore configured but FLASHSTAT__GUARDIAN__PASSWORD not set")
            })?;
            LocalWallet::decrypt_keystore(path, password)?.with_chain_id(chain_id)
        } else {
            return Err(eyre!("No guardian wallet configured"));
        };

        let client = Arc::new(SignerMiddleware::new(provider, wallet));
        let contract = SlashingManager::new(config.slashing_contract, client);

        Ok(Self { contract })
    }

    pub async fn submit_equivocation_proof(&self, proof_bytes: Vec<u8>) -> Result<H256> {
        let tx = self.contract.submit_equivocation_proof(proof_bytes.into());
        let pending_tx = tx.send().await?;
        Ok(pending_tx.tx_hash())
    }

    pub async fn submit_double_spend_proof(&self, proof_bytes: Vec<u8>) -> Result<H256> {
        let tx = self.contract.submit_double_spend_proof(proof_bytes.into());
        let pending_tx = tx.send().await?;
        Ok(pending_tx.tx_hash())
    }
}
