use crate::core::address::Address;
use crate::crypto::primitives::{CryptoError, KeyPair};

pub trait ConsensusSigner: Send + Sync {
    fn public_key_bytes(&self) -> [u8; 32];
    fn address(&self) -> Address {
        Address::from(self.public_key_bytes())
    }
    fn sign_block(&self, block_hash: &[u8; 32]) -> Result<Vec<u8>, CryptoError>;
    fn sign_prevote(&self, _msg: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.sign_block(&crate::core::hash::calculate_hash_bytes(_msg))
    }
    fn sign_precommit(&self, _msg: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.sign_block(&crate::core::hash::calculate_hash_bytes(_msg))
    }
    fn backend_name(&self) -> &'static str;
}

pub struct KeyPairSigner {
    keypair: KeyPair,
}

impl KeyPairSigner {
    pub fn new(keypair: KeyPair) -> Self {
        Self { keypair }
    }
}

impl ConsensusSigner for KeyPairSigner {
    fn public_key_bytes(&self) -> [u8; 32] {
        self.keypair.public_key_bytes()
    }

    fn sign_block(&self, block_hash: &[u8; 32]) -> Result<Vec<u8>, CryptoError> {
        Ok(self.keypair.sign(block_hash).to_vec())
    }

    fn backend_name(&self) -> &'static str {
        "local"
    }
}
