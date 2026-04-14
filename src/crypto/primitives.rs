use ed25519_dalek::{
    Signature, Signer, SigningKey, Verifier, VerifyingKey, SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};
use pqcrypto_dilithium::dilithium5;
use pqcrypto_traits::sign::{
    DetachedSignature as PqDetachedSignatureTrait, PublicKey as PqPublicKeyTrait,
    SecretKey as PqSecretKeyTrait,
};
use rand::RngCore;
use sha3::{Digest, Sha3_256};
use std::io::{Read, Write};
use std::path::Path;
#[derive(Debug)]
pub enum CryptoError {
    KeyGeneration(String),
    Signing(String),
    Verification(String),
    Io(String),
    InvalidKey(String),
}
impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::KeyGeneration(s) => write!(f, "Key generation error: {}", s),
            CryptoError::Signing(s) => write!(f, "Signing error: {}", s),
            CryptoError::Verification(s) => write!(f, "Verification error: {}", s),
            CryptoError::Io(s) => write!(f, "I/O error: {}", s),
            CryptoError::InvalidKey(s) => write!(f, "Invalid key: {}", s),
        }
    }
}
impl std::error::Error for CryptoError {}
#[derive(Clone)]
pub struct KeyPair {
    signing_key: SigningKey,
}

use schnorrkel::Keypair as SchnorrkelKeypair;

#[derive(Clone)]
pub struct ValidatorKeys {
    pub sig_key: KeyPair,
    pub vrf_key: SchnorrkelKeypair,
    pub pq_key: Option<PqKeyPair>,
}

#[derive(Clone)]
pub struct PqKeyPair {
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
}

impl PqKeyPair {
    pub fn generate() -> Self {
        let (public_key, secret_key) = dilithium5::keypair();
        PqKeyPair {
            public_key: public_key.as_bytes().to_vec(),
            secret_key: secret_key.as_bytes().to_vec(),
        }
    }

    pub fn from_bytes(public_key: &[u8], secret_key: &[u8]) -> Result<Self, CryptoError> {
        if public_key.len() != dilithium5::public_key_bytes() {
            return Err(CryptoError::InvalidKey(format!(
                "Invalid Dilithium public key length: expected {}, got {}",
                dilithium5::public_key_bytes(),
                public_key.len()
            )));
        }
        if secret_key.len() != dilithium5::secret_key_bytes() {
            return Err(CryptoError::InvalidKey(format!(
                "Invalid Dilithium secret key length: expected {}, got {}",
                dilithium5::secret_key_bytes(),
                secret_key.len()
            )));
        }
        Ok(Self {
            public_key: public_key.to_vec(),
            secret_key: secret_key.to_vec(),
        })
    }

    pub fn public_key_bytes(&self) -> &[u8] {
        &self.public_key
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let secret_key = dilithium5::SecretKey::from_bytes(&self.secret_key)
            .map_err(|e| CryptoError::Signing(e.to_string()))?;
        Ok(dilithium5::detached_sign(message, &secret_key)
            .as_bytes()
            .to_vec())
    }

    pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        let public_key = dilithium5::PublicKey::from_bytes(public_key)
            .map_err(|e| CryptoError::Verification(e.to_string()))?;
        let signature = dilithium5::DetachedSignature::from_bytes(signature)
            .map_err(|e| CryptoError::Verification(e.to_string()))?;
        dilithium5::verify_detached_signature(&signature, message, &public_key)
            .map_err(|e| CryptoError::Verification(e.to_string()))
    }
}

