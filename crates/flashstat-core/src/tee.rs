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

    /// Recovers the signer's address from the signature and block hash.
    pub fn recover_signer(&self, block_hash: H256, signature_bytes: &[u8]) -> Result<Address> {
        if signature_bytes.len() != 65 {
            return Err(eyre!("Invalid signature length: expected 65 bytes, got {}", signature_bytes.len()));
        }
        let signature = Signature::try_from(signature_bytes)?;
        let recovered_address = signature.recover(block_hash)?;
        Ok(recovered_address)
    }

    /// Verifies the sequencer signature against the block hash.
    pub fn verify_sequencer_signature(&self, block_hash: H256, signature_bytes: &[u8]) -> Result<bool> {
        let recovered_address = self.recover_signer(block_hash, signature_bytes)?;
        let is_valid = recovered_address == self.expected_sequencer;
        
        debug!(
            "TEE Verification | Expected: {:?} | Recovered: {:?} | Valid: {}",
            self.expected_sequencer, recovered_address, is_valid
        );

        Ok(is_valid)
    }

    /// Verifies the TEE attestation quote (e.g. Intel TDX).
    /// In a production environment, this would involve verifying the DCAP quote using a quote verification service.
    pub fn verify_tdx_attestation(&self, quote: &[u8], expected_mrenclave: Option<&str>) -> Result<bool> {
        // SIMULATION: In reality, we'd use a crate like `dcap-ql` or a remote verification service.
        // For the POC, we check if the quote is non-empty and optionally match the MRENCLAVE if provided.
        if quote.is_empty() {
            return Ok(false);
        }
        
        if let Some(expected) = expected_mrenclave {
            // Simulate extracting MRENCLAVE from quote bytes (usually at a specific offset)
            let simulated_mrenclave = hex::encode(&quote[0..min(quote.len(), 32)]);
            return Ok(simulated_mrenclave == expected);
        }

        Ok(true)
    }
}

fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}
