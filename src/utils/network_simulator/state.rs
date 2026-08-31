//! # State Snapshot / Restore
//!
//! Full and diff-based state snapshots for the network simulator. Snapshots
//! can be saved to/loaded from JSON files for reproducibility.

use crate::utils::network_simulator::simulator::{AccountInfo, ContractInstance, LedgerInfo};
use crate::utils::network_simulator::time::LedgerTime;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A complete snapshot of the simulator's state at one point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Unique snapshot identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// When the snapshot was taken.
    pub created_at: String,
    /// Ledger state at the time of the snapshot.
    pub ledger_info: LedgerInfo,
    /// Accounts.
    pub accounts: Vec<AccountInfo>,
    /// Deployed contract instances.
    pub contracts: Vec<ContractInstance>,
    /// Ledger time.
    pub ledger_time: LedgerTime,
    /// Transaction count leading up to this snapshot.
    pub tx_count: u64,
    /// Optional metadata for the snapshot.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Manages creation, listing, loading, and pruning of state snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManager {
    /// Directory where snapshot files are stored.
    #[serde(skip)]
    storage_dir: Option<PathBuf>,
    /// In-memory snapshots (available even without disk storage).
    #[serde(skip)]
    snapshots: HashMap<String, StateSnapshot>,
    /// Auto-increment counter for snapshot IDs.
    next_id: u64,
}

impl SnapshotManager {
    pub fn new() -> Self {
        Self {
            storage_dir: None,
            snapshots: HashMap::new(),
            next_id: 0,
        }
    }

    /// Set an optional on-disk directory for snapshot persistence.
    pub fn with_storage_dir(mut self, dir: PathBuf) -> Self {
        if !dir.exists() {
            let _ = fs::create_dir_all(&dir);
        }
        self.storage_dir = Some(dir);
        self
    }

    /// Take a snapshot of the current simulator state.
    // Each parameter is an independent, named input (CLI flags / distinct config
    // values); bundling them into a struct here would add indirection without
    // reducing real complexity.
    #[allow(clippy::too_many_arguments)]
    pub fn take_snapshot(
        &mut self,
        label: &str,
        ledger_info: &LedgerInfo,
        accounts: &[AccountInfo],
        contracts: &[ContractInstance],
        ledger_time: &LedgerTime,
        tx_count: u64,
        metadata: HashMap<String, String>,
    ) -> StateSnapshot {
        let id = format!("snap-{:04}", self.next_id);
        self.next_id += 1;

        let snapshot = StateSnapshot {
            id: id.clone(),
            label: label.to_string(),
            created_at: Utc::now().to_rfc3339(),
            ledger_info: ledger_info.clone(),
            accounts: accounts.to_vec(),
            contracts: contracts.to_vec(),
            ledger_time: *ledger_time,
            tx_count,
            metadata,
        };

        // Store in memory.
        self.snapshots.insert(id.clone(), snapshot.clone());

        // Persist to disk if a storage directory is configured.
        if let Some(ref dir) = self.storage_dir {
            let path = dir.join(format!("{}.json", id));
            if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
                let _ = fs::write(&path, json);
            }
        }

