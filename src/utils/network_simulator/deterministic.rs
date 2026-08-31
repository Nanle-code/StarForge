//! # Deterministic Execution Engine
//!
//! Provides seeded pseudo-random number generation and deterministic
//! parameters so that simulations produce identical results across runs.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

// ── Deterministic Configuration ───────────────────────────────────────────────

/// Controls deterministic behaviour in the simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicConfig {
    /// Seed for the PRNG (0 = auto-generated).
    pub seed: u64,
    /// If `true`, contract IDs are derived deterministically from the seed +
    /// WASM hash instead of using random addresses.
    pub deterministic_contract_ids: bool,
    /// If `true`, transaction results (return values, events) are reproducible.
    pub deterministic_execution: bool,
}

impl Default for DeterministicConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            deterministic_contract_ids: true,
            deterministic_execution: true,
        }
    }
}

// ── Seeded RNG ────────────────────────────────────────────────────────────────

/// A thread-safe, seeded RNG wrapper.
///
/// All random decisions in the simulator go through this type so that they
/// are reproducible when the same seed is used.
pub struct SeededRng {
    inner: Mutex<StdRng>,
    seed: u64,
}

impl std::fmt::Debug for SeededRng {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeededRng")
            .field("seed", &self.seed)
            .finish_non_exhaustive()
    }
}

impl SeededRng {
    /// Create a new RNG from the given seed.
    pub fn new(seed: u64) -> Self {
        Self {
            inner: Mutex::new(StdRng::seed_from_u64(seed)),
            seed,
        }
    }

    /// Return the seed used to initialise this RNG.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Generate a random u64.
    pub fn next_u64(&self) -> u64 {
        self.inner.lock().unwrap().gen::<u64>()
    }

    /// Generate a random u32.
    pub fn next_u32(&self) -> u32 {
        self.inner.lock().unwrap().gen::<u32>()
    }

    /// Generate a random byte slice of the given length.
    pub fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        self.inner.lock().unwrap().fill(&mut buf[..]);
        buf
    }

    /// Pick a random element from a slice.
    pub fn pick<'a, T>(&self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let idx = self.inner.lock().unwrap().gen_range(0..items.len());
        Some(&items[idx])
    }

    /// Roll a probability check (0.0..1.0).
    pub fn probability(&self) -> f64 {
        self.inner.lock().unwrap().gen::<f64>()
    }

    /// Reset the RNG back to its initial seed.
    pub fn reset(&self) {
        *self.inner.lock().unwrap() = StdRng::seed_from_u64(self.seed);
    }
}

// ── Deterministic helpers ─────────────────────────────────────────────────────

/// Derive a deterministic contract ID (56-char Stellar strkey `C...`) from a
/// seed and a WASM hash.
pub fn derive_contract_id(seed: u64, wasm_hash: &str, index: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(wasm_hash.as_bytes());
    hasher.update(index.to_le_bytes());
    let hash = hasher.finalize();

    // Use first 29 bytes of the hash to build 56-base32 chars (C...)
    let mut id = String::with_capacity(56);
    id.push('C');
    // Simple deterministic mapping from hash bytes to base32 chars
    let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    for &byte in hash.iter().take(55) {
        id.push(chars[(byte as usize) % 32] as char);
    }
    // Pad to 56 chars if needed
    while id.len() < 56 {
        id.push('2');
    }
    id
}

/// Derive a deterministic public key (56-char Stellar strkey `G...`) from a
/// seed and account index.
pub fn derive_public_key(seed: u64, account_index: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"starforge-sim-key");
    hasher.update(seed.to_le_bytes());
    hasher.update(account_index.to_le_bytes());
    let hash = hasher.finalize();

    let mut key = String::with_capacity(56);
    key.push('G');
    let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    for &byte in hash.iter().take(55) {
        key.push(chars[(byte as usize) % 32] as char);
    }
    while key.len() < 56 {
        key.push('2');
    }
    key
}

/// Derive a deterministic transaction hash (64 hex chars).
pub fn derive_tx_hash(seed: u64, nonce: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"starforge-sim-tx");
    hasher.update(seed.to_le_bytes());
    hasher.update(nonce.to_le_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_rng_is_reproducible() {
        let rng1 = SeededRng::new(42);
        let rng2 = SeededRng::new(42);
        let vals1: Vec<u64> = (0..10).map(|_| rng1.next_u64()).collect();
        let vals2: Vec<u64> = (0..10).map(|_| rng2.next_u64()).collect();
        assert_eq!(vals1, vals2);
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        let rng1 = SeededRng::new(42);
        let rng2 = SeededRng::new(99);
        let v1 = rng1.next_u64();
        let v2 = rng2.next_u64();
        assert_ne!(v1, v2);
    }

    #[test]
    fn reset_restores_initial_sequence() {
        let rng = SeededRng::new(7);
        let first = rng.next_u64();
        // Advance
        for _ in 0..5 {
            rng.next_u64();
        }
        rng.reset();
        let after_reset = rng.next_u64();
        assert_eq!(first, after_reset);
    }

    #[test]
    fn derive_contract_id_is_56_chars_starts_with_c() {
        let id = derive_contract_id(42, "abc123", 0);
        assert_eq!(id.len(), 56);
        assert!(id.starts_with('C'));
        assert!(id.chars().all(|c| matches!(c, 'A'..='Z' | '2'..='7')));
    }

    #[test]
    fn derive_contract_id_is_deterministic() {
        let id1 = derive_contract_id(42, "abc123", 0);
        let id2 = derive_contract_id(42, "abc123", 0);
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_indexes_give_different_ids() {
        let id0 = derive_contract_id(42, "abc", 0);
        let id1 = derive_contract_id(42, "abc", 1);
        assert_ne!(id0, id1);
    }

    #[test]
    fn derive_public_key_is_56_chars_starts_with_g() {
        let key = derive_public_key(42, 0);
        assert_eq!(key.len(), 56);
        assert!(key.starts_with('G'));
    }

    #[test]
    fn derive_tx_hash_is_64_hex_chars() {
        let hash = derive_tx_hash(42, 1);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn probability_is_between_zero_and_one() {
        let rng = SeededRng::new(123);
        for _ in 0..100 {
            let p = rng.probability();
            assert!(p >= 0.0 && p <= 1.0);
        }
    }

    #[test]
    fn random_bytes_returns_correct_length() {
        let rng = SeededRng::new(7);
        for len in [0, 1, 8, 16, 32, 64] {
            let bytes = rng.random_bytes(len);
            assert_eq!(bytes.len(), len);
        }
    }
}
