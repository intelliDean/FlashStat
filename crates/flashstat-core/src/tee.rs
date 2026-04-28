use ethers::prelude::*;
use eyre::{Result, eyre};
use tracing::debug;

pub struct TeeVerifier {
    pub expected_sequencer: Address,
}

impl TeeVerifier {
    pub fn new(expected_sequencer: Address) -> Self {
        Self { expected_sequencer }
    }

    /// Verifies the sequencer signature against the block hash.
    /// The signature should recover to the expected sequencer address.
    pub fn verify_sequencer_signature(&self, block_hash: H256, signature_bytes: &[u8]) -> Result<bool> {
        if signature_bytes.len() != 65 {
            return Err(eyre!("Invalid signature length: expected 65 bytes, got {}", signature_bytes.len()));
        }

        // Parse signature from bytes
        let signature = Signature::try_from(signature_bytes)?;
        
        // Recover the address from the block hash
        // We use the raw block hash without Ethereum signed message prefix
        // since this is a protocol-level signature from the sequencer.
        let recovered_address = signature.recover(block_hash)?;

        let is_valid = recovered_address == self.expected_sequencer;
        
        debug!(
            "TEE Verification | Expected: {:?} | Recovered: {:?} | Valid: {}",
            self.expected_sequencer, recovered_address, is_valid
        );

        Ok(is_valid)
    }
}
