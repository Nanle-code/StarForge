/// In-memory, mlock-protected key store for the signing agent.
///
/// Security properties
/// -------------------
/// * Key bytes are stored in a `Zeroizing<[u8; 32]>` buffer — `Drop` impl
///   overwrites them with zeros automatically.
/// * On Unix, `mlock(2)` prevents the key pages from being swapped to disk.
///   `munlock(2)` is called before the memory is released.
/// * On Windows, `VirtualLock` is the equivalent and is called through the
///   `windows-sys` transitive dependency.
/// * Keys expire after a configurable TTL. The `KeyStore::sweep` method
///   zeroizes and removes expired entries; it should be driven by a periodic
///   tokio timer (see `server.rs`).
/// * `RemoveAll` and `Drop` both zeroize every remaining entry.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

use crate::agent::proto::{AgentError, AgentErrorCode, LoadedKeyInfo};

/// Default key lifetime when none is specified by the caller.
pub const DEFAULT_TIMEOUT_SECS: u64 = 900; // 15 minutes

// ---------------------------------------------------------------------------
// LockedKey — single key entry
// ---------------------------------------------------------------------------

/// A loaded Ed25519 secret key with an optional expiry.
pub struct LockedKey {
    /// Raw 32-byte seed of the Ed25519 signing key, held in zeroizing memory.
    seed: Zeroizing<[u8; 32]>,
    /// Stellar G-address of this key (for display and verification only).
    pub public_key: String,
    /// Absolute point in time when this key expires. `None` = never.
    expires_at: Option<Instant>,
}

impl LockedKey {
    /// Create a new `LockedKey` from a plaintext Stellar secret key (S…).
    ///
    /// Returns an error if `secret_str` is not a valid Stellar Ed25519 seed.
    pub fn from_stellar_secret(
        secret_str: &str,
        public_key: String,
        ttl: Option<Duration>,
    ) -> Result<Self> {
        let seed = stellar_secret_to_seed(secret_str)?;

        // Verify the public key matches before storing.
        let derived_pub = seed_to_public_key(&seed)?;
        if derived_pub != public_key {
            return Err(AgentError::new(
                AgentErrorCode::PublicKeyMismatch,
                format!(
                    "Provided public key {public_key} does not match the secret key \
                     (derived {derived_pub})"
                ),
            )
            .into());
        }

        let expires_at = ttl.map(|d| Instant::now() + d);

        let mut locked = Self {
            seed: Zeroizing::new([0u8; 32]),
            public_key,
            expires_at,
        };
        locked.seed.copy_from_slice(&seed);

        // Attempt to lock the key pages in RAM so they cannot be swapped.
        mlock_seed(&locked.seed);

        Ok(locked)
    }

    /// Return `true` if this key has passed its expiry time.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|t| Instant::now() >= t)
            .unwrap_or(false)
    }

    /// Seconds remaining until expiry, or `None` if the key never expires.
    pub fn ttl_remaining(&self) -> Option<u64> {
        self.expires_at.map(|t| {
            let now = Instant::now();
            if now >= t {
                0
            } else {
                (t - now).as_secs()
            }
        })
    }

    /// Sign `transaction_xdr` (base64 `TransactionEnvelope`) with this key.
    pub fn sign(&self, transaction_xdr: &str, network_passphrase: &str) -> Result<String> {
        // Encode the seed back to a Stellar StrKey, then let wallet_signer
        // do the full XDR decode → sign → encode pipeline.
        let secret_str = {
            use stellar_strkey::ed25519::PrivateKey;
            PrivateKey::from_bytes(&*self.seed).to_string()
        };
        let request = crate::utils::wallet_signer::SigningRequest::local_secret(
            zeroize::Zeroizing::new(secret_str),
            network_passphrase,
        );
        crate::utils::wallet_signer::sign_transaction_xdr(transaction_xdr, &request)
            .context("Agent: failed to sign transaction")
    }

    /// Extend the TTL of this key (used by `add_key` when refreshing an already-loaded key).
    pub fn refresh_ttl(&mut self, ttl: Option<Duration>) {
        self.expires_at = ttl.map(|d| Instant::now() + d);
    }
}

impl Drop for LockedKey {
    fn drop(&mut self) {
        munlock_seed(&self.seed);
        // `Zeroizing` takes care of the actual zero-fill via its own Drop impl.
    }
}

// ---------------------------------------------------------------------------
// mlock helpers — Unix
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn mlock_seed(seed: &[u8; 32]) {
    use nix::sys::mman::mlock;
    use std::num::NonZeroUsize;
    let ptr = seed.as_ptr() as *const libc::c_void;
    let len = NonZeroUsize::new(32).unwrap();
    // Best-effort: if mlock fails (e.g. RLIMIT_MEMLOCK too low) we continue
    // without paging protection and log a warning.
    if let Err(e) = unsafe { mlock(ptr, len) } {
        tracing::warn!(
            error = %e,
            "signing agent: mlock failed — key may be swappable; \
             raise RLIMIT_MEMLOCK or run with sufficient privileges"
        );
    }
}

