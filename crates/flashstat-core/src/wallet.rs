use ethers::prelude::*;
use eyre::{Result, eyre};
use std::sync::Arc;
use flashstat_common::GuardianConfig;
use std::str::FromStr;

abigen!(
    SlashingManager,
    r#"[
        function submitEquivocationProof(bytes calldata proof) external
        function submitDoubleSpendProof(bytes calldata proof) external
    ]"#
);

pub struct GuardianWallet {
    client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    contract: SlashingManager<SignerMiddleware<Provider<Http>, LocalWallet>>,
}

impl GuardianWallet {
    pub async fn new(config: &GuardianConfig, http_url: &str) -> Result<Self> {
        let provider = Provider::<Http>::try_from(http_url)?;
        let chain_id = provider.get_chainid().await?.as_u64();

        let wallet = if let Some(pk) = &config.private_key {
            pk.parse::<LocalWallet>()?.with_chain_id(chain_id)
        } else if let Some(_path) = &config.keystore_path {
            // Placeholder for keystore logic - would require password prompt
            return Err(eyre!("Keystore support requires interactive password. Use private_key for now."));
        } else {
            return Err(eyre!("No guardian wallet configured"));
        };

        let client = Arc::new(SignerMiddleware::new(provider, wallet));
        let contract = SlashingManager::new(config.slashing_contract, client.clone());

        Ok(Self { client, contract })
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
