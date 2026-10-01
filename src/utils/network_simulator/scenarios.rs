//! # Simulation Scenarios
//!
//! Pre-built, parameterizable test scenarios that set up the simulator
//! with realistic contract + account states for reproducible testing.

use crate::utils::network_simulator::deterministic::derive_public_key;
use crate::utils::network_simulator::deterministic::SeededRng;
use crate::utils::network_simulator::failure::{FailureMode as InjectedFailureMode, FailureRule};
use crate::utils::network_simulator::simulator::{NetworkSimulator, SimulatorConfig};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Identifies a built-in scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltInScenario {
    /// Simple counter contract with a single account.
    SimpleCounter,
    /// Token contract with two accounts (minter, user).
    TokenTransfer,
    /// Escrow contract with three participants.
    Escrow,
    /// Multi-sig vault with threshold 2/3.
    MultisigVault,
    /// Empty network (no accounts, no contracts).
    Empty,
    /// Network with many accounts for load testing.
    LoadTest,
}

impl BuiltInScenario {
    pub fn name(&self) -> &'static str {
        match self {
            BuiltInScenario::SimpleCounter => "simple-counter",
            BuiltInScenario::TokenTransfer => "token-transfer",
            BuiltInScenario::Escrow => "escrow",
            BuiltInScenario::MultisigVault => "multisig-vault",
            BuiltInScenario::Empty => "empty",
            BuiltInScenario::LoadTest => "load-test",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            BuiltInScenario::SimpleCounter => "A single counter contract with one account",
            BuiltInScenario::TokenTransfer => "A token contract with a minter and a user account",
            BuiltInScenario::Escrow => "An escrow contract with sender, receiver, and arbiter",
            BuiltInScenario::MultisigVault => "A multi-sig vault with 2/3 threshold",
            BuiltInScenario::Empty => "An empty network with no accounts or contracts",
            BuiltInScenario::LoadTest => "10 accounts and 3 contracts for load/performance testing",
        }
    }
}