impl ValidatorKeys {
    pub fn generate() -> Result<Self, CryptoError> {
        let sig_key = KeyPair::generate()?;
        let mut csprng = rand_core::OsRng;
        let vrf_key = SchnorrkelKeypair::generate_with(&mut csprng);
        let pq_key = Some(PqKeyPair::generate());
        Ok(ValidatorKeys {
            sig_key,
            vrf_key,
            pq_key,
        })
    }
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), CryptoError> {
        let mut bytes = self.sig_key.signing_key.as_bytes().to_vec();
        bytes.extend_from_slice(&self.vrf_key.to_bytes());
        if let Some(pq_key) = &self.pq_key {
            bytes.extend_from_slice(pq_key.public_key_bytes());
            bytes.extend_from_slice(&pq_key.secret_key);
        }
        std::fs::write(path.as_ref(), bytes).map_err(|e| CryptoError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(path.as_ref(), perms);
        }
        Ok(())
    }
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, CryptoError> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| CryptoError::Io(e.to_string()))?;
        if bytes.len() < 128 {
            return Err(CryptoError::InvalidKey("Key file too short".into()));
        }
        let sig_key = KeyPair::from_bytes(&bytes[0..32])?;
        let vrf_key = SchnorrkelKeypair::from_bytes(&bytes[32..128])
            .map_err(|e| CryptoError::InvalidKey(e.to_string()))?;

        let pq_key = if bytes.len() == 128 {
            None
        } else {
            let expected_len =
                128 + dilithium5::public_key_bytes() + dilithium5::secret_key_bytes();
            if bytes.len() != expected_len {
                return Err(CryptoError::InvalidKey(format!(
                    "Invalid validator key file length: expected {} or {}, got {}",
                    128,
                    expected_len,
                    bytes.len()
                )));
            }
            let pq_pk_end = 128 + dilithium5::public_key_bytes();
            Some(PqKeyPair::from_bytes(
                &bytes[128..pq_pk_end],
                &bytes[pq_pk_end..expected_len],
            )?)
        };

        Ok(ValidatorKeys {
            sig_key,
            vrf_key,
            pq_key,
        })
    }
}
impl KeyPair {
    pub fn generate() -> Result<Self, CryptoError> {
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        rand::rng().fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        println!("New keypair generated");
        println!(
            "   Public key: {}",
            hex::encode(signing_key.verifying_key().as_bytes())
        );
        Ok(KeyPair { signing_key })
    }
    pub fn from_seed(seed: &[u8; SECRET_KEY_LENGTH]) -> Result<Self, CryptoError> {
        let signing_key = SigningKey::from_bytes(seed);
        Ok(KeyPair { signing_key })
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != SECRET_KEY_LENGTH {
            return Err(CryptoError::InvalidKey(format!(
                "Expected {} bytes, got {}",
                SECRET_KEY_LENGTH,
                bytes.len()
            )));
        }
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        seed.copy_from_slice(bytes);
        Self::from_seed(&seed)
    }
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, CryptoError> {
        let mut file =
            std::fs::File::open(path.as_ref()).map_err(|e| CryptoError::Io(e.to_string()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| CryptoError::Io(e.to_string()))?;
        Self::from_bytes(&bytes)
    }
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), CryptoError> {
        let mut file =
            std::fs::File::create(path.as_ref()).map_err(|e| CryptoError::Io(e.to_string()))?;
        file.write_all(self.signing_key.as_bytes())
            .map_err(|e| CryptoError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path.as_ref(), perms)
                .map_err(|e| CryptoError::Io(e.to_string()))?;
        }
        println!("Keypair saved to {:?}", path.as_ref());
        Ok(())
    }
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LENGTH] {
        let signature = self.signing_key.sign(message);
        signature.to_bytes()
    }
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        verify_signature(message, signature, &self.public_key_bytes())
    }
}
pub fn verify_signature(
    message: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> Result<(), CryptoError> {
    if signature.len() != SIGNATURE_LENGTH {
        return Err(CryptoError::Verification(format!(
            "Invalid signature length: expected {}, got {}",
            SIGNATURE_LENGTH,
            signature.len()
        )));
    }
    let sig_bytes: [u8; SIGNATURE_LENGTH] = signature
        .try_into()
        .map_err(|_| CryptoError::Verification("Invalid signature format".into()))?;
    let sig = Signature::from_bytes(&sig_bytes);
    if public_key.len() != 32 {
        return Err(CryptoError::Verification(format!(
            "Invalid public key length: expected 32, got {}",
            public_key.len()
        )));
    }
    let pk_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| CryptoError::Verification("Invalid public key format".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| CryptoError::Verification(e.to_string()))?;
    verifying_key
        .verify(message, &sig)
        .map_err(|e| CryptoError::Verification(e.to_string()))
}
pub fn hash_message(message: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    hasher.update(message);
    hasher.finalize().into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_keypair_generation() {
        let kp = KeyPair::generate().unwrap();
        assert_eq!(kp.public_key_bytes().len(), 32);
    }
    #[test]
    fn test_sign_and_verify() {
        let kp = KeyPair::generate().unwrap();
        let message = b"Hello, Budlum!";
        let signature = kp.sign(message);
        assert_eq!(signature.len(), 64);
        assert!(kp.verify(message, &signature).is_ok());
        assert!(kp.verify(b"Wrong message", &signature).is_err());
    }
    #[test]
    fn test_deterministic_signature() {
        let seed = [0u8; 32];
        let kp1 = KeyPair::from_seed(&seed).unwrap();
        let kp2 = KeyPair::from_seed(&seed).unwrap();
        let message = b"test";
        let sig1 = kp1.sign(message);
        let sig2 = kp2.sign(message);
        assert_eq!(sig1, sig2);
    }
    #[test]
    fn test_standalone_verification() {
        let kp = KeyPair::generate().unwrap();
        let message = b"Standalone test";
        let signature = kp.sign(message);
        assert!(verify_signature(message, &signature, &kp.public_key_bytes()).is_ok());
    }
    #[test]
    fn test_invalid_signature_length() {
        let kp = KeyPair::generate().unwrap();
        let message = b"test";
        let bad_sig = [0u8; 32];
        assert!(kp.verify(message, &bad_sig).is_err());
    }
    #[test]
    fn test_save_and_load() {
        let kp = KeyPair::generate().unwrap();
        let path = "/tmp/test_budlum_key";
        kp.save(path).unwrap();
        let loaded = KeyPair::load(path).unwrap();
        assert_eq!(kp.public_key_bytes(), loaded.public_key_bytes());
        let msg = b"test";
        assert_eq!(kp.sign(msg), loaded.sign(msg));
        std::fs::remove_file(path).ok();
    }
}
