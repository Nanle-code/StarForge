//! Per-network contract and account aliases.
//!
//! Users can give a deployed contract (or account) a short, human-friendly
//! name and then use that name anywhere an address is accepted:
//!
//! ```text
//! starforge alias set token CDLZ...SC --network testnet
//! starforge contract invoke --id token ...
//! ```
//!
//! Aliases are scoped per network and persisted as JSON in
//! `~/.starforge/aliases.json`. [`AliasStore::resolve`] is the single entry
//! point every command should use: it passes a raw Stellar address through
//! unchanged and otherwise looks the input up as an alias.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// File name of the alias store inside the StarForge config directory.
pub const ALIASES_FILE: &str = "aliases.json";

/// Number of characters in a Stellar strkey-encoded contract/account id.
const STELLAR_ADDRESS_LEN: usize = 56;

/// Errors produced while loading, storing, or resolving aliases.
#[derive(Debug, thiserror::Error)]
pub enum AliasError {
    /// The user's home directory could not be determined.
    #[error("could not determine your home directory (needed for ~/.starforge/aliases.json)")]
    NoHomeDir,

    /// The alias store could not be read.
    #[error("failed to read alias store at {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },

    /// The alias store could not be written.
    #[error("failed to write alias store at {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },

    /// The alias store contained invalid JSON.
    #[error("failed to parse alias store at {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },

    /// The alias name is empty or contains unsupported characters.
    #[error("alias name '{name}' is invalid: {reason}")]
    InvalidName { name: String, reason: String },

    /// The network name is empty or contains unsupported characters.
    #[error("network name '{name}' is invalid: {reason}")]
    InvalidNetwork { name: String, reason: String },

    /// The alias name is itself a raw Stellar address.
    #[error("alias name '{name}' must not be a raw Stellar address")]
    NameLooksLikeAddress { name: String },

    /// The address being stored does not look like a Stellar address.
    #[error("'{address}' is not a Stellar contract (C...) or account (G...) address (expected 56 characters)")]
    InvalidAddress { address: String },

    /// No alias with that name exists on the requested network, and it does
    /// not exist on any other network either.
    #[error("unknown alias '{name}' on network '{network}'. Define it with `starforge alias set {name} <ADDRESS> --network {network}`")]
    UnknownAlias { name: String, network: String },

    /// The alias exists, but only on a different network.
    #[error("alias '{name}' is not defined on network '{network}' (it exists on: {others}).\n  Re-run with `--network <NETWORK>` or create it here with `starforge alias set {name} <ADDRESS> --network {network}`")]
    WrongNetwork {
        name: String,
        network: String,
        others: String,
    },
}

/// A per-network alias table persisted as JSON at `~/.starforge/aliases.json`.
///
/// The on-disk shape is `{ "<network>": { "<name>": "<address>" } }`; the
/// store's own path is not serialized (it is restored on load).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AliasStore {
    #[serde(skip)]
    path: PathBuf,
    #[serde(default)]
    networks: BTreeMap<String, BTreeMap<String, String>>,
}

