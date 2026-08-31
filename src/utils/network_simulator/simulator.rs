//! # Core Network Simulator
//!
//! An in-process Stellar/Soroban network simulator that mimics the
//! Soroban JSON-RPC interface for testing contracts without a live network.

use crate::utils::network_simulator::deterministic::{
    derive_contract_id, derive_public_key, derive_tx_hash, DeterministicConfig, SeededRng,
};
use crate::utils::network_simulator::failure::{failure_to_rpc_error, FailureInjector};
use crate::utils::network_simulator::state::SnapshotManager;
use crate::utils::network_simulator::time::TimeController;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;

// ── Mode ──────────────────────────────────────────────────────────────────────

/// Operating mode for the simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulatorMode {
    /// Standalone in-process mode (no network listener).
    InProcess,
    /// Local JSON-RPC server mode.
    RpcServer,
}

// ── Top-level types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerInfo {
    pub sequence: u32,
    pub protocol_version: u32,
    pub max_contract_size_bytes: u64,
    pub base_reserve: f64,
}

impl Default for LedgerInfo {
    fn default() -> Self {
        Self {
            sequence: 1,
            protocol_version: 22,
            max_contract_size_bytes: 128_000,
            base_reserve: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub public_key: String,
    pub balance: f64,
    pub sequence: u64,
    pub num_subentries: u32,
    pub trustlines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInstance {
    pub contract_id: String,
    pub wasm_hash: String,
    pub deployer: String,
    pub storage: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    pub hash: String,
    pub status: String,
    pub ledger: u32,
    pub contract_id: String,
    pub function: String,
    pub return_value: String,
    pub fee_stroops: u64,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationOutcome {
    pub return_value: String,
    pub fee_stroops: u64,
    pub events: Vec<String>,
    pub ledger: u32,
    pub success: bool,
    pub error: Option<String>,
}

// ── Simulator Configuration ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorConfig {
    pub mode: SimulatorMode,
    pub rpc_bind_addr: Option<SocketAddr>,
    pub deterministic: DeterministicConfig,
    pub initial_ledger_sequence: u32,
    pub protocol_version: u32,
    pub max_contract_size: u64,
    pub base_reserve: f64,
    pub initial_accounts: Vec<(String, f64)>, // (public_key / name, balance)
    pub enable_failure_injection: bool,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            mode: SimulatorMode::InProcess,
            rpc_bind_addr: Some(SocketAddr::from(([127, 0, 0, 1], 0))),
            deterministic: DeterministicConfig::default(),
            initial_ledger_sequence: 1,
            protocol_version: 22,
            max_contract_size: 128_000,
            base_reserve: 0.5,
            initial_accounts: Vec::new(),
            enable_failure_injection: false,
        }
    }
}

// ── Network Simulator ─────────────────────────────────────────────────────────

/// The main network simulator.
///
/// Manages accounts, ledgers, contracts, and provides an in-process
/// Soroban RPC-compatible interface for testing.
#[derive(Debug)]
pub struct NetworkSimulator {
    /// Simulator configuration.
    pub config: SimulatorConfig,
    /// Current ledger information.
    pub ledger: LedgerInfo,
    /// In-memory accounts (public_key → info).
    pub accounts: HashMap<String, AccountInfo>,
    /// In-memory contract instances (contract_id → info).
    pub contracts: HashMap<String, ContractInstance>,
    /// Deterministic RNG (seeded for reproducibility).
    pub rng: SeededRng,
    /// Time controller.
    pub time_controller: TimeController,
    /// Snapshot manager.
    pub snapshot_manager: SnapshotManager,
    /// Failure injector.
    pub failure_injector: FailureInjector,
    /// Transaction counter.
    tx_nonce: u64,
    /// Historical transaction receipts.
    pub tx_history: Vec<TransactionReceipt>,
    /// Simulated WASM store (wasm_hash → bytes).
    wasm_store: HashMap<String, Vec<u8>>,
}

impl NetworkSimulator {
    /// Create a new simulator with default configuration.
    pub fn new() -> Self {
        let config = SimulatorConfig::default();
        let seed = config.deterministic.seed;
        Self {
            ledger: LedgerInfo {
                sequence: config.initial_ledger_sequence,
                protocol_version: config.protocol_version,
                max_contract_size_bytes: config.max_contract_size,
                base_reserve: config.base_reserve,
            },
            accounts: HashMap::new(),
            contracts: HashMap::new(),
            rng: SeededRng::new(seed),
            time_controller: TimeController::new(),
            snapshot_manager: SnapshotManager::new(),
            failure_injector: FailureInjector::new(),
            tx_nonce: 0,
            tx_history: Vec::new(),
            wasm_store: HashMap::new(),
            config,
        }
    }