/// A configured scenario ready to run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    pub built_in: Option<BuiltInScenario>,
    pub config: SimulatorConfig,
    pub accounts_to_create: Vec<ScenarioAccount>,
    pub contracts_to_deploy: Vec<ScenarioContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioAccount {
    pub name: String,
    pub initial_balance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioContract {
    pub name: String,
    pub wasm_hash: String,
    pub deployer_account: String,
    pub initial_storage: HashMap<String, String>,
}

/// Runs scenarios against a simulator instance.
pub struct ScenarioRunner;

impl ScenarioRunner {
    /// Create a new scenario runner.
    pub fn new() -> Self {
        Self
    }

    /// Load a built-in scenario by name.
    pub fn load_built_in(scenario: BuiltInScenario, seed: u64) -> Scenario {
        match scenario {
            BuiltInScenario::SimpleCounter => Self::simple_counter(seed),
            BuiltInScenario::TokenTransfer => Self::token_transfer(seed),
            BuiltInScenario::Escrow => Self::escrow(seed),
            BuiltInScenario::MultisigVault => Self::multisig_vault(seed),
            BuiltInScenario::Empty => Self::empty(seed),
            BuiltInScenario::LoadTest => Self::load_test(seed),
        }
    }

    /// Apply a scenario to a simulator, returning the mapping of
    /// logical names → public keys / contract IDs.
    pub fn apply(&self, sim: &mut NetworkSimulator, scenario: &Scenario) -> Result<ScenarioResult> {
        let mut result = ScenarioResult::new(&scenario.name);

        // Create accounts.
        for acct in &scenario.accounts_to_create {
            let info = sim.create_account(acct.initial_balance);
            result
                .accounts
                .insert(acct.name.clone(), info.public_key.clone());
        }

        // Deploy contracts.
        for ctr in &scenario.contracts_to_deploy {
            let deployer_pk = result
                .accounts
                .get(&ctr.deployer_account)
                .cloned()
                .unwrap_or_else(|| {
                    derive_public_key(sim.config.deterministic.seed, sim.accounts.len() as u32)
                });

            // Ensure the deployer account exists.
            if sim.get_account(&deployer_pk).is_none() {
                sim.create_account_with_key(&deployer_pk, 1000.0);
                result
                    .accounts
                    .insert(ctr.deployer_account.clone(), deployer_pk.clone());
            }

            let instance = sim
                .deploy_contract(&ctr.wasm_hash, &deployer_pk)
                .map_err(|e| anyhow::anyhow!("Failed to deploy '{}': {}", ctr.name, e))?;

            // Write initial storage.
            for (key, value) in &ctr.initial_storage {
                sim.write_contract_storage(&instance.contract_id, key, value.clone())
                    .map_err(|e| anyhow::anyhow!(e))?;
            }

            result
                .contracts
                .insert(ctr.name.clone(), instance.contract_id);
        }

        Ok(result)
    }

    /// Run a scenario from scratch, returning the simulator and result.
    pub fn run(scenario: Scenario) -> (NetworkSimulator, ScenarioResult) {
        let config = scenario.config.clone();
        let mut sim = NetworkSimulator::with_config(config);
        match Self::new().apply(&mut sim, &scenario) {
            Ok(result) => (sim, result),
            Err(e) => panic!("Scenario '{}' failed to apply: {}", scenario.name, e),
        }
    }

    // ── Built-in scenarios ───────────────────────────────────────────────────

    fn simple_counter(seed: u64) -> Scenario {
        Scenario {
            name: "simple-counter".to_string(),
            description: "A single counter contract with one account".to_string(),
            built_in: Some(BuiltInScenario::SimpleCounter),
            config: SimulatorConfig {
                deterministic:
                    crate::utils::network_simulator::deterministic::DeterministicConfig {
                        seed,
                        ..Default::default()
                    },
                ..Default::default()
            },
            accounts_to_create: vec![ScenarioAccount {
                name: "alice".to_string(),
                initial_balance: 10_000.0,
            }],
            contracts_to_deploy: vec![ScenarioContract {
                name: "counter".to_string(),
                wasm_hash: "counter_wasm_v1".to_string(),
                deployer_account: "alice".to_string(),
                initial_storage: HashMap::from([("count".to_string(), "0".to_string())]),
            }],
        }
    }

    fn token_transfer(seed: u64) -> Scenario {
        Scenario {
            name: "token-transfer".to_string(),
            description: "A token contract with a minter and a user account".to_string(),
            built_in: Some(BuiltInScenario::TokenTransfer),
            config: SimulatorConfig {
                deterministic:
                    crate::utils::network_simulator::deterministic::DeterministicConfig {
                        seed,
                        ..Default::default()
                    },
                ..Default::default()
            },
            accounts_to_create: vec![
                ScenarioAccount {
                    name: "minter".to_string(),
                    initial_balance: 100_000.0,
                },
                ScenarioAccount {
                    name: "user".to_string(),
                    initial_balance: 1_000.0,
                },
            ],
            contracts_to_deploy: vec![ScenarioContract {
                name: "token".to_string(),
                wasm_hash: "token_wasm_v1".to_string(),
                deployer_account: "minter".to_string(),
                initial_storage: HashMap::from([
                    ("name".to_string(), "\"SimToken\"".to_string()),
                    ("symbol".to_string(), "\"SIM\"".to_string()),
                    ("decimals".to_string(), "7".to_string()),
                    ("total_supply".to_string(), "10000000000000".to_string()),
                    ("balance_minter".to_string(), "10000000000000".to_string()),
                ]),
            }],
        }
    }

    fn escrow(seed: u64) -> Scenario {
        Scenario {
            name: "escrow".to_string(),
            description: "An escrow contract with sender, receiver, and arbiter".to_string(),
            built_in: Some(BuiltInScenario::Escrow),
            config: SimulatorConfig {
                deterministic:
                    crate::utils::network_simulator::deterministic::DeterministicConfig {
                        seed,
                        ..Default::default()
                    },
                ..Default::default()
            },
            accounts_to_create: vec![
                ScenarioAccount {
                    name: "sender".to_string(),
                    initial_balance: 50_000.0,
                },
                ScenarioAccount {
                    name: "receiver".to_string(),
                    initial_balance: 1_000.0,
                },
                ScenarioAccount {
                    name: "arbiter".to_string(),
                    initial_balance: 10_000.0,
                },
            ],
            contracts_to_deploy: vec![ScenarioContract {
                name: "escrow".to_string(),
                wasm_hash: "escrow_wasm_v1".to_string(),
                deployer_account: "sender".to_string(),
                initial_storage: HashMap::from([
                    ("sender".to_string(), "pending".to_string()),
                    ("receiver".to_string(), "".to_string()),
                    ("arbiter".to_string(), "".to_string()),
                    ("amount".to_string(), "10000".to_string()),
                    ("status".to_string(), "\"created\"".to_string()),
                ]),
            }],
        }
    }

    fn multisig_vault(seed: u64) -> Scenario {
        Scenario {
            name: "multisig-vault".to_string(),
            description: "A multi-sig vault with 2/3 threshold".to_string(),
            built_in: Some(BuiltInScenario::MultisigVault),
            config: SimulatorConfig {
                deterministic:
                    crate::utils::network_simulator::deterministic::DeterministicConfig {
                        seed,
                        ..Default::default()
                    },
                ..Default::default()
            },
            accounts_to_create: vec![
                ScenarioAccount {
                    name: "signer1".to_string(),
                    initial_balance: 100_000.0,
                },
                ScenarioAccount {
                    name: "signer2".to_string(),
                    initial_balance: 100_000.0,
                },
                ScenarioAccount {
                    name: "signer3".to_string(),
                    initial_balance: 100_000.0,
                },
            ],
            contracts_to_deploy: vec![ScenarioContract {
                name: "vault".to_string(),
                wasm_hash: "vault_wasm_v1".to_string(),
                deployer_account: "signer1".to_string(),
                initial_storage: HashMap::from([
                    ("threshold".to_string(), "2".to_string()),
                    ("total_signers".to_string(), "3".to_string()),
                    ("balance".to_string(), "500000".to_string()),
                ]),
            }],
        }
    }

    fn empty(_seed: u64) -> Scenario {
        Scenario {
            name: "empty".to_string(),
            description: "An empty network with no accounts or contracts".to_string(),
            built_in: Some(BuiltInScenario::Empty),
            config: SimulatorConfig::default(),
            accounts_to_create: vec![],
            contracts_to_deploy: vec![],
        }
    }

    fn load_test(_seed: u64) -> Scenario {
        let mut accounts = Vec::new();
        for i in 0..10 {
            accounts.push(ScenarioAccount {
                name: format!("user_{}", i),
                initial_balance: 1_000.0 + (i as f64 * 1_000.0),
            });
        }

        let contracts = vec![
            ScenarioContract {
                name: "contract_a".to_string(),
                wasm_hash: "load_wasm_a".to_string(),
                deployer_account: "user_0".to_string(),
                initial_storage: HashMap::from([("data".to_string(), "initial".to_string())]),
            },
            ScenarioContract {
                name: "contract_b".to_string(),
                wasm_hash: "load_wasm_b".to_string(),
                deployer_account: "user_1".to_string(),
                initial_storage: HashMap::from([("data".to_string(), "initial".to_string())]),
            },
            ScenarioContract {
                name: "contract_c".to_string(),
                wasm_hash: "load_wasm_c".to_string(),
                deployer_account: "user_2".to_string(),
                initial_storage: HashMap::from([("data".to_string(), "initial".to_string())]),
            },
        ];

        Scenario {
            name: "load-test".to_string(),
            description: "10 accounts and 3 contracts for load/performance testing".to_string(),
            built_in: Some(BuiltInScenario::LoadTest),
            config: SimulatorConfig::default(),
            accounts_to_create: accounts,
            contracts_to_deploy: contracts,
        }
    }
}

impl Default for ScenarioRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of applying a scenario – maps logical names to addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario_name: String,
    pub accounts: HashMap<String, String>,
    pub contracts: HashMap<String, String>,
}