impl AliasStore {
    /// Create an empty store bound to `path` without touching the disk.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            networks: BTreeMap::new(),
        }
    }

    /// Load the default store from `~/.starforge/aliases.json`.
    ///
    /// A missing file yields an empty store.
    pub fn load() -> Result<Self, AliasError> {
        Self::load_from(default_store_path()?)
    }

    /// Load the store at `path`. A missing file yields an empty store.
    pub fn load_from(path: PathBuf) -> Result<Self, AliasError> {
        if !path.exists() {
            return Ok(Self::new(path));
        }
        let data = fs::read_to_string(&path).map_err(|source| AliasError::Read {
            path: path.display().to_string(),
            source,
        })?;
        let mut store: AliasStore =
            serde_json::from_str(&data).map_err(|source| AliasError::Parse {
                path: path.display().to_string(),
                source,
            })?;
        store.path = path;
        Ok(store)
    }

    /// Path this store is persisted to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Persist the store, creating the parent directory if needed.
    pub fn save(&self) -> Result<(), AliasError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|source| AliasError::Write {
                    path: self.path.display().to_string(),
                    source,
                })?;
            }
        }
        let data = serde_json::to_string_pretty(self).map_err(|source| AliasError::Parse {
            path: self.path.display().to_string(),
            source,
        })?;
        fs::write(&self.path, data).map_err(|source| AliasError::Write {
            path: self.path.display().to_string(),
            source,
        })
    }

    /// Insert or update `name` on `network`, returning the previous address.
    pub fn set(
        &mut self,
        network: &str,
        name: &str,
        address: &str,
    ) -> Result<Option<String>, AliasError> {
        let network = validate_network_name(network)?;
        let name = validate_alias_name(name)?;
        let address = validate_address(address)?;
        let entries = self.networks.entry(network).or_default();
        Ok(entries.insert(name, address))
    }

    /// Remove `name` from `network`. Returns `true` when an entry was removed.
    pub fn remove(&mut self, network: &str, name: &str) -> bool {
        self.networks
            .get_mut(network)
            .and_then(|entries| entries.remove(name))
            .is_some()
    }

    /// All aliases defined for `network`, sorted by name.
    pub fn list(&self, network: &str) -> Vec<(String, String)> {
        self.networks
            .get(network)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(name, address)| (name.clone(), address.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve `input` to a Stellar address.
    ///
    /// A raw 56-character `C...`/`G...` address is returned unchanged. Anything
    /// else is looked up as an alias scoped to `network`; a clear error is
    /// returned when the alias is unknown or defined only on another network.
    pub fn resolve(&self, network: &str, input: &str) -> Result<String, AliasError> {
        if looks_like_stellar_address(input) {
            return Ok(input.to_string());
        }

        if let Some(address) = self
            .networks
            .get(network)
            .and_then(|entries| entries.get(input))
        {
            return Ok(address.clone());
        }

        let others: Vec<&str> = self
            .networks
            .iter()
            .filter(|(candidate, _)| candidate.as_str() != network)
            .filter(|(_, entries)| entries.contains_key(input))
            .map(|(candidate, _)| candidate.as_str())
            .collect();

        if others.is_empty() {
            Err(AliasError::UnknownAlias {
                name: input.to_string(),
                network: network.to_string(),
            })
        } else {
            Err(AliasError::WrongNetwork {
                name: input.to_string(),
                network: network.to_string(),
                others: others.join(", "),
            })
        }
    }
}

/// Path to the default alias store (`~/.starforge/aliases.json`).
pub fn default_store_path() -> Result<PathBuf, AliasError> {
    let home = dirs::home_dir().ok_or(AliasError::NoHomeDir)?;
    Ok(home.join(".starforge").join(ALIASES_FILE))
}

/// True when `value` looks like a Stellar contract/account address: exactly
/// 56 characters and starting with `C` or `G`.
pub fn looks_like_stellar_address(value: &str) -> bool {
    value.len() == STELLAR_ADDRESS_LEN && (value.starts_with('C') || value.starts_with('G'))
}

/// Convenience hook for deploy flows: record the manifest contract name
/// against the address it deployed to on `network`.
pub fn record_deploy_alias(network: &str, name: &str, address: &str) -> Result<(), AliasError> {
    let mut store = AliasStore::load()?;
    store.set(network, name, address)?;
    store.save()
}

fn validate_network_name(network: &str) -> Result<String, AliasError> {
    let network = network.trim();
    if network.is_empty() {
        return Err(AliasError::InvalidNetwork {
            name: network.to_string(),
            reason: "network name cannot be empty".to_string(),
        });
    }
    if let Some(bad) = network
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        return Err(AliasError::InvalidNetwork {
            name: network.to_string(),
            reason: format!(
                "contains invalid character '{}'; use letters, digits, '-' or '_'",
                bad
            ),
        });
    }
    Ok(network.to_string())
}

fn validate_alias_name(name: &str) -> Result<String, AliasError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AliasError::InvalidName {
            name: name.to_string(),
            reason: "alias name cannot be empty".to_string(),
        });
    }
    if name.len() > 64 {
        return Err(AliasError::InvalidName {
            name: name.to_string(),
            reason: "alias name must be 64 characters or fewer".to_string(),
        });
    }
    if looks_like_stellar_address(name) {
        return Err(AliasError::NameLooksLikeAddress {
            name: name.to_string(),
        });
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        return Err(AliasError::InvalidName {
            name: name.to_string(),
            reason: format!(
                "contains invalid character '{}'; use letters, digits, '-' or '_'",
                bad
            ),
        });
    }
    Ok(name.to_string())
}

