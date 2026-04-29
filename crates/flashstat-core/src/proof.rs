use ethers::utils::rlp::{Encodable, RlpStream};
use ethers::types::{Address, Bytes, H256, U256};
use flashstat_common::DoubleSpendProof;

pub struct DoubleSpendProofRLP {
    pub tx_hash_1: H256,
    pub tx_hash_2: H256,
    pub sender: Address,
    pub nonce: U256,
}

impl Encodable for DoubleSpendProofRLP {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(4);
        s.append(&self.tx_hash_1);
        s.append(&self.tx_hash_2);
        s.append(&self.sender);
        s.append(&self.nonce);
    }
}

pub struct EquivocationProofRLP {
    pub block_number: U256,
    pub signer: Address,
    pub signature_1: Bytes,
    pub signature_2: Bytes,
    pub block_hash_1: H256,
    pub block_hash_2: H256,
}

impl Encodable for EquivocationProofRLP {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(6);
        s.append(&self.block_number);
        s.append(&self.signer);
        s.append(&self.signature_1.as_ref());
        s.append(&self.signature_2.as_ref());
        s.append(&self.block_hash_1);
        s.append(&self.block_hash_2);
    }
}

pub fn encode_double_spend_proof(proof: DoubleSpendProof) -> Vec<u8> {
    let rlp_proof = DoubleSpendProofRLP {
        tx_hash_1: proof.tx_hash_1,
        tx_hash_2: proof.tx_hash_2,
        sender: proof.sender,
        nonce: proof.nonce,
    };
    let mut s = RlpStream::new();
    rlp_proof.rlp_append(&mut s);
    s.out().to_vec()
}

pub fn encode_equivocation_proof(
    number: U256,
    signer: Address,
    sig1: Bytes,
    sig2: Bytes,
    hash1: H256,
    hash2: H256,
) -> Vec<u8> {
    let proof = EquivocationProofRLP {
        block_number: number,
        signer,
        signature_1: sig1,
        signature_2: sig2,
        block_hash_1: hash1,
        block_hash_2: hash2,
    };
    let mut s = RlpStream::new();
    proof.rlp_append(&mut s);
    s.out().to_vec()
}
