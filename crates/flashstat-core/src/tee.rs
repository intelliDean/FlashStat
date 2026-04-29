use ethers::prelude::*;
use eyre::{eyre, Result};
use hex;
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
            return Err(eyre!(
                "Invalid signature length: expected 65 bytes, got {}",
                signature_bytes.len()
            ));
        }
        let signature = Signature::try_from(signature_bytes)?;
        let recovered_address = signature.recover(block_hash)?;
        Ok(recovered_address)
    }

    /// Verifies the sequencer signature against the block hash.
    pub fn verify_sequencer_signature(
        &self,
        block_hash: H256,
        signature_bytes: &[u8],
    ) -> Result<bool> {
        let recovered_address = self.recover_signer(block_hash, signature_bytes)?;
        let is_valid = recovered_address == self.expected_sequencer;

        debug!(
            "TEE Verification | Expected: {:?} | Recovered: {:?} | Valid: {}",
            self.expected_sequencer, recovered_address, is_valid
        );

        Ok(is_valid)
    }

    /// Verifies the TEE attestation quote (e.g. Intel TDX).
    /// Performs basic structural validation for TDX Quote V4.
    pub fn verify_tdx_attestation(
        &self,
        quote: &[u8],
        expected_mrenclave: Option<&str>,
    ) -> Result<bool> {
        if quote.len() < 48 {
            return Err(eyre!("Quote too short for TDX V4: expected >= 48 bytes, got {}", quote.len()));
        }

        // TDX Quote V4 Header check
        let version = u16::from_le_bytes([quote[0], quote[1]]);
        let att_type = u16::from_le_bytes([quote[2], quote[3]]);
        
        if version != 4 {
            return Err(eyre!("Unsupported TDX Quote version: {}", version));
        }
        
        // Attestation Type 2 = TDX
        if att_type != 2 {
            debug!("Quote is not a TDX attestation type: {}", att_type);
            return Ok(false);
        }

        if let Some(expected) = expected_mrenclave {
            // TD Report starts at offset 48 in Quote V4.
            // MRENCLAVE is at offset 48 + 48 = 96 in the full Quote (32 bytes).
            if quote.len() < 128 {
                return Err(eyre!("Quote too short for TD Report extraction"));
            }
            
            let mrenclave_bytes = &quote[96..128];
            let actual_mrenclave = hex::encode(mrenclave_bytes);
            
            let is_match = actual_mrenclave == expected;
            debug!("TDX MRENCLAVE Check | Actual: {} | Expected: {} | Match: {}", 
                actual_mrenclave, expected, is_match);
            return Ok(is_match);
        }

        Ok(true)
    }
}

