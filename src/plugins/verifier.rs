use crate::plugins::manifest::PluginManifest;
use crate::utils::config::Config;
use anyhow::Result;
use ed25519_dalek::{Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Verification status for a plugin library and manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// Valid Ed25519 signature from a verified/trusted publisher key.
    Verified,
    /// Plugin is unsigned (no publisher signature provided).
    #[default]
    Unsigned,
    /// Signature provided, but cryptographic verification failed (tampered file or key mismatch).
    InvalidSignature,
    /// Valid signature, but publisher public key is not in the trusted publishers allowlist.
    UntrustedPublisher,
    /// Publisher public key string is malformed or invalid format.
    MalformedKey,
    /// Signature string is malformed or invalid format.
    MalformedSignature,
    /// Library file is missing or unreadable.
    Failed,
}

impl VerificationStatus {
    pub fn label(&self) -> &'static str {
        match self {
            VerificationStatus::Verified => "verified",
            VerificationStatus::Unsigned => "unsigned",
            VerificationStatus::InvalidSignature => "invalid_signature",
            VerificationStatus::UntrustedPublisher => "untrusted_publisher",
            VerificationStatus::MalformedKey => "malformed_key",
            VerificationStatus::MalformedSignature => "malformed_signature",
            VerificationStatus::Failed => "failed",
        }
    }

    pub fn is_verified(&self) -> bool {
        matches!(self, VerificationStatus::Verified)
    }
}

/// Result details of plugin signature verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub publisher: Option<String>,
    pub publisher_key: Option<String>,
    pub detail: Option<String>,
}

impl VerificationResult {
    pub fn unsigned() -> Self {
        Self {
            status: VerificationStatus::Unsigned,
            publisher: None,
            publisher_key: None,
            detail: Some("No signature provided in plugin manifest".into()),
        }
    }
}

/// Compute SHA-256 digest of a file.
pub fn compute_file_sha256(path: &Path) -> Result<[u8; 32]> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let array: [u8; 32] = hasher.finalize().into();
    Ok(array)
}

/// Decode a 32-byte public key from Stellar StrKey ('G...') or hex string.
pub fn parse_public_key_bytes(key_str: &str) -> Result<[u8; 32]> {
    let key_str = key_str.trim();
    if key_str.starts_with('G') && key_str.len() == 56 {
        let pk = stellar_strkey::ed25519::PublicKey::from_string(key_str)
            .map_err(|e| anyhow::anyhow!("Invalid Stellar public key 'G...': {:?}", e))?;
        Ok(pk.0)
    } else if key_str.len() == 64 {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(key_str, &mut bytes)
            .map_err(|e| anyhow::anyhow!("Invalid hex public key: {}", e))?;
        Ok(bytes)
    } else {
        anyhow::bail!(
            "Invalid public key format (expected 56-char Stellar G-address or 64-char hex string)"
        )
    }
}

/// Decode a 64-byte Ed25519 signature from hex or base64.
pub fn parse_signature_bytes(sig_str: &str) -> Result<[u8; 64]> {
    let sig_str = sig_str.trim();
    let mut bytes = [0u8; 64];
    if sig_str.len() == 128 {
        hex::decode_to_slice(sig_str, &mut bytes)
            .map_err(|e| anyhow::anyhow!("Invalid signature hex format: {}", e))?;
        Ok(bytes)
    } else {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(sig_str)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(sig_str))
            .map_err(|e| anyhow::anyhow!("Invalid signature encoding: {}", e))?;
        if decoded.len() != 64 {
            anyhow::bail!("Signature must be 64 bytes (got {})", decoded.len());
        }
        bytes.copy_from_slice(&decoded);
        Ok(bytes)
    }
}