    /// Create a simulator with a custom configuration.
    pub fn with_config(config: SimulatorConfig) -> Self {
        let seed = config.deterministic.seed;
        let initial_accounts = config.initial_accounts.clone();
        let enable_failure_injection = config.enable_failure_injection;
        let mut sim = Self {
            ledger: LedgerInfo {
                sequence: config.initial_ledger_sequence,
                protocol_version: config.protocol_version,
                max_contract_size_bytes: config.max_contract_size,
                base_reserve: config.base_reserve,
            },
            accounts: HashMap::new(),
            contracts: HashMap::new(),
            rng: SeededRng::new(seed),
            time_controller: TimeController::new(),
            snapshot_manager: SnapshotManager::new(),
            failure_injector: FailureInjector::new(),
            tx_nonce: 0,
            tx_history: Vec::new(),
            wasm_store: HashMap::new(),
            config: config.clone(),
        };

        if enable_failure_injection {
            sim.failure_injector.enable();
        }

        // Create initial accounts.
        for (key_or_name, balance) in &initial_accounts {
            let pk = if key_or_name.starts_with('G') && key_or_name.len() == 56 {
                key_or_name.clone()
            } else {
                derive_public_key(seed, sim.accounts.len() as u32)
            };
            sim.accounts.insert(
                pk.clone(),
                AccountInfo {
                    public_key: pk,
                    balance: *balance,
                    sequence: 1,
                    num_subentries: 0,
                    trustlines: Vec::new(),
                },
            );
        }

        sim
    }

    /// Set a deterministic seed (convenience builder method).
    pub fn with_deterministic_seed(mut self, seed: u64) -> Self {
        self.config.deterministic.seed = seed;
        self.rng = SeededRng::new(seed);
        self
    }

    /// Enable failure injection (convenience builder method).
    pub fn with_failure_injection(mut self) -> Self {
        self.config.enable_failure_injection = true;
        self.failure_injector.enable();
        self
    }

    /// Start the simulator in its current mode.
    ///
    /// In **in-process** mode this initialises the deterministic RNG and
    /// readies the simulator for use without starting any network listener.
    /// In **RPC server** mode a future implementation would start a JSON-RPC
    /// server on the configured bind address.
    pub fn start(&mut self) {
        // In in-process mode, reset the RNG so execution is reproducible.
        self.rng.reset();
    }

    /// Reset the simulator to its initial state.
    pub fn reset(&mut self) {
        let config = self.config.clone();
        let seed = config.deterministic.seed;
        self.ledger = LedgerInfo {
            sequence: config.initial_ledger_sequence,
            protocol_version: config.protocol_version,
            max_contract_size_bytes: config.max_contract_size,
            base_reserve: config.base_reserve,
        };
        self.accounts.clear();
        self.contracts.clear();
        self.rng = SeededRng::new(seed);
        self.time_controller.reset();
        self.snapshot_manager.clear();
        self.failure_injector.clear_rules();
        self.tx_nonce = 0;
        self.tx_history.clear();
        self.wasm_store.clear();

        // Re-create initial accounts.
        for (key_or_name, balance) in &config.initial_accounts {
            let pk = if key_or_name.starts_with('G') && key_or_name.len() == 56 {
                key_or_name.clone()
            } else {
                derive_public_key(seed, self.accounts.len() as u32)
            };
            self.accounts.insert(
                pk.clone(),
                AccountInfo {
                    public_key: pk,
                    balance: *balance,
                    sequence: 1,
                    num_subentries: 0,
                    trustlines: Vec::new(),
                },
            );
        }

        if config.enable_failure_injection {
            self.failure_injector.enable();
        }
    }

    // ── Ledger ────────────────────────────────────────────────────────────────

    /// Advance the ledger by one sequence.
    pub fn advance_ledger(&mut self) {
        self.ledger.sequence += 1;
        self.time_controller.tick();
    }