impl ScenarioResult {
    pub fn new(scenario_name: &str) -> Self {
        Self {
            scenario_name: scenario_name.to_string(),
            accounts: HashMap::new(),
            contracts: HashMap::new(),
        }
    }

    /// Get a contract ID by logical name.
    pub fn contract_id(&self, name: &str) -> Option<&str> {
        self.contracts.get(name).map(|s| s.as_str())
    }

    /// Get a public key by logical name.
    pub fn public_key(&self, name: &str) -> Option<&str> {
        self.accounts.get(name).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_counter_scenario_creates_account_and_contract() {
        let (sim, result) = ScenarioRunner::run(ScenarioRunner::simple_counter(42));
        assert!(result.accounts.contains_key("alice"));
        assert!(result.contracts.contains_key("counter"));
        assert_eq!(sim.accounts.len(), 1);
        assert_eq!(sim.contracts.len(), 1);
    }

    #[test]
    fn token_transfer_scenario_creates_two_accounts() {
        let (sim, result) = ScenarioRunner::run(ScenarioRunner::token_transfer(42));
        assert!(result.accounts.contains_key("minter"));
        assert!(result.accounts.contains_key("user"));
        assert!(result.contracts.contains_key("token"));
        assert_eq!(sim.accounts.len(), 2);
        assert_eq!(sim.contracts.len(), 1);

        // Verify initial storage.
        let cid = result.contract_id("token").unwrap();
        let storage = &sim.get_contract(cid).unwrap().storage;
        assert!(storage.contains_key("name"));
        assert!(storage.contains_key("symbol"));
    }

    #[test]
    fn escrow_scenario_has_three_participants() {
        let (sim, result) = ScenarioRunner::run(ScenarioRunner::escrow(42));
        assert!(result.accounts.contains_key("sender"));
        assert!(result.accounts.contains_key("receiver"));
        assert!(result.accounts.contains_key("arbiter"));
        assert_eq!(sim.accounts.len(), 3);
        assert_eq!(sim.contracts.len(), 1);
    }

    #[test]
    fn multisig_vault_scenario_has_three_signers() {
        let (_, result) = ScenarioRunner::run(ScenarioRunner::multisig_vault(42));
        assert_eq!(result.accounts.len(), 3);
        assert!(result.contracts.contains_key("vault"));
    }

    #[test]
    fn empty_scenario_creates_nothing() {
        let (sim, result) = ScenarioRunner::run(ScenarioRunner::empty(42));
        assert!(result.accounts.is_empty());
        assert!(result.contracts.is_empty());
        assert!(sim.accounts.is_empty());
        assert!(sim.contracts.is_empty());
    }

    #[test]
    fn load_test_creates_ten_accounts_three_contracts() {
        let (sim, result) = ScenarioRunner::run(ScenarioRunner::load_test(42));
        assert_eq!(result.accounts.len(), 10);
        assert_eq!(result.contracts.len(), 3);
        assert_eq!(sim.accounts.len(), 10);
        assert_eq!(sim.contracts.len(), 3);
    }

    #[test]
    fn scenario_result_provides_convenience_accessors() {
        let (_, result) = ScenarioRunner::run(ScenarioRunner::simple_counter(42));
        assert!(result.public_key("alice").is_some());
        assert!(result.public_key("unknown").is_none());
        assert!(result.contract_id("counter").is_some());
        assert!(result.contract_id("unknown").is_none());
    }

    #[test]
    fn all_scenarios_have_unique_names() {
        let scenarios = vec![
            BuiltInScenario::SimpleCounter,
            BuiltInScenario::TokenTransfer,
            BuiltInScenario::Escrow,
            BuiltInScenario::MultisigVault,
            BuiltInScenario::Empty,
            BuiltInScenario::LoadTest,
        ];

        let mut names = std::collections::HashSet::new();
        for s in &scenarios {
            assert!(names.insert(s.name()), "Duplicate name: {}", s.name());
        }
    }

    #[test]
    fn load_scenario_by_enum_is_deterministic() {
        let s1 = ScenarioRunner::load_built_in(BuiltInScenario::SimpleCounter, 42);
        let s2 = ScenarioRunner::load_built_in(BuiltInScenario::SimpleCounter, 42);
        assert_eq!(s1.accounts_to_create.len(), s2.accounts_to_create.len());
        assert_eq!(s1.contracts_to_deploy.len(), s2.contracts_to_deploy.len());
    }
}

/// A step-oriented scenario for exercising simulator behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimScenario {
    pub name: String,
    pub description: String,
    pub seed: u64,
    pub initial_ledger: u32,
    pub steps: Vec<SimScenarioStep>,
}