/// Verify plugin signature against library file SHA-256 digest and manifest signature metadata.
pub fn verify_plugin_signature(
    library_path: &Path,
    manifest: &PluginManifest,
    config: &Config,
) -> VerificationResult {
    let publisher = manifest.publisher.clone();
    let publisher_key = manifest.publisher_key.clone();

    let sig_str = match &manifest.signature {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return VerificationResult::unsigned(),
    };

    let key_str = match &manifest.publisher_key {
        Some(k) if !k.trim().is_empty() => k.trim(),
        _ => {
            return VerificationResult {
                status: VerificationStatus::MalformedKey,
                publisher,
                publisher_key: None,
                detail: Some("Signature present but publisher_key is missing".into()),
            }
        }
    };

    if !library_path.exists() {
        return VerificationResult {
            status: VerificationStatus::Failed,
            publisher,
            publisher_key: Some(key_str.to_string()),
            detail: Some(format!(
                "Library file not found at {}",
                library_path.display()
            )),
        };
    }

    let file_digest = match compute_file_sha256(library_path) {
        Ok(d) => d,
        Err(e) => {
            return VerificationResult {
                status: VerificationStatus::Failed,
                publisher,
                publisher_key: Some(key_str.to_string()),
                detail: Some(format!("Failed to compute library checksum: {}", e)),
            }
        }
    };

    let pk_bytes = match parse_public_key_bytes(key_str) {
        Ok(b) => b,
        Err(e) => {
            return VerificationResult {
                status: VerificationStatus::MalformedKey,
                publisher,
                publisher_key: Some(key_str.to_string()),
                detail: Some(e.to_string()),
            }
        }
    };

    let sig_bytes = match parse_signature_bytes(sig_str) {
        Ok(b) => b,
        Err(e) => {
            return VerificationResult {
                status: VerificationStatus::MalformedSignature,
                publisher,
                publisher_key: Some(key_str.to_string()),
                detail: Some(e.to_string()),
            }
        }
    };

    let verifying_key = match VerifyingKey::from_bytes(&pk_bytes) {
        Ok(vk) => vk,
        Err(e) => {
            return VerificationResult {
                status: VerificationStatus::MalformedKey,
                publisher,
                publisher_key: Some(key_str.to_string()),
                detail: Some(format!("Invalid Ed25519 public key bytes: {}", e)),
            }
        }
    };

    let ed_sig = match ed25519_dalek::Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(e) => {
            return VerificationResult {
                status: VerificationStatus::MalformedSignature,
                publisher,
                publisher_key: Some(key_str.to_string()),
                detail: Some(format!("Invalid Ed25519 signature slice: {}", e)),
            }
        }
    };

    // Verify signature against file digest or full library bytes
    let digest_verified = verifying_key.verify(&file_digest, &ed_sig).is_ok();
    let raw_bytes_verified = if !digest_verified {
        if let Ok(raw_bytes) = fs::read(library_path) {
            verifying_key.verify(&raw_bytes, &ed_sig).is_ok()
        } else {
            false
        }
    } else {
        false
    };

    if !digest_verified && !raw_bytes_verified {
        return VerificationResult {
            status: VerificationStatus::InvalidSignature,
            publisher,
            publisher_key: Some(key_str.to_string()),
            detail: Some(
                "Signature verification failed: signature does not match binary contents".into(),
            ),
        };
    }

    // Check trusted publishers list if configured
    if !config.plugin_trust.trusted_publishers.is_empty() {
        let is_trusted_pubkey = config
            .plugin_trust
            .trusted_publishers
            .iter()
            .any(|tp| is_publisher_key_match(tp, key_str, &pk_bytes));

        if !is_trusted_pubkey {
            return VerificationResult {
                status: VerificationStatus::UntrustedPublisher,
                publisher,
                publisher_key: Some(key_str.to_string()),
                detail: Some(format!(
                    "Publisher key '{}' is not in trusted_publishers list",
                    key_str
                )),
            };
        }
    }

    VerificationResult {
        status: VerificationStatus::Verified,
        publisher,
        publisher_key: Some(key_str.to_string()),
        detail: Some("Ed25519 signature verified successfully".into()),
    }
}

