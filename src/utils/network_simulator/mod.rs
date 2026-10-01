//! Canonical in-process Stellar/Soroban network simulator.
//!
//! This module owns simulator state and the shared simulation API used by
//! commands and tests. It is the canonical implementation for network
//! simulation; callers should use its engine and focused submodules rather than
//! maintaining a second simulator. The public API is grouped by responsibility:
//!
//! - **`simulator`**: Core Soroban RPC simulator (accounts, contracts, ledgers)
//! - **`deterministic`**: Seeded RNG and deterministic execution parameters
//! - **`state`**: State snapshot/restore, export/import, and simulator state files
//! - **`time`**: Ledger time control (advance, freeze, jump)
//! - **`failure`**: Failure injection (RPC errors, transaction failures, network faults)
//! - **`scenarios`**: Built-in account/contract fixtures and step-oriented scripted scenarios
//!
//! ## Quick-start
//!
//! ```ignore
//! use starforge::utils::network_simulator::*;
//!
//! let mut sim = NetworkSimulator::new().with_deterministic_seed(42);
//! sim.start();
//!
//! // Deploy a contract, invoke it, inspect state…
//! ```

pub mod deterministic;
pub mod compat;
pub mod failure;
pub mod scenarios;
pub mod simulator;
pub mod state;
pub mod time;

// ── Re-exports for convenience ────────────────────────────────────────────────

pub use deterministic::{DeterministicConfig, SeededRng};
pub use compat::{
    FailureMode as LegacyFailureMode, SimContract, SimEvent, SimInvokeResult,
    SimLedgerState, SimScenario as LegacySimScenario,
    SimScenarioResult as LegacySimScenarioResult,
    SimScenarioStep as LegacySimScenarioStep,
    NetworkSimulator as LegacyNetworkSimulator,
};
pub use failure::{FailureInjector, FailureMode, FailureRule};
pub use scenarios::{
    builtin_scenarios, load_scenario, save_scenario, sim_data_dir, BuiltInScenario,
    Scenario, ScenarioResult, ScenarioRunner, ScriptFailureMode, SimScenario,
    SimScenarioResult, SimScenarioStep,
};
pub use simulator::{
    AccountInfo, ContractInstance, LedgerInfo, NetworkSimulator, SimulationOutcome,
    SimulatorConfig, SimulatorMode, TransactionReceipt,
};
pub use state::{SnapshotManager, StateSnapshot};
pub use time::{LedgerTime, TimeController};