/// Actions supported by a step-oriented scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SimScenarioStep {
    Deploy {
        contract_id: String,
        wasm_hash: String,
    },
    Invoke {
        contract_id: String,
        function: String,
        args: Vec<String>,
        expected_return: Option<String>,
    },
    AdvanceTime {
        seconds: u64,
    },
    AdvanceLedger {
        count: u32,
    },
    InjectFailure {
        mode: ScriptFailureMode,
    },
    Snapshot {
        name: String,
    },
    Restore {
        name: String,
    },
    FundAccount {
        address: String,
        amount: u64,
    },
}

/// Failure choices available in a step-oriented scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptFailureMode {
    None,
    RpcTimeout,
    RpcError,
    InsufficientFee,
    ContractNotFound,
    Random { probability_pct: u8 },
}

/// Outcome of running a step-oriented scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimScenarioResult {
    pub scenario: String,
    pub passed: bool,
    pub steps_run: usize,
    pub steps_total: usize,
    pub errors: Vec<String>,
    pub final_ledger: u32,
}

/// Return the default directory for simulator data.
pub fn sim_data_dir() -> PathBuf {
    crate::utils::config::config_dir().join("sim")
}

/// Load a step-oriented scenario from JSON.
pub fn load_scenario(path: &Path) -> Result<SimScenario> {
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

/// Save a step-oriented scenario as JSON.
pub fn save_scenario(scenario: &SimScenario, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(scenario)?)?;
    Ok(())
}