fn is_publisher_key_match(trusted_entry: &str, key_str: &str, key_bytes: &[u8; 32]) -> bool {
    let trusted_entry = trusted_entry.trim();
    if trusted_entry.is_empty() {
        return false;
    }

    if trusted_entry.eq_ignore_ascii_case(key_str) {
        return true;
    }

    if let Ok(tp_bytes) = parse_public_key_bytes(trusted_entry) {
        if &tp_bytes == key_bytes {
            return true;
        }
    }

    let g_addr = stellar_strkey::ed25519::PublicKey(*key_bytes).to_string();
    if trusted_entry.eq_ignore_ascii_case(&g_addr) {
        return true;
    }

    let hex_addr = hex::encode(key_bytes);
    if trusted_entry.eq_ignore_ascii_case(&hex_addr) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::TempDir;

    fn create_test_keypair() -> (SigningKey, VerifyingKey, String, String) {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        let pk_bytes = verifying_key.to_bytes();
        let g_addr = stellar_strkey::ed25519::PublicKey(pk_bytes).to_string();
        let hex_pk = hex::encode(pk_bytes);
        (signing_key, verifying_key, g_addr, hex_pk)
    }

    #[test]
    fn parse_stellar_public_key_works() {
        let (_, _, g_addr, hex_pk) = create_test_keypair();
        let bytes1 = parse_public_key_bytes(&g_addr).unwrap();
        let bytes2 = parse_public_key_bytes(&hex_pk).unwrap();
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn parse_invalid_public_key_fails() {
        assert!(parse_public_key_bytes("invalid").is_err());
        assert!(parse_public_key_bytes("GTOO_SHORT").is_err());
    }

    #[test]
    fn verify_signed_plugin_digest_succeeds() {
        let tmp = TempDir::new().unwrap();
        let lib_path = tmp.path().join("libtest.so");
        fs::write(&lib_path, b"plugin binary content").unwrap();

        let (signing_key, _, g_addr, _) = create_test_keypair();
        let digest = compute_file_sha256(&lib_path).unwrap();
        let signature = signing_key.sign(&digest);
        let sig_hex = hex::encode(signature.to_bytes());

        let manifest = PluginManifest {
            name: "test".into(),
            version: "1.0.0".into(),
            starforge_version: "0.1.0".into(),
            description: "".into(),
            starforge_version_min: None,
            starforge_version_max: None,
            required_capabilities: vec![],
            publisher: Some("Test Publisher".into()),
            publisher_key: Some(g_addr.clone()),
            signature: Some(sig_hex),
        };

        let config = Config::default();
        let res = verify_plugin_signature(&lib_path, &manifest, &config);
        assert_eq!(res.status, VerificationStatus::Verified);
        assert_eq!(res.publisher, Some("Test Publisher".into()));
        assert_eq!(res.publisher_key, Some(g_addr));
    }

    #[test]
    fn verify_tampered_plugin_fails_with_invalid_signature() {
        let tmp = TempDir::new().unwrap();
        let lib_path = tmp.path().join("libtest.so");
        fs::write(&lib_path, b"original content").unwrap();

        let (signing_key, _, g_addr, _) = create_test_keypair();
        let digest = compute_file_sha256(&lib_path).unwrap();
        let signature = signing_key.sign(&digest);
        let sig_hex = hex::encode(signature.to_bytes());

        // Tamper with file after signing
        fs::write(&lib_path, b"tampered content").unwrap();

        let manifest = PluginManifest {
            name: "test".into(),
            version: "1.0.0".into(),
            starforge_version: "0.1.0".into(),
            description: "".into(),
            starforge_version_min: None,
            starforge_version_max: None,
            required_capabilities: vec![],
            publisher: Some("Test Publisher".into()),
            publisher_key: Some(g_addr),
            signature: Some(sig_hex),
        };

        let config = Config::default();
        let res = verify_plugin_signature(&lib_path, &manifest, &config);
        assert_eq!(res.status, VerificationStatus::InvalidSignature);
    }
}
