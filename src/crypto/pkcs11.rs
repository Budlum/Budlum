use crate::crypto::primitives::CryptoError;
use crate::crypto::signer::ConsensusSigner;
use std::sync::Mutex;

pub struct Pkcs11Signer {
    #[allow(dead_code)]
    module_path: String,
    #[allow(dead_code)]
    slot_id: u64,
    #[allow(dead_code)]
    token_pin_env: String,
    public_key_bytes: [u8; 32],
    inner: Mutex<Option<Pkcs11Inner>>,
}

struct Pkcs11Inner {
    #[allow(dead_code)]
    pkcs11_client: cryptoki::context::Pkcs11,
    session: cryptoki::session::Session,
}

impl Pkcs11Signer {
    pub fn new(
        module_path: String,
        slot_id: u64,
        token_pin_env: String,
    ) -> Result<Self, CryptoError> {
        let pin = std::env::var(&token_pin_env).map_err(|e| {
            CryptoError::KeyGeneration(format!(
                "PKCS#11 PIN environment variable '{}' not accessible: {}",
                token_pin_env, e
            ))
        })?;
        if pin.is_empty() {
            return Err(CryptoError::KeyGeneration(
                "PKCS#11 PIN is empty".to_string(),
            ));
        }

        let pkcs11_client = cryptoki::context::Pkcs11::new(&module_path).map_err(|e| {
            CryptoError::KeyGeneration(format!(
                "Failed to load PKCS#11 module '{}': {}",
                module_path, e
            ))
        })?;

        pkcs11_client
            .initialize(cryptoki::context::CInitializeArgs::OsThreads)
            .map_err(|e| {
                CryptoError::KeyGeneration(format!("Failed to initialize PKCS#11: {}", e))
            })?;

        let slots = pkcs11_client.get_slots_with_token().map_err(|e| {
            CryptoError::KeyGeneration(format!("Failed to enumerate PKCS#11 slots: {}", e))
        })?;

        let target_slot = slots
            .iter()
            .find(|s: &&cryptoki::slot::Slot| s.id() == slot_id)
            .ok_or_else(|| {
                CryptoError::KeyGeneration(format!(
                    "Slot {} not found (available slots with tokens: {:?})",
                    slot_id,
                    slots
                        .iter()
                        .map(|s: &cryptoki::slot::Slot| s.id())
                        .collect::<Vec<_>>()
                ))
            })?;

        let session = pkcs11_client.open_ro_session(*target_slot).map_err(|e| {
            CryptoError::KeyGeneration(format!(
                "Failed to open RO session on slot {}: {}",
                slot_id, e
            ))
        })?;

        let pin_secret = secrecy::Secret::new(pin);
        session
            .login(cryptoki::session::UserType::User, Some(&pin_secret))
            .map_err(|e| CryptoError::KeyGeneration(format!("PKCS#11 login failed: {}", e)))?;

        let public_key_bytes = Self::extract_ed25519_public_key(&session).map_err(|e| {
            CryptoError::KeyGeneration(format!(
                "Failed to extract Ed25519 public key from HSM: {}",
                e
            ))
        })?;

        Ok(Self {
            module_path,
            slot_id,
            token_pin_env,
            public_key_bytes,
            inner: Mutex::new(Some(Pkcs11Inner {
                pkcs11_client,
                session,
            })),
        })
    }

    fn extract_ed25519_public_key(
        session: &cryptoki::session::Session,
    ) -> Result<[u8; 32], String> {
        let template = &[
            cryptoki::object::Attribute::Class(cryptoki::object::ObjectClass::PUBLIC_KEY),
            cryptoki::object::Attribute::KeyType(cryptoki::object::KeyType::EC_EDWARDS),
        ];
        let objects = session
            .find_objects(template)
            .map_err(|e| format!("Failed to search for Ed25519 key: {}", e))?;
        if objects.is_empty() {
            return Err("No Ed25519 public key found in HSM slot".to_string());
        }
        let attr = session
            .get_attributes(objects[0], &[cryptoki::object::AttributeType::Value])
            .map_err(|e| format!("Failed to read public key value: {}", e))?;
        if let Some(cryptoki::object::Attribute::Value(value)) = attr.first() {
            if value.len() >= 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&value[..32]);
                return Ok(key);
            }
        }
        Err("Failed to extract public key bytes".to_string())
    }
}

impl ConsensusSigner for Pkcs11Signer {
    fn public_key_bytes(&self) -> [u8; 32] {
        self.public_key_bytes
    }

    fn sign_block(&self, block_hash: &[u8; 32]) -> Result<Vec<u8>, CryptoError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| CryptoError::Signing("PKCS#11 inner mutex poisoned".to_string()))?;
        let inner = guard
            .as_ref()
            .ok_or_else(|| CryptoError::Signing("PKCS#11 session already closed".to_string()))?;

        let template = &[
            cryptoki::object::Attribute::Class(cryptoki::object::ObjectClass::PRIVATE_KEY),
            cryptoki::object::Attribute::KeyType(cryptoki::object::KeyType::EC_EDWARDS),
        ];
        let objects = inner.session.find_objects(template).map_err(|e| {
            CryptoError::Signing(format!("Failed to find Ed25519 private key: {}", e))
        })?;
        if objects.is_empty() {
            return Err(CryptoError::Signing(
                "No Ed25519 private key found in HSM slot".to_string(),
            ));
        }
        let key_handle = objects[0];

        let mechanism = cryptoki::mechanism::Mechanism::Eddsa;
        let signature = inner
            .session
            .sign(&mechanism, key_handle, block_hash)
            .map_err(|e| CryptoError::Signing(format!("HSM sign operation failed: {}", e)))?;

        if signature.len() < 64 {
            return Err(CryptoError::Signing(format!(
                "HSM returned undersized signature: {} bytes",
                signature.len()
            )));
        }
        Ok(signature[..64].to_vec())
    }

    fn backend_name(&self) -> &'static str {
        "pkcs11"
    }
}