fn validate_address(address: &str) -> Result<String, AliasError> {
    let address = address.trim();
    if !looks_like_stellar_address(address) {
        return Err(AliasError::InvalidAddress {
            address: address.to_string(),
        });
    }
    Ok(address.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTRACT: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";
    const ACCOUNT: &str = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNT";

    /// A store backed by a real temp file. The returned [`tempfile::TempDir`]
    /// must stay alive for the duration of the test.
    fn temp_store() -> (tempfile::TempDir, AliasStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ALIASES_FILE);
        (dir, AliasStore::new(path))
    }

    #[test]
    fn resolves_a_known_alias_after_a_save_and_reload() {
        let (_dir, mut store) = temp_store();
        store.set("testnet", "token", CONTRACT).unwrap();
        store.save().unwrap();

        let reloaded = AliasStore::load_from(store.path().to_path_buf()).unwrap();
        assert_eq!(reloaded.resolve("testnet", "token").unwrap(), CONTRACT);
    }

    #[test]
    fn aliases_are_scoped_per_network() {
        let (_dir, mut store) = temp_store();
        store.set("testnet", "token", CONTRACT).unwrap();
        store.set("mainnet", "token", ACCOUNT).unwrap();

        assert_eq!(store.resolve("testnet", "token").unwrap(), CONTRACT);
        assert_eq!(store.resolve("mainnet", "token").unwrap(), ACCOUNT);
    }

    #[test]
    fn unknown_alias_error_names_the_alias_and_network() {
        let (_dir, store) = temp_store();
        let err = store.resolve("testnet", "missing").unwrap_err();
        assert!(matches!(err, AliasError::UnknownAlias { .. }));
        let message = err.to_string();
        assert!(message.contains("missing"));
        assert!(message.contains("testnet"));
    }

    #[test]
    fn alias_only_on_another_network_is_a_wrong_network_error() {
        let (_dir, mut store) = temp_store();
        store.set("mainnet", "token", CONTRACT).unwrap();

        let err = store.resolve("testnet", "token").unwrap_err();
        match &err {
            AliasError::WrongNetwork { others, .. } => assert!(others.contains("mainnet")),
            other => panic!("expected WrongNetwork, got {other:?}"),
        }
        assert!(err.to_string().contains("not defined on network 'testnet'"));
    }

    #[test]
    fn raw_addresses_pass_through_unchanged() {
        let (_dir, store) = temp_store();
        assert_eq!(store.resolve("testnet", CONTRACT).unwrap(), CONTRACT);
        assert_eq!(store.resolve("mainnet", ACCOUNT).unwrap(), ACCOUNT);
    }

    #[test]
    fn remove_only_affects_the_requested_network() {
        let (_dir, mut store) = temp_store();
        store.set("testnet", "token", CONTRACT).unwrap();
        store.set("mainnet", "token", CONTRACT).unwrap();

        assert!(store.remove("testnet", "token"));
        assert!(!store.remove("testnet", "token"));
        assert!(store.resolve("testnet", "token").is_err());
        assert_eq!(store.resolve("mainnet", "token").unwrap(), CONTRACT);
    }

    #[test]
    fn list_returns_names_sorted() {
        let (_dir, mut store) = temp_store();
        store.set("testnet", "zeta", CONTRACT).unwrap();
        store.set("testnet", "alpha", CONTRACT).unwrap();

        let names: Vec<String> = store.list("testnet").into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn invalid_names_and_addresses_are_rejected() {
        let (_dir, mut store) = temp_store();
        assert!(store.set("testnet", "", CONTRACT).is_err());
        assert!(store.set("testnet", "has space", CONTRACT).is_err());
        assert!(store.set("testnet", CONTRACT, CONTRACT).is_err());
        assert!(store.set("testnet", "token", "not-an-address").is_err());
        assert!(store.set("bad network", "token", CONTRACT).is_err());
    }
}