/// Return the standard scripted smoke scenarios.
pub fn builtin_scenarios() -> Vec<SimScenario> {
    vec![
        SimScenario {
            name: "basic-deploy-invoke".to_string(),
            description: "Deploy a contract and invoke a function".to_string(),
            seed: 42,
            initial_ledger: 100,
            steps: vec![
                SimScenarioStep::Deploy {
                    contract_id: "C_SIM_COUNTER".to_string(),
                    wasm_hash: "abc123def456".to_string(),
                },
                SimScenarioStep::Invoke {
                    contract_id: "C_SIM_COUNTER".to_string(),
                    function: "increment".to_string(),
                    args: vec![],
                    expected_return: None,
                },
                SimScenarioStep::AdvanceLedger { count: 5 },
            ],
        },
        SimScenario {
            name: "failure-recovery".to_string(),
            description: "Inject failure, snapshot, restore, and retry".to_string(),
            seed: 99,
            initial_ledger: 200,
            steps: vec![
                SimScenarioStep::Deploy {
                    contract_id: "C_SIM_TOKEN".to_string(),
                    wasm_hash: "token_wasm_hash".to_string(),
                },
                SimScenarioStep::Snapshot {
                    name: "pre-failure".to_string(),
                },
                SimScenarioStep::InjectFailure {
                    mode: ScriptFailureMode::RpcTimeout,
                },
                SimScenarioStep::Restore {
                    name: "pre-failure".to_string(),
                },
                SimScenarioStep::InjectFailure {
                    mode: ScriptFailureMode::None,
                },
                SimScenarioStep::Invoke {
                    contract_id: "C_SIM_TOKEN".to_string(),
                    function: "balance".to_string(),
                    args: vec!["GABC".to_string()],
                    expected_return: None,
                },
            ],
        },
        SimScenario {
            name: "time-travel".to_string(),
            description: "Advance virtual time and ledger sequence".to_string(),
            seed: 7,
            initial_ledger: 1,
            steps: vec![
                SimScenarioStep::FundAccount {
                    address: "GTESTACCOUNT".to_string(),
                    amount: 10_000_000_000,
                },
                SimScenarioStep::AdvanceTime { seconds: 3600 },
                SimScenarioStep::AdvanceLedger { count: 100 },
                SimScenarioStep::Deploy {
                    contract_id: "C_SIM_ESCROW".to_string(),
                    wasm_hash: "escrow_hash".to_string(),
                },
            ],
        },
    ]
}

impl NetworkSimulator {
    /// Run a step-oriented scenario against this simulator.
    pub fn run_scenario(&mut self, scenario: &SimScenario) -> SimScenarioResult {
        self.config.deterministic.seed = scenario.seed;
        self.rng = SeededRng::new(scenario.seed);
        self.ledger.sequence = scenario.initial_ledger;
        self.time_controller.ledger_time.sequence = scenario.initial_ledger;
        self.failure_injector.clear_rules();
        self.failure_injector.disable();

        let mut errors = Vec::new();
        let mut steps_run = 0;
        let mut snapshots = HashMap::new();

        for step in &scenario.steps {
            steps_run += 1;
            if let Err(error) = execute_script_step(self, step, &mut snapshots) {
                errors.push(format!("Step {}: {}", steps_run, error));
                break;
            }
        }

        SimScenarioResult {
            scenario: scenario.name.clone(),
            passed: errors.is_empty(),
            steps_run,
            steps_total: scenario.steps.len(),
            errors,
            final_ledger: self.current_ledger(),
        }
    }
}