    /// Advance the ledger by `n` sequences.
    pub fn advance_ledgers(&mut self, n: u32) {
        for _ in 0..n {
            self.advance_ledger();
        }
    }

    /// Get the current ledger sequence.
    pub fn current_ledger(&self) -> u32 {
        self.ledger.sequence
    }

    // ── Accounts ──────────────────────────────────────────────────────────────

    /// Create an account with a deterministic public key and initial balance.
    pub fn create_account(&mut self, balance: f64) -> AccountInfo {
        let seed = self.config.deterministic.seed;
        let index = self.accounts.len() as u32;
        let pk = derive_public_key(seed, index);
        let account = AccountInfo {
            public_key: pk.clone(),
            balance,
            sequence: 1,
            num_subentries: 0,
            trustlines: Vec::new(),
        };
        self.accounts.insert(pk, account.clone());
        account
    }

    /// Create an account with a specific public key.
    pub fn create_account_with_key(&mut self, public_key: &str, balance: f64) -> AccountInfo {
        let account = AccountInfo {
            public_key: public_key.to_string(),
            balance,
            sequence: 1,
            num_subentries: 0,
            trustlines: Vec::new(),
        };
        self.accounts
            .insert(public_key.to_string(), account.clone());
        account
    }

    /// Get account info by public key.
    pub fn get_account(&self, public_key: &str) -> Option<&AccountInfo> {
        self.accounts.get(public_key)
    }

    /// Get mutable account info by public key.
    pub fn get_account_mut(&mut self, public_key: &str) -> Option<&mut AccountInfo> {
        self.accounts.get_mut(public_key)
    }

    /// Fund an account with XLM.
    pub fn fund_account(&mut self, public_key: &str, amount: f64) -> Result<(), String> {
        let account = self
            .accounts
            .get_mut(public_key)
            .ok_or_else(|| format!("Account '{}' not found", public_key))?;
        account.balance += amount;
        Ok(())
    }

    /// Deduct from an account balance.
    pub fn deduct_balance(&mut self, public_key: &str, amount: f64) -> Result<(), String> {
        let account = self
            .accounts
            .get_mut(public_key)
            .ok_or_else(|| format!("Account '{}' not found", public_key))?;
        if account.balance < amount {
            return Err(format!(
                "Insufficient balance: have {}, need {}",
                account.balance, amount
            ));
        }
        account.balance -= amount;
        account.sequence += 1;
        Ok(())
    }

    /// List all accounts.
    pub fn list_accounts(&self) -> Vec<&AccountInfo> {
        self.accounts.values().collect()
    }

    // ── WASM / Contracts ──────────────────────────────────────────────────────

    /// Upload a WASM binary and return its SHA-256 hash.
    pub fn upload_wasm(&mut self, bytes: &[u8]) -> String {
        let hash = hex::encode(Sha256::digest(bytes));
        self.wasm_store.insert(hash.clone(), bytes.to_vec());
        hash
    }

    /// Deploy a contract from an already-uploaded WASM hash.
    pub fn deploy_contract(
        &mut self,
        wasm_hash: &str,
        deployer: &str,
    ) -> Result<ContractInstance, String> {
        if !self.wasm_store.contains_key(wasm_hash) {
            // Auto-register the WASM if not found (simplifies testing).
            self.wasm_store
                .insert(wasm_hash.to_string(), vec![0, 0x61, 0x73, 0x6d]); // minimal WASM magic
        }

        let seed = self.config.deterministic.seed;
        let index = self.contracts.len() as u32;
        let contract_id = derive_contract_id(seed, wasm_hash, index);

        let instance = ContractInstance {
            contract_id: contract_id.clone(),
            wasm_hash: wasm_hash.to_string(),
            deployer: deployer.to_string(),
            storage: HashMap::new(),
        };

        self.contracts.insert(contract_id, instance.clone());
        self.advance_ledger();
        Ok(instance)
    }

    /// Get a contract instance by ID.
    pub fn get_contract(&self, contract_id: &str) -> Option<&ContractInstance> {
        self.contracts.get(contract_id)
    }

    /// Get mutable contract instance by ID.
    pub fn get_contract_mut(&mut self, contract_id: &str) -> Option<&mut ContractInstance> {
        self.contracts.get_mut(contract_id)
    }