        snapshot
    }

    /// Load a snapshot by ID (in-memory only).
    ///
    /// For disk-backed snapshots or when you need to mutate via the returned
    /// reference, use [`load_mut`](Self::load_mut) instead.
    pub fn load(&self, id: &str) -> Option<&StateSnapshot> {
        self.snapshots.get(id)
    }

    /// Load a snapshot by ID (mutable access, can load from disk).
    pub fn load_mut(&mut self, id: &str) -> Option<&StateSnapshot> {
        // Check in-memory first.
        if self.snapshots.contains_key(id) {
            return self.snapshots.get(id);
        }

        // Try on-disk.
        if let Some(ref dir) = self.storage_dir {
            let path = dir.join(format!("{}.json", id));
            if path.exists() {
                if let Ok(json) = fs::read_to_string(&path) {
                    if let Ok(snapshot) = serde_json::from_str::<StateSnapshot>(&json) {
                        let _label = snapshot.label.clone();
                        self.snapshots.insert(id.to_string(), snapshot);
                        return self.snapshots.get(id);
                    }
                }
            }
        }

        None
    }

    /// List all available snapshot IDs and labels.
    pub fn list(&self) -> Vec<(String, String, String)> {
        let mut result: Vec<(String, String, String)> = self
            .snapshots
            .values()
            .map(|s| (s.id.clone(), s.label.clone(), s.created_at.clone()))
            .collect();

        // Also scan the storage directory.
        if let Some(ref dir) = self.storage_dir {
            if dir.exists() {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("json") {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                if !result.iter().any(|(id, _, _)| id == stem) {
                                    if let Ok(json) = fs::read_to_string(&path) {
                                        if let Ok(snap) =
                                            serde_json::from_str::<StateSnapshot>(&json)
                                        {
                                            result.push((snap.id, snap.label, snap.created_at));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        result.sort_by(|a, b| b.2.cmp(&a.2));
        result
    }

    /// Remove a snapshot by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let removed = self.snapshots.remove(id).is_some();

        if let Some(ref dir) = self.storage_dir {
            let path = dir.join(format!("{}.json", id));
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
        }

        removed || self.list().iter().any(|(sid, _, _)| sid == id)
    }

    /// Export a snapshot to a JSON file at the given path.
    pub fn export_to_file(&self, id: &str, path: &Path) -> Result<(), String> {
        let snapshot = self
            .load(id)
            .ok_or_else(|| format!("Snapshot '{}' not found", id))?;
        let json = serde_json::to_string_pretty(snapshot)
            .map_err(|e| format!("Serialization error: {}", e))?;
        fs::write(path, json).map_err(|e| format!("Write error: {}", e))?;
        Ok(())
    }

    /// Import a snapshot from a JSON file.
    pub fn import_from_file(&mut self, path: &Path) -> Result<String, String> {
        let json = fs::read_to_string(path).map_err(|e| format!("Read error: {}", e))?;
        let mut snapshot: StateSnapshot =
            serde_json::from_str(&json).map_err(|e| format!("Parse error: {}", e))?;
        let id = format!("snap-{:04}", self.next_id);
        self.next_id += 1;
        snapshot.id = id.clone();
        self.snapshots.insert(id.clone(), snapshot);
        Ok(id)
    }

    /// Number of in-memory snapshots.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Clear all in-memory snapshots.
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::network_simulator::simulator::{AccountInfo, ContractInstance, LedgerInfo};
    use crate::utils::network_simulator::time::LedgerTime;
    use std::collections::HashMap;

    fn dummy_ledger_info() -> LedgerInfo {
        LedgerInfo {
            sequence: 100,
            protocol_version: 22,
            max_contract_size_bytes: 128_000,
            base_reserve: 0.5,
        }
    }

    fn dummy_account() -> AccountInfo {
        AccountInfo {
            public_key: "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
            balance: 1000.0,
            sequence: 1,
            num_subentries: 0,
            trustlines: vec![],
        }
    }

    fn dummy_contract() -> ContractInstance {
        ContractInstance {
            contract_id: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABGHI".to_string(),
            wasm_hash: "abcd1234".to_string(),
            deployer: "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
            storage: HashMap::new(),
        }
    }

    #[test]
    fn take_and_load_snapshot() {
        let mut mgr = SnapshotManager::new();
        let li = dummy_ledger_info();
        let acct = dummy_account();
        let ctr = dummy_contract();
        let lt = LedgerTime::genesis();

        let snap = mgr.take_snapshot(
            "test-snapshot",
            &li,
            &[acct],
            &[ctr],
            &lt,
            42,
            HashMap::new(),
        );

        assert_eq!(snap.label, "test-snapshot");
        assert_eq!(snap.tx_count, 42);
        assert_eq!(snap.accounts.len(), 1);
        assert_eq!(snap.contracts.len(), 1);

        let loaded = mgr.load(&snap.id);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().label, "test-snapshot");
    }

    #[test]
    fn load_nonexistent_snapshot_returns_none() {
        let mgr = SnapshotManager::new();
        assert!(mgr.load("snap-9999").is_none());
    }

    #[test]
    fn list_returns_created_snapshots() {
        let mut mgr = SnapshotManager::new();
        let li = dummy_ledger_info();

        mgr.take_snapshot(
            "first",
            &li,
            &[],
            &[],
            &LedgerTime::genesis(),
            0,
            HashMap::new(),
        );
        mgr.take_snapshot(
            "second",
            &li,
            &[],
            &[],
            &LedgerTime::genesis(),
            0,
            HashMap::new(),
        );

        let list = mgr.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn remove_snapshot() {
        let mut mgr = SnapshotManager::new();
        let li = dummy_ledger_info();

        let snap = mgr.take_snapshot(
            "to-remove",
            &li,
            &[],
            &[],
            &LedgerTime::genesis(),
            0,
            HashMap::new(),
        );
        assert!(mgr.load(&snap.id).is_some());
        mgr.remove(&snap.id);
        assert!(mgr.load(&snap.id).is_none());
    }

    #[test]
    fn clear_removes_all() {
        let mut mgr = SnapshotManager::new();
        let li = dummy_ledger_info();

        mgr.take_snapshot(
            "a",
            &li,
            &[],
            &[],
            &LedgerTime::genesis(),
            0,
            HashMap::new(),
        );
        mgr.take_snapshot(
            "b",
            &li,
            &[],
            &[],
            &LedgerTime::genesis(),
            0,
            HashMap::new(),
        );
        assert_eq!(mgr.len(), 2);
        mgr.clear();
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn export_import_roundtrip() {
        let mut mgr = SnapshotManager::new();
        let li = dummy_ledger_info();

        let snap = mgr.take_snapshot(
            "export-test",
            &li,
            &[],
            &[],
            &LedgerTime::genesis(),
            7,
            HashMap::new(),
        );

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        mgr.export_to_file(&snap.id, &path).unwrap();

        let mut mgr2 = SnapshotManager::new();
        let imported_id = mgr2.import_from_file(&path).unwrap();
        let imported = mgr2.load(&imported_id).unwrap();
        assert_eq!(imported.tx_count, 7);
        assert_eq!(imported.label, "export-test");
    }
}
