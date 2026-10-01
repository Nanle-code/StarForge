//! Compatibility facade for the former single-file simulator API.
//!
//! All account, contract, and ledger storage lives in the canonical simulator.
//! This facade preserves the old constructor and result/state shapes for callers
//! migrating to `crate::utils::network_simulator::simulator::NetworkSimulator`.

pub use super::scenarios::{SimScenario, SimScenarioResult, SimScenarioStep};
pub use super::failure::FailureMode;
use super::simulator::{ContractInstance, NetworkSimulator as Engine};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimContract {
    pub contract_id: String,
    pub wasm_hash: String,
    pub storage: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimLedgerState {
    pub ledger_sequence: u32,
    pub timestamp: u64,
    pub contracts: HashMap<String, SimContract>,
    pub accounts: HashMap<String, u64>,
    pub events: Vec<SimEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimEvent {
    pub ledger: u32,
    pub contract_id: String,
    pub topic: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimInvokeResult {
    pub return_value: String,
    pub fee: u64,
    pub events: Vec<String>,
    pub ledger_sequence: u32,
}

#[derive(Clone)]
struct NamedSnapshot {
    engine_id: String,
    events: Vec<SimEvent>,
    balances: HashMap<String, u64>,
}

/// Adapter for the former `utils::network_sim::NetworkSimulator` API.
///
/// The canonical simulator remains the only owner of accounts, contracts, and
/// ledger state. The adapter retains the legacy event and integer-balance views
/// required to preserve the previous method signatures and serialized shape.
pub struct NetworkSimulator {
    engine: Engine,
    state: SimLedgerState,
    seed: u64,
    rng_state: u64,
    failure_mode: Option<FailureMode>,
    latency_ms: u64,
    events: Vec<SimEvent>,
    balances: HashMap<String, u64>,
    snapshots: HashMap<String, NamedSnapshot>,
}

impl NetworkSimulator {
    /// Create a compatibility simulator with a deterministic seed.
    pub fn new(seed: u64) -> Self {
        let engine = Engine::new().with_deterministic_seed(seed);
        let timestamp = engine.time_controller.ledger_time.timestamp;
        Self {
            engine,
            state: SimLedgerState {
                ledger_sequence: 1,
                timestamp: u64::try_from(timestamp).unwrap_or_default(),
                contracts: HashMap::new(),
                accounts: HashMap::new(),
                events: Vec::new(),
            },
            seed,
            rng_state: seed,
            failure_mode: None,
            latency_ms: 0,
            events: Vec::new(),
            balances: HashMap::new(),
            snapshots: HashMap::new(),
        }
    }

    /// Construct the compatibility facade from the former serialized state type.
    pub fn from_state(state: SimLedgerState, seed: u64) -> Self {
        let mut simulator = Self::new(seed);
        simulator.engine.ledger.sequence = state.ledger_sequence;
        simulator.engine.time_controller.ledger_time.sequence = state.ledger_sequence;
        simulator.engine.time_controller.ledger_time.timestamp =
            i64::try_from(state.timestamp).unwrap_or(i64::MAX);
        simulator.balances = state.accounts.clone();
        for (public_key, balance) in &state.accounts {
            simulator
                .engine
                .create_account_with_key(public_key, *balance as f64);
        }
        for (contract_id, contract) in &state.contracts {
            simulator.engine.contracts.insert(
                contract_id.clone(),
                ContractInstance {
                    contract_id: contract.contract_id.clone(),
                    wasm_hash: contract.wasm_hash.clone(),
                    deployer: String::new(),
                    storage: contract.storage.clone(),
                },
            );
        }
        simulator.events = state.events.clone();
        simulator.refresh_state();
        simulator
    }

    /// Return the legacy-shaped view of the canonical simulator state.
    pub fn state(&self) -> &SimLedgerState {
        &self.state
    }

    /// Return the seed used by deterministic compatibility operations.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Set the legacy failure mode for subsequent deployments and invocations.
    pub fn set_failure_mode(&mut self, mode: FailureMode) {
        self.failure_mode = Some(mode);
    }

    /// Set a fixed delay applied to deployments and invocations.
    pub fn set_latency(&mut self, milliseconds: u64) {
        self.latency_ms = milliseconds;
    }

    /// Advance virtual time without advancing the ledger.
    pub fn advance_time(&mut self, seconds: u64) {
        self.state.timestamp += seconds;
        self.engine.time_controller.ledger_time.timestamp =
            i64::try_from(self.state.timestamp).unwrap_or(i64::MAX);
        self.refresh_state();
    }

    /// Advance the ledger sequence without changing virtual time.
    pub fn advance_ledger(&mut self, count: u32) {
        self.engine.ledger.sequence += count;
        self.engine.time_controller.ledger_time.sequence = self.engine.ledger.sequence;
        self.refresh_state();
    }

    /// Save the current canonical state under a legacy caller-provided name.
    pub fn snapshot(&mut self, name: &str) {
        let engine_id = self.engine.take_snapshot(name);
        self.snapshots.insert(
            name.to_string(),
            NamedSnapshot {
                engine_id,
                events: self.events.clone(),
                balances: self.balances.clone(),
            },
        );
    }

    /// Restore a named snapshot.
    pub fn restore(&mut self, name: &str) -> Result<()> {
        let snapshot = self
            .snapshots
            .get(name)
            .cloned()
            .with_context(|| format!("Snapshot '{}' not found", name))?;
        self.engine
            .restore_snapshot(&snapshot.engine_id)
            .map_err(anyhow::Error::msg)?;
        self.events = snapshot.events;
        self.balances = snapshot.balances;
        self.refresh_state();
        Ok(())
    }

    /// Persist the legacy-compatible state JSON shape.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, serde_json::to_string_pretty(&self.state)?)?;
        Ok(())
    }

    /// Load a legacy-compatible state JSON file.
    pub fn load_from_file(path: &Path, seed: u64) -> Result<Self> {
        let state = serde_json::from_str(&fs::read_to_string(path)?)?;
        Ok(Self::from_state(state, seed))
    }

    /// Deploy a contract with the legacy deterministic ID format.
    pub fn deploy_contract(&mut self, wasm_hash: &str) -> Result<String> {
        self.check_failure()?;
        self.simulate_latency();
        let contract_id = deterministic_contract_id(
            wasm_hash,
            self.seed,
            self.engine.ledger.sequence,
        );
        self.deploy_at_id(contract_id, wasm_hash)
    }

    /// Deploy a contract with a caller-specified ID.
    pub fn deploy_contract_with_id(&mut self, contract_id: &str, wasm_hash: &str) -> Result<()> {
        self.check_failure()?;
        self.simulate_latency();
        self.deploy_at_id(contract_id.to_string(), wasm_hash)
    }

    /// Invoke a contract using the legacy deterministic result and event format.
    pub fn invoke(
        &mut self,
        contract_id: &str,
        function: &str,
        args: &[String],
    ) -> Result<SimInvokeResult> {
        self.check_failure()?;
        self.simulate_latency();
        let contract = self
            .engine
            .contracts
            .get(contract_id)
            .with_context(|| format!("Contract '{}' not found in simulator", contract_id))?;
        let return_value = deterministic_return(function, args, &contract.wasm_hash, self.seed);
        let fee = deterministic_fee(function, args.len(), self.seed);
        self.events.push(SimEvent {
            ledger: self.engine.ledger.sequence,
            contract_id: contract_id.to_string(),
            topic: function.to_string(),
            data: args.join(","),
        });
        self.engine.ledger.sequence += 1;
        self.engine.time_controller.ledger_time.sequence = self.engine.ledger.sequence;
        self.refresh_state();
        Ok(SimInvokeResult {
            events: vec![format!("{}:{}", function, return_value)],
            return_value,
            fee,
            ledger_sequence: self.engine.ledger.sequence,
        })
    }

    /// Fund an account, creating it when it does not yet exist.
    pub fn fund_account(&mut self, address: &str, amount: u64) {
        let balance = self.balances.entry(address.to_string()).or_insert(0);
        *balance += amount;
        if let Some(account) = self.engine.accounts.get_mut(address) {
            account.balance = *balance as f64;
        } else {
            self.engine
                .create_account_with_key(address, *balance as f64);
        }
        self.refresh_state();
    }

    /// Run the existing step-oriented scenario format with legacy execution semantics.
    pub fn run_scenario(&mut self, scenario: &SimScenario) -> SimScenarioResult {
        self.seed = scenario.seed;
        self.rng_state = scenario.seed;
        self.engine.config.deterministic.seed = scenario.seed;
        self.engine.rng = super::deterministic::SeededRng::new(scenario.seed);
        self.engine.ledger.sequence = scenario.initial_ledger;
        self.engine.time_controller.ledger_time.sequence = scenario.initial_ledger;
        self.failure_mode = None;

        let mut errors = Vec::new();
        let mut steps_run = 0;
        for step in &scenario.steps {
            steps_run += 1;
            if let Err(error) = self.execute_step(step) {
                errors.push(format!("Step {}: {}", steps_run, error));
                break;
            }
        }
        self.refresh_state();
        SimScenarioResult {
            scenario: scenario.name.clone(),
            passed: errors.is_empty(),
            steps_run,
            steps_total: scenario.steps.len(),
            errors,
            final_ledger: self.engine.ledger.sequence,
        }
    }

    fn deploy_at_id(&mut self, contract_id: String, wasm_hash: &str) -> Result<()> {
        let timestamp = self.engine.time_controller.ledger_time.timestamp;
        self.engine
            .deploy_contract_with_id(&contract_id, wasm_hash, "")
            .map_err(anyhow::Error::msg)?;
        self.engine.time_controller.ledger_time.timestamp = timestamp;
        self.refresh_state();
        Ok(())
    }

    fn execute_step(&mut self, step: &SimScenarioStep) -> Result<()> {
        match step {
            SimScenarioStep::Deploy {
                contract_id,
                wasm_hash,
            } => self.deploy_contract_with_id(contract_id, wasm_hash),
            SimScenarioStep::Invoke {
                contract_id,
                function,
                args,
                expected_return,
            } => {
                let result = self.invoke(contract_id, function, args)?;
                if let Some(expected) = expected_return {
                    if result.return_value != *expected {
                        anyhow::bail!("Expected return '{}', got '{}'", expected, result.return_value);
                    }
                }
                Ok(())
            }
            SimScenarioStep::AdvanceTime { seconds } => {
                self.advance_time(*seconds);
                Ok(())
            }
            SimScenarioStep::AdvanceLedger { count } => {
                self.advance_ledger(*count);
                Ok(())
            }
            SimScenarioStep::InjectFailure { mode } => {
                self.failure_mode = match mode {
                    super::scenarios::ScriptFailureMode::None => None,
                    super::scenarios::ScriptFailureMode::RpcTimeout => {
                        Some(FailureMode::RpcTimeout)
                    }
                    super::scenarios::ScriptFailureMode::RpcError => {
                        Some(FailureMode::RpcError { code: -32603 })
                    }
                    super::scenarios::ScriptFailureMode::InsufficientFee => {
                        Some(FailureMode::InsufficientFee)
                    }
                    super::scenarios::ScriptFailureMode::ContractNotFound => {
                        Some(FailureMode::ContractNotFound)
                    }
                    super::scenarios::ScriptFailureMode::Random { probability_pct } => {
                        Some(FailureMode::RandomFailure(
                            *probability_pct as f64 / 100.0,
                        ))
                    }
                };
                Ok(())
            }
            SimScenarioStep::Snapshot { name } => {
                self.snapshot(name);
                Ok(())
            }
            SimScenarioStep::Restore { name } => self.restore(name),
            SimScenarioStep::FundAccount { address, amount } => {
                self.fund_account(address, *amount);
                Ok(())
            }
        }
    }

    fn check_failure(&mut self) -> Result<()> {
        match self.failure_mode.clone() {
            None => Ok(()),
            Some(FailureMode::RpcTimeout) => {
                anyhow::bail!("Simulated RPC timeout (injected failure)")
            }
            Some(FailureMode::RpcError { code }) => {
                anyhow::bail!("Simulated RPC error: {} internal error (injected failure)", code)
            }
            Some(FailureMode::InsufficientFee) => {
                anyhow::bail!("Simulated insufficient fee error (injected failure)")
            }
            Some(FailureMode::ContractNotFound) => {
                anyhow::bail!("Simulated contract not found error (injected failure)")
            }
            Some(FailureMode::RandomFailure(probability)) => {
                let roll = self.next_random() % 100;
                if (roll as f64) < probability * 100.0 {
                    anyhow::bail!(
                        "Simulated random failure ({}% probability, roll={})",
                        probability * 100.0,
                        roll
                    );
                }
                Ok(())
            }
            Some(mode) => {
                let (code, message) = super::failure::failure_to_rpc_error(&mode);
                anyhow::bail!("RPC error {}: {} (injected failure)", code, message)
            }
        }
    }

    fn next_random(&mut self) -> u64 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        self.rng_state
    }

    fn simulate_latency(&self) {
        if self.latency_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.latency_ms));
        }
    }

    fn refresh_state(&mut self) {
        self.state = SimLedgerState {
            ledger_sequence: self.engine.ledger.sequence,
            timestamp: u64::try_from(self.engine.time_controller.ledger_time.timestamp)
                .unwrap_or_default(),
            contracts: self
                .engine
                .contracts
                .iter()
                .map(|(id, contract)| {
                    (
                        id.clone(),
                        SimContract {
                            contract_id: contract.contract_id.clone(),
                            wasm_hash: contract.wasm_hash.clone(),
                            storage: contract.storage.clone(),
                        },
                    )
                })
                .collect(),
            accounts: self
                .engine
                .accounts
                .iter()
                .map(|(address, account)| {
                    (
                        address.clone(),
                        self.balances
                            .get(address)
                            .copied()
                            .unwrap_or_else(|| account.balance.max(0.0) as u64),
                    )
                })
                .collect(),
            events: self.events.clone(),
        };
    }
}