fn execute_script_step(
    simulator: &mut NetworkSimulator,
    step: &SimScenarioStep,
    snapshots: &mut HashMap<String, String>,
) -> Result<(), String> {
    match step {
        SimScenarioStep::Deploy {
            contract_id,
            wasm_hash,
        } => {
            let deployer = simulator
                .accounts
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| {
                    let key = derive_public_key(simulator.config.deterministic.seed, 0);
                    simulator.create_account_with_key(&key, 1_000.0);
                    key
                });
            simulator.deploy_contract_with_id(contract_id, wasm_hash, &deployer)?;
        }
        SimScenarioStep::Invoke {
            contract_id,
            function,
            args,
            expected_return,
        } => {
            let source_account = simulator
                .accounts
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| {
                    let key = derive_public_key(simulator.config.deterministic.seed, 0);
                    simulator.create_account_with_key(&key, 1_000.0);
                    key
                });
            let outcome = simulator.simulate_invoke(contract_id, function, args, &source_account)?;
            if let Some(expected) = expected_return {
                if outcome.return_value != *expected {
                    return Err(format!(
                        "Expected return '{}', got '{}'",
                        expected, outcome.return_value
                    ));
                }
            }
            simulator.advance_ledger();
        }
        SimScenarioStep::AdvanceTime { seconds } => simulator.advance_time(*seconds),
        SimScenarioStep::AdvanceLedger { count } => simulator.advance_ledgers(*count),
        SimScenarioStep::InjectFailure { mode } => {
            simulator.failure_injector.clear_rules();
            match mode {
                ScriptFailureMode::None => simulator.failure_injector.disable(),
                mode => {
                    let injected = match mode {
                        ScriptFailureMode::RpcTimeout => InjectedFailureMode::RpcTimeout,
                        ScriptFailureMode::RpcError => {
                            InjectedFailureMode::RpcError { code: -32603 }
                        }
                        ScriptFailureMode::InsufficientFee => InjectedFailureMode::InsufficientFee,
                        ScriptFailureMode::ContractNotFound => {
                            InjectedFailureMode::ContractNotFound
                        }
                        ScriptFailureMode::Random { probability_pct } => {
                            InjectedFailureMode::RandomFailure(*probability_pct as f64 / 100.0)
                        }
                        ScriptFailureMode::None => unreachable!(),
                    };
                    simulator.failure_injector.add_rule(FailureRule::new(
                        "scripted-failure",
                        injected,
                    ));
                    simulator.failure_injector.enable();
                }
            }
        }
        SimScenarioStep::Snapshot { name } => {
            let id = simulator.take_snapshot(name);
            snapshots.insert(name.clone(), id);
        }
        SimScenarioStep::Restore { name } => {
            let id = snapshots
                .get(name)
                .ok_or_else(|| format!("Snapshot '{}' not found", name))?;
            simulator.restore_snapshot(id)?;
        }
        SimScenarioStep::FundAccount { address, amount } => {
            if simulator.get_account(address).is_none() {
                simulator.create_account_with_key(address, 0.0);
            }
            simulator.fund_account(address, *amount as f64)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod scripted_scenario_tests {
    use super::*;

    #[test]
    fn migrated_builtin_scripted_scenarios_run() {
        for scenario in builtin_scenarios() {
            let mut simulator = NetworkSimulator::new();
            let result = simulator.run_scenario(&scenario);
            assert!(result.passed, "{}: {:?}", scenario.name, result.errors);
            assert_eq!(result.steps_run, scenario.steps.len());
        }
    }

    #[test]
    fn scripted_scenario_json_round_trips() {
        let scenario = builtin_scenarios().remove(1);
        let encoded = serde_json::to_string(&scenario).unwrap();
        let decoded: SimScenario = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.name, scenario.name);
        assert_eq!(decoded.steps.len(), scenario.steps.len());
    }
}