    /// List all deployed contracts.
    pub fn list_contracts(&self) -> Vec<&ContractInstance> {
        self.contracts.values().collect()
    }

    /// Read a storage value from a contract.
    pub fn read_contract_storage(&self, contract_id: &str, key: &str) -> Option<&String> {
        self.contracts
            .get(contract_id)
            .and_then(|c| c.storage.get(key))
    }

    /// Write a storage value to a contract.
    pub fn write_contract_storage(
        &mut self,
        contract_id: &str,
        key: &str,
        value: String,
    ) -> Result<(), String> {
        let contract = self
            .contracts
            .get_mut(contract_id)
            .ok_or_else(|| format!("Contract '{}' not found", contract_id))?;
        contract.storage.insert(key.to_string(), value);
        Ok(())
    }

    // ── Transaction Simulation ────────────────────────────────────────────────

    /// Simulate a contract invocation (does not modify state).
    pub fn simulate_invoke(
        &mut self,
        contract_id: &str,
        function: &str,
        args: &[String],
        source_account: &str,
    ) -> Result<SimulationOutcome, String> {
        // Check failure injection.
        let prob = self.rng.probability();
        if let Some(mode) = self.failure_injector.check(
            "simulateTransaction",
            Some(contract_id),
            Some(source_account),
            prob,
        ) {
            let (code, message) = failure_to_rpc_error(&mode);
            return Err(format!("RPC error {}: {}", code, message));
        }

        // Validate contract exists.
        let _contract = self
            .contracts
            .get(contract_id)
            .ok_or_else(|| format!("Contract '{}' not found", contract_id))?;

        // Validate account exists.
        let _account = self
            .accounts
            .get(source_account)
            .ok_or_else(|| format!("Account '{}' not found", source_account))?;

        // Simulated execution: generate deterministic return.
        let return_value = if self.config.deterministic.deterministic_execution {
            self.simulated_deterministic_return(contract_id, function, args)
        } else {
            format!(
                "0x{}",
                hex::encode(Sha256::digest(
                    format!("{}{}{:?}", contract_id, function, args).as_bytes()
                ))
            )
        };

        // Simulated fee estimation.
        let fee_stroops = 100_000 + (args.len() as u64 * 10_000);

        // Simulated events.
        let events = vec![format!(
            "Contract({})::{}() executed",
            &contract_id[..8],
            function
        )];

        Ok(SimulationOutcome {
            return_value,
            fee_stroops,
            events,
            ledger: self.ledger.sequence,
            success: true,
            error: None,
        })
    }

    /// Submit a contract invocation (modifies state, advances ledger).
    pub fn submit_invoke(
        &mut self,
        contract_id: &str,
        function: &str,
        args: &[String],
        source_account: &str,
        fee_stroops: u64,
    ) -> Result<TransactionReceipt, String> {
        // Check failure injection.
        let prob = self.rng.probability();
        if let Some(mode) = self.failure_injector.check(
            "sendTransaction",
            Some(contract_id),
            Some(source_account),
            prob,
        ) {
            let (code, message) = failure_to_rpc_error(&mode);
            return Err(format!("RPC error {}: {}", code, message));
        }

        // First simulate.
        let sim = self.simulate_invoke(contract_id, function, args, source_account)?;

        // Deduct fee from account.
        self.deduct_balance(source_account, fee_stroops as f64 / 10_000_000.0)?;

        // Advance ledger.
        self.advance_ledger();

        // Generate transaction receipt.
        self.tx_nonce += 1;
        let tx_hash = derive_tx_hash(self.config.deterministic.seed, self.tx_nonce);

        let receipt = TransactionReceipt {
            hash: tx_hash.clone(),
            status: "success".to_string(),
            ledger: self.ledger.sequence,
            contract_id: contract_id.to_string(),
            function: function.to_string(),
            return_value: sim.return_value,
            fee_stroops,
            events: sim.events,
        };

        self.tx_history.push(receipt.clone());
        Ok(receipt)
    }

    /// Get a transaction receipt by hash.
    pub fn get_transaction(&self, hash: &str) -> Option<&TransactionReceipt> {
        self.tx_history.iter().find(|tx| tx.hash == hash)
    }

    /// List all transaction receipts.
    pub fn list_transactions(&self) -> &[TransactionReceipt] {
        &self.tx_history
    }