fn deterministic_contract_id(wasm_hash: &str, seed: u64, ledger: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(wasm_hash.as_bytes());
    hasher.update(seed.to_le_bytes());
    hasher.update(ledger.to_le_bytes());
    let hash = hasher.finalize();
    format!("C{}", hex::encode(&hash[..32]))
}

fn deterministic_return(function: &str, args: &[String], wasm_hash: &str, seed: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(function.as_bytes());
    for arg in args {
        hasher.update(arg.as_bytes());
    }
    hasher.update(wasm_hash.as_bytes());
    hasher.update(seed.to_le_bytes());
    hex::encode(&hasher.finalize()[..8])
}

fn deterministic_fee(function: &str, arg_count: usize, seed: u64) -> u64 {
    10_000 + function.len() as u64 * 100 + arg_count as u64 * 500 + (seed % 1000) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_deployment_repeats_for_same_seed_and_differs_across_seeds() {
        let mut first = NetworkSimulator::new(42);
        let mut same_seed = NetworkSimulator::new(42);
        let mut other_seed = NetworkSimulator::new(43);
        let first_id = first.deploy_contract("hash123").unwrap();
        assert_eq!(first_id, same_seed.deploy_contract("hash123").unwrap());
        assert_ne!(first_id, other_seed.deploy_contract("hash123").unwrap());
    }

    #[test]
    fn named_snapshot_restore_preserves_legacy_state() {
        let mut simulator = NetworkSimulator::new(42);
        simulator
            .deploy_contract_with_id("C_TEST", "hash")
            .unwrap();
        simulator.fund_account("GACC", 1000);
        simulator.snapshot("checkpoint");
        simulator.fund_account("GACC", 5000);
        simulator.restore("checkpoint").unwrap();
        assert_eq!(simulator.state().accounts.get("GACC"), Some(&1000));
        assert!(simulator.state().contracts.contains_key("C_TEST"));
    }

    #[test]
    fn failure_injection_blocks_deployment() {
        let mut simulator = NetworkSimulator::new(42);
        simulator.set_failure_mode(FailureMode::RpcTimeout);
        assert!(simulator
            .deploy_contract("hash")
            .unwrap_err()
            .to_string()
            .contains("timeout"));
    }

    #[test]
    fn scripted_scenario_runs_successfully() {
        let scenario = super::super::scenarios::builtin_scenarios().remove(0);
        let mut simulator = NetworkSimulator::new(scenario.seed);
        let result = simulator.run_scenario(&scenario);
        assert!(result.passed, "errors: {:?}", result.errors);
        assert_eq!(result.steps_run, scenario.steps.len());
    }

    #[test]
    fn ledger_and_time_advance_independently() {
        let mut simulator = NetworkSimulator::new(1);
        let timestamp = simulator.state().timestamp;
        simulator.advance_time(60);
        simulator.advance_ledger(10);
        assert_eq!(simulator.state().timestamp, timestamp + 60);
        assert_eq!(simulator.state().ledger_sequence, 11);
    }

    #[test]
    fn invoke_keeps_legacy_result_and_event_behavior() {
        let mut simulator = NetworkSimulator::new(42);
        simulator
            .deploy_contract_with_id("C_TEST", "wasm")
            .unwrap();
        let result = simulator
            .invoke("C_TEST", "increment", &["1".to_string()])
            .unwrap();
        assert_eq!(result.fee, 10_000 + 900 + 500 + 43);
        assert_eq!(result.events, vec![format!("increment:{}", result.return_value)]);
        assert_eq!(simulator.state().events.len(), 1);
        assert_eq!(simulator.state().events[0].data, "1");
    }

    #[test]
    fn legacy_json_state_round_trips_through_compatibility_api() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy-sim.json");
        let mut simulator = NetworkSimulator::new(7);
        simulator
            .deploy_contract_with_id("C_TEST", "wasm")
            .unwrap();
        simulator.fund_account("G_TEST", 1234);
        simulator
            .invoke("C_TEST", "balance", &["G_TEST".to_string()])
            .unwrap();
        let expected = simulator.state().clone();
        let restored_from_state = NetworkSimulator::from_state(expected.clone(), 7);
        assert_eq!(restored_from_state.state(), &expected);
        simulator.save_to_file(&path).unwrap();
        let restored = NetworkSimulator::load_from_file(&path, 7).unwrap();
        assert_eq!(restored.state(), &expected);
        assert_eq!(restored.seed(), 7);
    }
}
