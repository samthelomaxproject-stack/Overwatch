/// Ed25519 signing and verification for TileUpdate batches.
///
/// Each node generates a keypair on first run. The device_id is derived
/// from the public key (SHA-256 → hex prefix). All outgoing TileUpdate
/// batches are signed; the hub verifies before merging.
///
/// Key rotation design: device_id is stable across rotations because it's
/// derived at registration time. Rotation is a Phase 5 concern — this
/// module deliberately leaves a rotation hook in [`DeviceKeys`].
use ed25519_dalek::{Signer, SigningKey, VerifyingKey, Signature, Verifier};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use crate::Error;

// ── Key management ────────────────────────────────────────────────────────────

/// Persistent device keypair. Stored in NodeDb (device_keys table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeys {
    /// Stable device identifier (first 16 hex chars of SHA-256(public_key))
    pub device_id: String,
    /// Base64-encoded 32-byte signing key seed
    pub secret_key_b64: String,
    /// Base64-encoded 32-byte verifying key
    pub public_key_b64: String,
    /// Unix timestamp of key creation
    pub created_at: u64,
    // Phase 5: add rotation_id, previous_device_id for key rotation
}

impl DeviceKeys {
    /// Generate a fresh Ed25519 keypair.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let pub_bytes = verifying_key.to_bytes();
        let device_id = derive_device_id(&pub_bytes);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            device_id,
            secret_key_b64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, signing_key.to_bytes()),
            public_key_b64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, pub_bytes),
            created_at: now,
        }
    }

    /// Load signing key from stored bytes.
    pub fn signing_key(&self) -> Result<SigningKey, Error> {
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &self.secret_key_b64)
            .map_err(|e| Error::Other(format!("base64 decode: {e}")))?;
        let arr: [u8; 32] = bytes.try_into()
            .map_err(|_| Error::Other("signing key must be 32 bytes".to_string()))?;
        Ok(SigningKey::from_bytes(&arr))
    }

    /// Load verifying key from stored bytes.
    pub fn verifying_key(&self) -> Result<VerifyingKey, Error> {
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &self.public_key_b64)
            .map_err(|e| Error::Other(format!("base64 decode: {e}")))?;
        let arr: [u8; 32] = bytes.try_into()
            .map_err(|_| Error::Other("verifying key must be 32 bytes".to_string()))?;
        VerifyingKey::from_bytes(&arr)
            .map_err(|e| Error::Other(format!("invalid verifying key: {e}")))
    }
}

/// Derive a stable device_id from a public key.
/// Format: first 16 hex chars of a simple hash (not SHA-256 to avoid dep).
fn derive_device_id(pub_bytes: &[u8]) -> String {
    // Simple FNV-1a fold into 8 bytes → 16 hex chars. Good enough for MVP.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in pub_bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

// ── Signing ───────────────────────────────────────────────────────────────────

/// Sign a JSON-serializable payload. Returns base64-encoded signature.
pub fn sign_payload<T: Serialize>(payload: &T, keys: &DeviceKeys) -> Result<String, Error> {
    let signing_key = keys.signing_key()?;
    let canonical = serde_json::to_vec(payload)
        .map_err(|e| Error::Json(e))?;
    let sig: Signature = signing_key.sign(&canonical);
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, sig.to_bytes()))
}

/// Verify a signature against a JSON payload.
pub fn verify_payload<T: Serialize>(
    payload: &T,
    signature_b64: &str,
    verifying_key_b64: &str,
) -> Result<bool, Error> {
    let canonical = serde_json::to_vec(payload).map_err(Error::Json)?;

    let sig_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, signature_b64)
        .map_err(|e| Error::Other(format!("sig base64: {e}")))?;
    let sig_arr: [u8; 64] = sig_bytes.try_into()
        .map_err(|_| Error::Other("signature must be 64 bytes".to_string()))?;
    let sig = Signature::from_bytes(&sig_arr);

    let vk_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, verifying_key_b64)
        .map_err(|e| Error::Other(format!("vk base64: {e}")))?;
    let vk_arr: [u8; 32] = vk_bytes.try_into()
        .map_err(|_| Error::Other("verifying key must be 32 bytes".to_string()))?;
    let vk = VerifyingKey::from_bytes(&vk_arr)
        .map_err(|e| Error::Other(format!("invalid vk: {e}")))?;

    Ok(vk.verify(&canonical, &sig).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn generate_produces_valid_keys() {
        let keys = DeviceKeys::generate();
        assert_eq!(keys.device_id.len(), 16);
        assert!(keys.signing_key().is_ok());
        assert!(keys.verifying_key().is_ok());
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let keys = DeviceKeys::generate();
        let payload = json!({"tile_id": "8a2a1072b59ffff", "value": 42});

        let sig = sign_payload(&payload, &keys).unwrap();
        let ok = verify_payload(&payload, &sig, &keys.public_key_b64).unwrap();
        assert!(ok);
    }

    #[test]
    fn tampered_payload_fails_verification() {
        let keys = DeviceKeys::generate();
        let payload = json!({"tile_id": "8a2a1072b59ffff", "value": 42});
        let sig = sign_payload(&payload, &keys).unwrap();

        let tampered = json!({"tile_id": "8a2a1072b59ffff", "value": 99});
        let ok = verify_payload(&tampered, &sig, &keys.public_key_b64).unwrap();
        assert!(!ok);
    }

    #[test]
    fn wrong_key_fails_verification() {
        let keys1 = DeviceKeys::generate();
        let keys2 = DeviceKeys::generate();
        let payload = json!({"data": "test"});
        let sig = sign_payload(&payload, &keys1).unwrap();
        let ok = verify_payload(&payload, &sig, &keys2.public_key_b64).unwrap();
        assert!(!ok);
    }

    #[test]
    fn device_id_is_deterministic() {
        let keys = DeviceKeys::generate();
        let vk = keys.verifying_key().unwrap();
        let id1 = derive_device_id(&vk.to_bytes());
        let id2 = derive_device_id(&vk.to_bytes());
        assert_eq!(id1, id2);
    }
}