    /// Get the total number of transactions.
    pub fn tx_count(&self) -> u64 {
        self.tx_history.len() as u64
    }

    // ── State Snapshots ───────────────────────────────────────────────────────

    /// Take a full state snapshot.
    pub fn take_snapshot(&mut self, label: &str) -> String {
        let accounts: Vec<AccountInfo> = self.accounts.values().cloned().collect();
        let contracts: Vec<ContractInstance> = self.contracts.values().cloned().collect();
        let metadata = HashMap::from([
            ("accounts".to_string(), accounts.len().to_string()),
            ("contracts".to_string(), contracts.len().to_string()),
            ("tx_count".to_string(), self.tx_count().to_string()),
        ]);

        let snap = self.snapshot_manager.take_snapshot(
            label,
            &self.ledger,
            &accounts,
            &contracts,
            &self.time_controller.ledger_time,
            self.tx_count(),
            metadata,
        );
        snap.id
    }

    /// Restore from a snapshot by ID.
    pub fn restore_snapshot(&mut self, id: &str) -> Result<(), String> {
        let snapshot = self
            .snapshot_manager
            .load(id)
            .ok_or_else(|| format!("Snapshot '{}' not found", id))?;

        self.ledger = snapshot.ledger_info.clone();
        self.time_controller.ledger_time = snapshot.ledger_time;
        self.tx_nonce = snapshot.tx_count;

        // Restore accounts.
        self.accounts.clear();
        for account in &snapshot.accounts {
            self.accounts
                .insert(account.public_key.clone(), account.clone());
        }

        // Restore contracts.
        self.contracts.clear();
        for contract in &snapshot.contracts {
            self.contracts
                .insert(contract.contract_id.clone(), contract.clone());
        }

        Ok(())
    }

    // ── Health / Status ───────────────────────────────────────────────────────

    /// Get health status.
    pub fn get_health(&self) -> String {
        "healthy".to_string()
    }

    /// Get the latest ledger info (Soroban RPC compatible).
    pub fn get_latest_ledger(&self) -> serde_json::Value {
        serde_json::json!({
            "id": format!("{:x}", self.ledger.sequence),
            "protocolVersion": self.ledger.protocol_version,
            "sequence": self.ledger.sequence,
            "maxContractSizeBytes": self.ledger.max_contract_size_bytes,
            "baseReserve": self.ledger.base_reserve,
            "timestamp": self.time_controller.ledger_time.timestamp,
        })
    }

    /// Get simulator status as a JSON value.
    pub fn get_status(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": match self.config.mode {
                SimulatorMode::InProcess => "in-process",
                SimulatorMode::RpcServer => "rpc-server",
            },
            "ledger": {
                "sequence": self.ledger.sequence,
                "protocol_version": self.ledger.protocol_version,
            },
            "accounts": self.accounts.len(),
            "contracts": self.contracts.len(),
            "transactions": self.tx_history.len(),
            "time": {
                "sequence": self.time_controller.ledger_time.sequence,
                "timestamp": self.time_controller.ledger_time.timestamp,
                "frozen": self.time_controller.ledger_time.frozen,
            },
            "failure_injection": self.failure_injector.enabled,
            "seed": self.config.deterministic.seed,
        })
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn simulated_deterministic_return(
        &self,
        contract_id: &str,
        function: &str,
        args: &[String],
    ) -> String {
        // Generate a deterministic return value based on inputs.
        let mut hasher = Sha256::new();
        hasher.update(b"starforge-sim-return");
        hasher.update(contract_id.as_bytes());
        hasher.update(function.as_bytes());
        for arg in args {
            hasher.update(arg.as_bytes());
        }
        hasher.update(self.ledger.sequence.to_le_bytes());
        let hash = hasher.finalize();

        match function {
            "balance" | "getBalance" | "get_balance" => {
                // Look up a simulated balance from storage.
                if let Some(contract) = self.contracts.get(contract_id) {
                    if let Some(balance) = contract.storage.get("balance") {
                        return balance.clone();
                    }
                }
                format!("{}", (hash[0] as u64) * 1_000_000)
            }
            "symbol" | "name" | "decimals" => match function {
                "symbol" => "\"SIM\"".to_string(),
                "name" => "\"Simulator Token\"".to_string(),
                "decimals" => "7".to_string(),
                _ => "\"unknown\"".to_string(),
            },
            "total_supply" | "totalSupply" => {
                format!("{}", (hash[0] as u64 + hash[1] as u64) * 100_000)
            }
            _ => format!("0x{}", hex::encode(&hash[..8])),
        }
    }
}