#[cfg(unix)]
fn munlock_seed(seed: &[u8; 32]) {
    use nix::sys::mman::munlock;
    use std::num::NonZeroUsize;
    let ptr = seed.as_ptr() as *const libc::c_void;
    let len = NonZeroUsize::new(32).unwrap();
    let _ = unsafe { munlock(ptr, len) };
}

// ---------------------------------------------------------------------------
// mlock helpers — Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn mlock_seed(seed: &[u8; 32]) {
    use std::ffi::c_void;
    // SAFETY: VirtualLock is a benign advisory call; failure is non-fatal.
    let ok = unsafe {
        windows_sys::Win32::System::Memory::VirtualLock(
            seed.as_ptr() as *const c_void,
            32,
        )
    };
    if ok == 0 {
        tracing::warn!(
            "signing agent: VirtualLock failed — key may be pageable to disk"
        );
    }
}

#[cfg(windows)]
fn munlock_seed(seed: &[u8; 32]) {
    use std::ffi::c_void;
    unsafe {
        let _ = windows_sys::Win32::System::Memory::VirtualUnlock(
            seed.as_ptr() as *const c_void,
            32,
        );
    }
}

// ---------------------------------------------------------------------------
// mlock helpers — other platforms (no-op)
// ---------------------------------------------------------------------------

#[cfg(not(any(unix, windows)))]
fn mlock_seed(_seed: &[u8; 32]) {}

#[cfg(not(any(unix, windows)))]
fn munlock_seed(_seed: &[u8; 32]) {}

// ---------------------------------------------------------------------------
// Stellar StrKey helpers
// ---------------------------------------------------------------------------

/// Decode a Stellar secret-key StrKey (S…) into a 32-byte seed.
pub fn stellar_secret_to_seed(secret: &str) -> Result<[u8; 32]> {
    use stellar_strkey::ed25519::PrivateKey;
    let key = PrivateKey::from_string(secret).map_err(|e| {
        AgentError::new(
            AgentErrorCode::InvalidSecretKey,
            format!("Invalid Stellar secret key: {e}"),
        )
    })?;
    Ok(*key.as_bytes())
}

/// Derive a Stellar G-address from a 32-byte seed.
pub fn seed_to_public_key(seed: &[u8; 32]) -> Result<String> {
    use ed25519_dalek::SigningKey;
    use stellar_strkey::ed25519::PublicKey;
    let signing_key = SigningKey::from_bytes(seed);
    let verifying = signing_key.verifying_key();
    Ok(PublicKey(*verifying.as_bytes()).to_string())
}

// ---------------------------------------------------------------------------
// KeyStore
// ---------------------------------------------------------------------------

/// Thread-safe in-memory map of wallet name → loaded key.
///
/// Callers must acquire the `Arc<Mutex<KeyStore>>` (done in `server.rs`).
pub struct KeyStore {
    keys: HashMap<String, LockedKey>,
    pub default_timeout: Duration,
    pub started_at: Instant,
}

impl KeyStore {
    pub fn new(default_timeout_secs: u64) -> Self {
        Self {
            keys: HashMap::new(),
            default_timeout: Duration::from_secs(default_timeout_secs),
            started_at: Instant::now(),
        }
    }

    /// Add or refresh a key. Returns `Ok(true)` if a new key was inserted,
    /// `Ok(false)` if an existing key's TTL was refreshed.
    pub fn add_key(
        &mut self,
        wallet_name: &str,
        public_key: &str,
        secret_key: &str,
        ttl_secs: Option<u64>,
    ) -> Result<bool> {
        let ttl = Some(Duration::from_secs(
            ttl_secs.unwrap_or(self.default_timeout.as_secs()),
        ));

        // If a key for this wallet is already loaded, just refresh its TTL
        // rather than re-allocating and re-locking the memory page.
        if let Some(existing) = self.keys.get_mut(wallet_name) {
            existing.refresh_ttl(ttl);
            return Ok(false);
        }

        let locked = LockedKey::from_stellar_secret(secret_key, public_key.to_string(), ttl)?;
        self.keys.insert(wallet_name.to_string(), locked);
        Ok(true)
    }

    /// Remove and zeroize a single key. Returns `true` if it was present.
    pub fn remove_key(&mut self, wallet_name: &str) -> bool {
        self.keys.remove(wallet_name).is_some()
    }

    /// Remove and zeroize all keys.
    pub fn remove_all(&mut self) {
        self.keys.clear(); // each LockedKey::drop() zeroizes
    }