impl Default for NetworkSimulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::network_simulator::failure::FailureMode;

    #[test]
    fn new_simulator_has_default_state() {
        let sim = NetworkSimulator::new();
        assert_eq!(sim.ledger.sequence, 1);
        assert!(sim.accounts.is_empty());
        assert!(sim.contracts.is_empty());
        assert_eq!(sim.tx_history.len(), 0);
    }

    #[test]
    fn create_account_adds_to_state() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        assert_eq!(account.balance, 1000.0);
        assert!(account.public_key.starts_with('G'));
        assert_eq!(account.public_key.len(), 56);
        assert_eq!(sim.accounts.len(), 1);
    }

    #[test]
    fn fund_account_increases_balance() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(100.0);
        sim.fund_account(&account.public_key, 50.0).unwrap();
        assert_eq!(sim.get_account(&account.public_key).unwrap().balance, 150.0);
    }

    #[test]
    fn upload_and_deploy_contract() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic
        let wasm_hash = sim.upload_wasm(&wasm_bytes);
        assert_eq!(wasm_hash.len(), 64); // SHA-256 hex

        let contract = sim
            .deploy_contract(&wasm_hash, &account.public_key)
            .unwrap();
        assert!(contract.contract_id.starts_with('C'));
        assert_eq!(contract.contract_id.len(), 56);
        assert_eq!(sim.contracts.len(), 1);
    }

    #[test]
    fn deploy_contract_auto_registers_wasm() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        let contract = sim
            .deploy_contract("fake_hash_123", &account.public_key)
            .unwrap();
        assert!(contract.contract_id.starts_with('C'));
    }

    #[test]
    fn contract_storage_read_write() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        let contract = sim
            .deploy_contract("wasm_hash", &account.public_key)
            .unwrap();
        let cid = &contract.contract_id;

        sim.write_contract_storage(cid, "counter", "42".to_string())
            .unwrap();
        assert_eq!(
            sim.read_contract_storage(cid, "counter"),
            Some(&"42".to_string())
        );
    }

    #[test]
    fn simulate_invoke_returns_expected() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        let contract = sim.deploy_contract("wh", &account.public_key).unwrap();

        let result = sim
            .simulate_invoke(&contract.contract_id, "increment", &[], &account.public_key)
            .unwrap();
        assert!(result.success);
        assert!(result.fee_stroops > 0);
        assert!(result.return_value.starts_with("0x"));
    }

    #[test]
    fn submit_invoke_creates_receipt() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        let contract = sim.deploy_contract("wh", &account.public_key).unwrap();
        let cid = &contract.contract_id;

        let receipt = sim
            .submit_invoke(cid, "increment", &[], &account.public_key, 100_000)
            .unwrap();
        assert_eq!(receipt.status, "success");
        assert_eq!(&receipt.contract_id, cid);
        assert_eq!(receipt.function, "increment");
        assert_eq!(sim.tx_history.len(), 1);
    }

    #[test]
    fn submit_invoke_reduces_balance() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(1000.0);
        let contract = sim.deploy_contract("wh", &account.public_key).unwrap();
        let pk = &account.public_key;

        let balance_before = sim.get_account(pk).unwrap().balance;
        sim.submit_invoke(&contract.contract_id, "increment", &[], pk, 100_000)
            .unwrap();
        let balance_after = sim.get_account(pk).unwrap().balance;
        assert!(balance_after < balance_before);
    }

    #[test]
    fn simulate_invoke_deterministic_reproducibility() {
        let (r1, r2) = {
            let mut sim1 = NetworkSimulator::new().with_deterministic_seed(42);
            let acct1 = sim1.create_account(1000.0);
            let c1 = sim1.deploy_contract("wh", &acct1.public_key).unwrap();
            let r1 = sim1
                .simulate_invoke(&c1.contract_id, "ping", &[], &acct1.public_key)
                .unwrap()
                .return_value;

            let mut sim2 = NetworkSimulator::new().with_deterministic_seed(42);
            let acct2 = sim2.create_account(1000.0);
            let c2 = sim2.deploy_contract("wh", &acct2.public_key).unwrap();
            let r2 = sim2
                .simulate_invoke(&c2.contract_id, "ping", &[], &acct2.public_key)
                .unwrap()
                .return_value;
            (r1, r2)
        };
        assert_eq!(r1, r2);
    }

    #[test]
    fn take_and_restore_snapshot() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(100.0);
        let contract = sim.deploy_contract("wh", &account.public_key).unwrap();
        sim.write_contract_storage(&contract.contract_id, "key", "value".to_string())
            .unwrap();

        let snap_id = sim.take_snapshot("before-operation");
        assert!(!snap_id.is_empty());

        // Mutate state.
        sim.advance_ledgers(10);
        sim.deduct_balance(&account.public_key, 10.0).unwrap();

        // Restore.
        sim.restore_snapshot(&snap_id).unwrap();
        assert_eq!(sim.get_account(&account.public_key).unwrap().balance, 100.0);
        assert_eq!(
            sim.read_contract_storage(&contract.contract_id, "key"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn failure_injection_in_process() {
        let mut sim = NetworkSimulator::new().with_failure_injection();
        let account = sim.create_account(1000.0);
        let contract = sim.deploy_contract("wh", &account.public_key).unwrap();

        // Add a failure rule that always fires.
        sim.failure_injector
            .add_rule(crate::utils::network_simulator::failure::FailureRule::new(
                "always-fail",
                FailureMode::InsufficientFee,
            ));

        let result = sim.simulate_invoke(&contract.contract_id, "test", &[], &account.public_key);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Insufficient fee"));
    }

    #[test]
    fn snapshot_determinism() {
        // Two runs with same seed produce same snapshot state.
        let check = |seed: u64| -> (u32, usize, usize) {
            let mut sim = NetworkSimulator::new().with_deterministic_seed(seed);
            sim.create_account(500.0);
            let pk = sim.list_accounts()[0].public_key.clone();
            sim.deploy_contract("wh", &pk).unwrap();
            let snap_id = sim.take_snapshot("check");
            let snap = sim.snapshot_manager.load(&snap_id).unwrap().clone();
            (
                snap.ledger_info.sequence,
                snap.accounts.len(),
                snap.contracts.len(),
            )
        };

        assert_eq!(check(42), check(42));
    }

    #[test]
    fn get_health_returns_healthy() {
        let sim = NetworkSimulator::new();
        assert_eq!(sim.get_health(), "healthy");
    }

    #[test]
    fn get_latest_ledger_returns_json() {
        let sim = NetworkSimulator::new();
        let ledger = sim.get_latest_ledger();
        assert_eq!(ledger["sequence"], 1);
        assert_eq!(ledger["protocolVersion"], 22);
    }

    #[test]
    fn fund_nonexistent_account_fails() {
        let mut sim = NetworkSimulator::new();
        let result = sim.fund_account("G_NONEXISTENT", 100.0);
        assert!(result.is_err());
    }

    #[test]
    fn insufficient_balance_returns_error() {
        let mut sim = NetworkSimulator::new();
        let account = sim.create_account(10.0);
        let result = sim.deduct_balance(&account.public_key, 100.0);
        assert!(result.is_err());
    }

    #[test]
    fn advance_ledger_increases_sequence() {
        let mut sim = NetworkSimulator::new();
        sim.advance_ledger();
        assert_eq!(sim.current_ledger(), 2);
        sim.advance_ledgers(5);
        assert_eq!(sim.current_ledger(), 7);
    }

    #[test]
    fn get_transaction_returns_none_for_unknown() {
        let sim = NetworkSimulator::new();
        assert!(sim.get_transaction("nonexistent").is_none());
    }

    #[test]
    fn reset_clears_state() {
        let mut sim = NetworkSimulator::new();
        sim.create_account(100.0);
        let pk = sim.list_accounts()[0].public_key.clone();
        sim.deploy_contract("wh", &pk).unwrap();
        assert_eq!(sim.accounts.len(), 1);
        assert_eq!(sim.contracts.len(), 1);

        sim.reset();
        assert!(sim.accounts.is_empty());
        assert!(sim.contracts.is_empty());
        assert_eq!(sim.ledger.sequence, 1);
    }
}