    /// Sign `transaction_xdr` with the key for `wallet_name`.
    pub fn sign(
        &self,
        wallet_name: &str,
        transaction_xdr: &str,
        network_passphrase: &str,
    ) -> Result<String, AgentError> {
        let key = self.keys.get(wallet_name).ok_or_else(|| {
            AgentError::new(
                AgentErrorCode::KeyNotLoaded,
                format!("Wallet '{wallet_name}' is not loaded in the agent"),
            )
        })?;
        key.sign(transaction_xdr, network_passphrase)
            .map_err(|e| AgentError::new(AgentErrorCode::SigningFailed, e.to_string()))
    }

    /// Remove all expired keys. Called by the background sweep timer.
    /// Returns the names of the keys that were removed.
    pub fn sweep_expired(&mut self) -> Vec<String> {
        let expired: Vec<String> = self
            .keys
            .iter()
            .filter(|(_, k)| k.is_expired())
            .map(|(name, _)| name.clone())
            .collect();

        for name in &expired {
            tracing::info!(wallet = %name, "agent: key expired, zeroizing");
            self.keys.remove(name);
        }
        expired
    }

    /// Snapshot of currently loaded keys for status reporting.
    pub fn loaded_key_infos(&self) -> Vec<LoadedKeyInfo> {
        self.keys
            .iter()
            .map(|(name, key)| LoadedKeyInfo {
                wallet_name: name.clone(),
                public_key: key.public_key.clone(),
                ttl_remaining_secs: key.ttl_remaining(),
            })
            .collect()
    }

    /// Number of currently loaded keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Return `true` if a non-expired key for `wallet_name` is present.
    pub fn has_key(&self, wallet_name: &str) -> bool {
        self.keys
            .get(wallet_name)
            .map(|k| !k.is_expired())
            .unwrap_or(false)
    }
}

impl Drop for KeyStore {
    fn drop(&mut self) {
        self.remove_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_keypair() -> (String, String) {
        // A known test Ed25519 key — never use on mainnet.
        // seed = [1u8; 32]
        let seed = [1u8; 32];
        let secret = {
            use stellar_strkey::ed25519::PrivateKey;
            PrivateKey::from_bytes(&seed).to_string()
        };
        let pubkey = seed_to_public_key(&seed).unwrap();
        (secret, pubkey)
    }

    #[test]
    fn add_and_remove_key() {
        let (secret, pubkey) = dummy_keypair();
        let mut ks = KeyStore::new(60);
        assert!(ks.add_key("alice", &pubkey, &secret, None).unwrap());
        assert!(ks.has_key("alice"));
        assert!(ks.remove_key("alice"));
        assert!(!ks.has_key("alice"));
    }

    #[test]
    fn refresh_existing_key_returns_false() {
        let (secret, pubkey) = dummy_keypair();
        let mut ks = KeyStore::new(60);
        assert!(ks.add_key("alice", &pubkey, &secret, None).unwrap());
        // Second add should refresh TTL, not insert a new key.
        assert!(!ks.add_key("alice", &pubkey, &secret, Some(120)).unwrap());
        assert_eq!(ks.len(), 1);
    }

    #[test]
    fn expired_key_is_swept() {
        let (secret, pubkey) = dummy_keypair();
        let mut ks = KeyStore::new(60);
        // Load with a TTL that is effectively already passed (0 seconds from now).
        // We fake expiry by loading with a 1-second TTL then sleeping 2 seconds —
        // but that's slow in unit tests. Instead we use the has_key/sweep path:
        // load with a 0-second TTL, which sets expires_at = Instant::now(),
        // and an immediate sweep should remove it.
        ks.add_key("alice", &pubkey, &secret, Some(0)).unwrap();
        // Key may or may not have expired within the same millisecond, but
        // after a tiny sleep it definitely will have.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let swept = ks.sweep_expired();
        assert!(swept.contains(&"alice".to_string()));
        assert!(!ks.has_key("alice"));
    }

    #[test]
    fn public_key_mismatch_rejected() {
        let (secret, _pubkey) = dummy_keypair();
        // Use a wrong public key (all-G, obviously invalid for this seed).
        let wrong_pub = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";
        let mut ks = KeyStore::new(60);
        let err = ks.add_key("alice", wrong_pub, &secret, None);
        assert!(err.is_err());
    }

    #[test]
    fn remove_all_clears_store() {
        let (secret, pubkey) = dummy_keypair();
        let mut ks = KeyStore::new(60);
        ks.add_key("alice", &pubkey, &secret, None).unwrap();
        ks.add_key("bob", &pubkey, &secret, None).unwrap();
        ks.remove_all();
        assert_eq!(ks.len(), 0);
    }
}
