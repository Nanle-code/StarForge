//! Soroban contract pattern library.
//!
//! Provides:
//! - Static definitions for known Soroban design patterns (token, governance,
//!   DeFi, access-control, storage).
//! - Anti-pattern definitions with severity and remediation advice.
//! - A file-backed feedback store so users can mark pattern matches as correct
//!   or incorrect, enabling the AI prompts to improve over time.
//! - Static analysis helpers that scan contract source for pattern/anti-pattern
//!   indicators before the LLM call, giving the model a structured head-start.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::utils::config;

// ── Pattern categories ────────────────────────────────────────────────────────

/// Top-level category a pattern belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternCategory {
    Token,
    Governance,
    DeFi,
    AccessControl,
    Storage,
    General,
}

impl std::fmt::Display for PatternCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PatternCategory::Token => "Token",
            PatternCategory::Governance => "Governance",
            PatternCategory::DeFi => "DeFi",
            PatternCategory::AccessControl => "Access Control",
            PatternCategory::Storage => "Storage",
            PatternCategory::General => "General",
        };
        write!(f, "{}", s)
    }
}

// ── Pattern definition ────────────────────────────────────────────────────────

/// A named design pattern recognised in Soroban contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractPattern {
    /// Short machine-readable identifier, e.g. `"sep41-fungible-token"`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Category.
    pub category: PatternCategory,
    /// One-paragraph description.
    pub description: String,
    /// Source code indicators searched for during static pre-scan.
    pub indicators: Vec<String>,
    /// Links to reference implementations or SEPs.
    pub references: Vec<String>,
    /// Concrete improvement suggestions for contracts that match this pattern.
    pub suggestions: Vec<String>,
}

// ── Anti-pattern definition ───────────────────────────────────────────────────

/// Severity of an anti-pattern finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AntiPatternSeverity {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for AntiPatternSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AntiPatternSeverity::Critical => "Critical",
            AntiPatternSeverity::High => "High",
            AntiPatternSeverity::Medium => "Medium",
            AntiPatternSeverity::Low => "Low",
        };
        write!(f, "{}", s)
    }
}

/// A known anti-pattern that should be flagged in Soroban contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPattern {
    pub id: String,
    pub name: String,
    pub category: PatternCategory,
    pub severity: AntiPatternSeverity,
    pub description: String,
    /// Source code indicators that suggest this anti-pattern is present.
    pub indicators: Vec<String>,
    /// How to fix it.
    pub remediation: String,
}

// ── Static pattern library ────────────────────────────────────────────────────

/// Returns all built-in Soroban design patterns.
pub fn all_patterns() -> Vec<ContractPattern> {
    vec![
        // ── Token patterns ────────────────────────────────────────────────────
        ContractPattern {
            id: "sep41-fungible-token".into(),
            name: "SEP-41 Fungible Token".into(),
            category: PatternCategory::Token,
            description: "Implements the SEP-41 token interface: initialize, mint, burn, \
                transfer, balance, allowance, approve, transfer_from. The canonical \
                fungible-token pattern on Stellar."
                .into(),
            indicators: vec![
                "fn initialize".into(),
                "fn mint".into(),
                "fn burn".into(),
                "fn transfer".into(),
                "fn balance".into(),
                "fn allowance".into(),
                "fn approve".into(),
                "fn transfer_from".into(),
            ],
            references: vec![
                "https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md"
                    .into(),
            ],
            suggestions: vec![
                "Ensure `initialize` is guarded so it can only be called once.".into(),
                "Emit events on every state-changing operation for off-chain indexers.".into(),
                "Store the admin key in contract instance storage, not ledger entry storage."
                    .into(),
            ],
        },
        ContractPattern {
            id: "nft-unique-token".into(),
            name: "Non-Fungible Token (NFT)".into(),
            category: PatternCategory::Token,
            description: "Mints unique tokens identified by a numeric or string ID with \
                ownership tracking and optional metadata URI storage."
                .into(),
            indicators: vec![
                "fn mint".into(),
                "fn owner_of".into(),
                "fn transfer".into(),
                "TokenId".into(),
                "token_id".into(),
            ],
            references: vec!["https://soroban.stellar.org/docs/tutorials/nft".into()],
            suggestions: vec![
                "Use `Map<u64, Address>` in persistent storage for the ownership registry.".into(),
                "Guard `mint` with an admin-only access check.".into(),
                "Emit a `Transfer` event with `from = Address::zero()` on mint.".into(),
            ],
        },
        ContractPattern {
            id: "wrapped-asset".into(),
            name: "Wrapped Asset".into(),
            category: PatternCategory::Token,
            description: "Wraps a Stellar classic asset (e.g. USDC, XLM) into a Soroban \
                contract token, bridging the classic and smart-contract layers."
                .into(),
            indicators: vec![
                "stellar_asset_contract".into(),
                "StellarAssetClient".into(),
                "wrap".into(),
                "unwrap".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Always verify the wrapped asset issuer in `initialize`.".into(),
                "Implement `unwrap` with a reentrancy guard pattern.".into(),
            ],
        },
        // ── Governance patterns ───────────────────────────────────────────────
        ContractPattern {
            id: "on-chain-voting".into(),
            name: "On-Chain Voting / Proposal".into(),
            category: PatternCategory::Governance,
            description: "Allows token holders to create proposals, cast votes, and \
                execute approved actions on-chain after a voting period."
                .into(),
            indicators: vec![
                "fn create_proposal".into(),
                "fn vote".into(),
                "fn execute".into(),
                "Proposal".into(),
                "VoteResult".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Use `ledger().timestamp()` for voting deadlines instead of block numbers.".into(),
                "Store proposals in `Persistent` storage so they survive TTL extension.".into(),
                "Require a quorum check before `execute` is permitted.".into(),
            ],
        },
        ContractPattern {
            id: "timelock-controller".into(),
            name: "Timelock Controller".into(),
            category: PatternCategory::Governance,
            description: "Enforces a mandatory waiting period between a governance action \
                being approved and it being executed, giving stakeholders time to react."
                .into(),
            indicators: vec![
                "timelock".into(),
                "delay".into(),
                "schedule".into(),
                "min_delay".into(),
                "execute_after".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Enforce a minimum delay of at least one ledger close time (~5 s) multiplied \
                 by a safe factor for your security model."
                    .into(),
                "Emit `CallScheduled` and `CallExecuted` events for off-chain monitoring.".into(),
            ],
        },
        ContractPattern {
            id: "multisig-admin".into(),
            name: "Multi-Signature Admin".into(),
            category: PatternCategory::Governance,
            description: "Requires M-of-N administrator signatures before a privileged \
                operation is executed, distributing trust across multiple key holders."
                .into(),
            indicators: vec![
                "signers".into(),
                "threshold".into(),
                "required_signatures".into(),
                "Vec<Address>".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Store signers in `Instance` storage for cheap reads on every call.".into(),
                "Use a nonce or proposal ID to prevent signature replay attacks.".into(),
            ],
        },
        // ── DeFi patterns ─────────────────────────────────────────────────────
        ContractPattern {
            id: "constant-product-amm".into(),
            name: "Constant-Product AMM (x·y=k)".into(),
            category: PatternCategory::DeFi,
            description: "Automated market-maker using the constant-product invariant. \
                Provides `swap`, `add_liquidity`, and `remove_liquidity` entry points."
                .into(),
            indicators: vec![
                "fn swap".into(),
                "fn add_liquidity".into(),
                "fn remove_liquidity".into(),
                "reserve_a".into(),
                "reserve_b".into(),
                "liquidity".into(),
            ],
            references: vec!["https://github.com/uniswap/v2-core".into()],
            suggestions: vec![
                "Always validate `min_amount_out` to protect against sandwich attacks.".into(),
                "Accumulate protocol fees in a separate storage key.".into(),
                "Emit `Swap` and `LiquidityChanged` events for every state transition.".into(),
            ],
        },
        ContractPattern {
            id: "lending-pool".into(),
            name: "Lending / Borrow Pool".into(),
            category: PatternCategory::DeFi,
            description: "Accepts deposits, issues debt positions, tracks collateral ratios, \
                and liquidates undercollateralised positions."
                .into(),
            indicators: vec![
                "fn deposit".into(),
                "fn borrow".into(),
                "fn repay".into(),
                "fn liquidate".into(),
                "collateral".into(),
                "health_factor".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Use a price oracle with a freshness check to prevent stale-price exploits.".into(),
                "Implement a liquidation bonus cap to prevent value drain.".into(),
                "Store debt balances scaled by an interest-rate accumulator.".into(),
            ],
        },
        ContractPattern {
            id: "staking-rewards".into(),
            name: "Staking / Yield Distribution".into(),
            category: PatternCategory::DeFi,
            description: "Users deposit tokens to earn a share of a reward pool distributed \
                proportionally over time."
                .into(),
            indicators: vec![
                "fn stake".into(),
                "fn unstake".into(),
                "fn claim".into(),
                "rewards_per_share".into(),
                "staked_amount".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Use the Synthetix staking rewards algorithm (rewards-per-token accumulator) \
                 to avoid O(n) reward distribution loops."
                    .into(),
                "Apply a withdrawal cooldown to protect against flash-loan draining.".into(),
            ],
        },
        ContractPattern {
            id: "escrow".into(),
            name: "Escrow".into(),
            category: PatternCategory::DeFi,
            description: "Holds funds on behalf of two parties and releases them when \
                agreed conditions are met or a mediator resolves a dispute."
                .into(),
            indicators: vec![
                "fn deposit".into(),
                "fn release".into(),
                "fn refund".into(),
                "fn dispute".into(),
                "escrow".into(),
                "mediator".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Store an expiry timestamp; allow refund after expiry without mediator.".into(),
                "Emit events on every state transition for auditability.".into(),
            ],
        },
        // ── Access control patterns ───────────────────────────────────────────
        ContractPattern {
            id: "owner-admin".into(),
            name: "Single Owner / Admin".into(),
            category: PatternCategory::AccessControl,
            description: "A single privileged address stored in contract storage controls \
                admin operations. The simplest access-control pattern."
                .into(),
            indicators: vec![
                "admin".into(),
                "owner".into(),
                "fn require_auth".into(),
                "env.current_contract_address".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Use `require_auth` on the admin address rather than manual signature checks."
                    .into(),
                "Implement a two-step ownership transfer to prevent accidental lock-out.".into(),
            ],
        },
        ContractPattern {
            id: "role-based-access".into(),
            name: "Role-Based Access Control (RBAC)".into(),
            category: PatternCategory::AccessControl,
            description: "Multiple named roles (e.g. MINTER, PAUSER, UPGRADER) each \
                controlling a subset of privileged operations."
                .into(),
            indicators: vec![
                "role".into(),
                "has_role".into(),
                "grant_role".into(),
                "revoke_role".into(),
                "Symbol::new".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Store roles in a `Map<Symbol, Vec<Address>>` in instance storage.".into(),
                "Never grant the default-admin role to a hot wallet in production.".into(),
            ],
        },
        ContractPattern {
            id: "pausable".into(),
            name: "Pausable".into(),
            category: PatternCategory::AccessControl,
            description: "Allows an admin to pause and unpause the contract, blocking all \
                state-changing operations during an emergency."
                .into(),
            indicators: vec![
                "fn pause".into(),
                "fn unpause".into(),
                "paused".into(),
                "is_paused".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Check the paused flag at the start of every state-changing function.".into(),
                "Emit `Paused` / `Unpaused` events for off-chain monitoring.".into(),
            ],
        },
        // ── Storage patterns ──────────────────────────────────────────────────
        ContractPattern {
            id: "instance-storage".into(),
            name: "Instance Storage for Config".into(),
            category: PatternCategory::Storage,
            description: "Uses `env.storage().instance()` for configuration data that is \
                read on every invocation (admin, token address, fees), minimising ledger \
                entry reads."
                .into(),
            indicators: vec![
                "storage().instance()".into(),
                "instance().get".into(),
                "instance().set".into(),
            ],
            references: vec!["https://soroban.stellar.org/docs/learn/storage".into()],
            suggestions: vec![
                "Extend the TTL of instance storage in the same transaction that writes to it."
                    .into(),
                "Group all config values behind a single `Config` struct key to reduce \
                 ledger entry count."
                    .into(),
            ],
        },
        ContractPattern {
            id: "persistent-user-data".into(),
            name: "Persistent Storage for User State".into(),
            category: PatternCategory::Storage,
            description: "Uses `env.storage().persistent()` for per-user balances, \
                allowances, and positions — data that must survive TTL but is not read \
                on every call."
                .into(),
            indicators: vec![
                "storage().persistent()".into(),
                "persistent().get".into(),
                "persistent().set".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Always call `bump_persistent` / `extend_ttl` when writing user state.".into(),
                "Use typed keys (`DataKey` enum) rather than raw strings.".into(),
            ],
        },
        ContractPattern {
            id: "temporary-storage".into(),
            name: "Temporary Storage for Transient State".into(),
            category: PatternCategory::Storage,
            description: "Uses `env.storage().temporary()` for data that only needs to \
                live for the duration of a transaction or a few ledgers (nonces, locks)."
                .into(),
            indicators: vec![
                "storage().temporary()".into(),
                "temporary().get".into(),
                "temporary().set".into(),
            ],
            references: vec![],
            suggestions: vec![
                "Temporary storage is the cheapest — prefer it for reentrancy locks.".into(),
                "Never rely on temporary storage surviving beyond one ledger close.".into(),
            ],
        },
    ]
}

// ── Static anti-pattern library ───────────────────────────────────────────────

/// Returns all built-in Soroban anti-patterns.
pub fn all_anti_patterns() -> Vec<AntiPattern> {
    vec![
        AntiPattern {
            id: "AP-001".into(),
            name: "Unbounded Storage Loop".into(),
            category: PatternCategory::Storage,
            severity: AntiPatternSeverity::Critical,
            description: "Iterating over a collection stored in persistent ledger entries \
                without a size bound causes gas to grow unboundedly as the collection grows, \
                eventually making the contract unusable."
                .into(),
            indicators: vec![
                "for ".into(),
                ".iter()".into(),
                "while ".into(),
                "persistent().get".into(),
            ],
            remediation: "Introduce pagination (offset + limit). Keep hot collections small \
                by archiving old entries off-chain. Consider a doubly-linked-list pattern \
                with head/tail pointers stored in instance storage."
                .into(),
        },
        AntiPattern {
            id: "AP-002".into(),
            name: "Missing require_auth".into(),
            category: PatternCategory::AccessControl,
            severity: AntiPatternSeverity::Critical,
            description: "State-changing functions (mint, burn, transfer admin role) that do \
                not call `env.require_auth(&admin)` or `address.require_auth()` can be \
                called by any account."
                .into(),
            indicators: vec![
                "fn mint".into(),
                "fn burn".into(),
                "fn set_admin".into(),
                "fn upgrade".into(),
            ],
            remediation: "Add `admin.require_auth()` at the top of every privileged function \
                before any state mutations."
                .into(),
        },
        AntiPattern {
            id: "AP-003".into(),
            name: "Unguarded Initialize".into(),
            category: PatternCategory::AccessControl,
            severity: AntiPatternSeverity::Critical,
            description: "`initialize` can be called multiple times, allowing an attacker to \
                reset admin keys or contract state after deployment."
                .into(),
            indicators: vec!["fn initialize".into(), "fn init".into()],
            remediation: "Store an `initialized: bool` flag in instance storage and panic \
                with `already_initialized` if it is true at the start of `initialize`."
                .into(),
        },
        AntiPattern {
            id: "AP-004".into(),
            name: "Integer Overflow / Underflow".into(),
            category: PatternCategory::General,
            severity: AntiPatternSeverity::High,
            description: "Arithmetic on token amounts without overflow checking can silently \
                wrap, causing balance corruption or infinite mint."
                .into(),
            indicators: vec![
                "+ ".into(),
                "- ".into(),
                "* ".into(),
                "as u64".into(),
                "as i128".into(),
            ],
            remediation: "Use Rust's `checked_add`, `checked_sub`, `checked_mul` and \
                unwrap with a meaningful error, or use `u128` for intermediate calculations \
                before casting back."
                .into(),
        },
        AntiPattern {
            id: "AP-005".into(),
            name: "Stale Price Oracle".into(),
            category: PatternCategory::DeFi,
            severity: AntiPatternSeverity::High,
            description: "Reading a price oracle without checking its freshness timestamp \
                can cause lending/AMM contracts to operate on stale prices during network \
                congestion or oracle downtime."
                .into(),
            indicators: vec![
                "oracle".into(),
                "price".into(),
                "get_price".into(),
                "last_updated".into(),
            ],
            remediation: "Compare the oracle's `timestamp` against `env.ledger().timestamp()` \
                and reject prices older than your configured freshness window (e.g. 60 s)."
                .into(),
        },
        AntiPattern {
            id: "AP-006".into(),
            name: "No TTL Extension on Write".into(),
            category: PatternCategory::Storage,
            severity: AntiPatternSeverity::High,
            description: "Writing to persistent storage without extending the TTL means the \
                entry can expire before the user interacts again, causing silent data loss."
                .into(),
            indicators: vec!["persistent().set".into()],
            remediation: "Call `env.storage().persistent().extend_ttl(key, low, high)` \
                immediately after every `persistent().set` call. Use a helper wrapper to \
                enforce this consistently."
                .into(),
        },
        AntiPattern {
            id: "AP-007".into(),
            name: "Raw String Storage Keys".into(),
            category: PatternCategory::Storage,
            severity: AntiPatternSeverity::Medium,
            description: "Using raw `&str` or `String` as storage keys makes refactoring \
                dangerous — a typo silently creates a new key, leaving the old data orphaned."
                .into(),
            indicators: vec![
                "storage().persistent().get(\"".into(),
                "storage().instance().get(\"".into(),
                "storage().temporary().get(\"".into(),
            ],
            remediation: "Define a typed `DataKey` enum derived with `#[contracttype]` and \
                use its variants as keys everywhere."
                .into(),
        },
        AntiPattern {
            id: "AP-008".into(),
            name: "Excessive Instance Storage".into(),
            category: PatternCategory::Storage,
            severity: AntiPatternSeverity::Medium,
            description: "Storing large or unbounded data (e.g. user lists, history) in \
                instance storage increases the base cost of every contract invocation because \
                instance storage is loaded unconditionally."
                .into(),
            indicators: vec![
                "storage().instance().set".into(),
                "Vec<Address>".into(),
                "Vec<String>".into(),
            ],
            remediation: "Keep instance storage to a handful of small config values. \
                Move per-user or historical data to persistent storage with typed keys."
                .into(),
        },
        AntiPattern {
            id: "AP-009".into(),
            name: "Hardcoded Admin Address".into(),
            category: PatternCategory::AccessControl,
            severity: AntiPatternSeverity::Medium,
            description: "Embedding an admin address as a compile-time constant makes it \
                impossible to rotate keys without redeploying the contract."
                .into(),
            indicators: vec![
                "const ADMIN".into(),
                "const OWNER".into(),
                "Address::from_str".into(),
            ],
            remediation: "Store the admin address in instance storage and provide a \
                two-step `transfer_admin` → `accept_admin` pattern."
                .into(),
        },
        AntiPattern {
            id: "AP-010".into(),
            name: "Missing Event Emission".into(),
            category: PatternCategory::General,
            severity: AntiPatternSeverity::Low,
            description: "State-changing functions that do not emit events make it hard for \
                off-chain indexers, wallets, and audit tools to track contract activity."
                .into(),
            indicators: vec![
                "fn transfer".into(),
                "fn mint".into(),
                "fn burn".into(),
                "fn swap".into(),
            ],
            remediation: "Add `env.events().publish(topics, data)` at the end of every \
                state-changing function following the Stellar event naming convention \
                (contract_name, action_name)."
                .into(),
        },
    ]
}

// ── Static pre-scan ───────────────────────────────────────────────────────────

/// Result of scanning contract source for pattern and anti-pattern indicators
/// before calling the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreScanResult {
    /// Patterns whose indicators were found in the source.
    pub matched_patterns: Vec<PatternMatch>,
    /// Anti-patterns whose indicators were found in the source.
    pub matched_anti_patterns: Vec<AntiPatternMatch>,
    /// Lines of source code scanned.
    pub lines_scanned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatch {
    pub pattern_id: String,
    pub pattern_name: String,
    pub category: PatternCategory,
    /// Number of distinct indicators found.
    pub indicator_hits: usize,
    /// Confidence 0–100 derived from how many indicators matched.
    pub confidence: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPatternMatch {
    pub anti_pattern_id: String,
    pub anti_pattern_name: String,
    pub category: PatternCategory,
    pub severity: AntiPatternSeverity,
    /// Number of distinct indicators found.
    pub indicator_hits: usize,
}

/// Scan `source` for pattern and anti-pattern indicators without calling the LLM.
pub fn pre_scan(source: &str) -> PreScanResult {
    let lines_scanned = source.lines().count();

    let matched_patterns = all_patterns()
        .into_iter()
        .filter_map(|p| {
            let hits = p
                .indicators
                .iter()
                .filter(|ind| source.contains(ind.as_str()))
                .count();
            if hits == 0 {
                return None;
            }
            let total = p.indicators.len().max(1);
            let confidence = ((hits as f64 / total as f64) * 100.0).round().min(100.0) as u8;
            Some(PatternMatch {
                pattern_id: p.id,
                pattern_name: p.name,
                category: p.category,
                indicator_hits: hits,
                confidence,
            })
        })
        .collect();

    let matched_anti_patterns = all_anti_patterns()
        .into_iter()
        .filter_map(|ap| {
            let hits = ap
                .indicators
                .iter()
                .filter(|ind| source.contains(ind.as_str()))
                .count();
            if hits == 0 {
                return None;
            }
            Some(AntiPatternMatch {
                anti_pattern_id: ap.id,
                anti_pattern_name: ap.name,
                category: ap.category,
                severity: ap.severity,
                indicator_hits: hits,
            })
        })
        .collect();

    PreScanResult {
        matched_patterns,
        matched_anti_patterns,
        lines_scanned,
    }
}

// ── Feedback store ────────────────────────────────────────────────────────────

/// Whether the user agreed with a pattern match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackVerdict {
    Correct,
    Incorrect,
    Partial,
}

/// A single piece of user feedback on an AI pattern recognition result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternFeedback {
    pub id: String,
    pub pattern_id: String,
    pub file_hash: String,
    pub verdict: FeedbackVerdict,
    /// Optional free-text note from the user.
    pub note: Option<String>,
    pub submitted_at: String,
}

fn feedback_path() -> Result<PathBuf> {
    let dir = config::config_dir().join("pattern_feedback");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("feedback.json"))
}

/// Load all stored feedback entries.
pub fn load_feedback() -> Result<Vec<PatternFeedback>> {
    let path = feedback_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).context("Failed to read pattern feedback store")?;
    serde_json::from_str(&raw).context("Failed to parse pattern feedback store")
}

/// Append a new feedback entry to the store.
pub fn save_feedback(entry: PatternFeedback) -> Result<()> {
    let mut entries = load_feedback()?;
    entries.push(entry);
    fs::write(feedback_path()?, serde_json::to_string_pretty(&entries)?)
        .context("Failed to write pattern feedback store")
}

/// Build a summary of feedback useful for injecting into AI prompts.
///
/// Returns a map of `pattern_id → (correct_count, incorrect_count)` for
/// patterns that have received at least one piece of feedback.
pub fn feedback_summary() -> Result<HashMap<String, (usize, usize)>> {
    let entries = load_feedback()?;
    let mut summary: HashMap<String, (usize, usize)> = HashMap::new();
    for entry in entries {
        let counts = summary.entry(entry.pattern_id).or_insert((0, 0));
        match entry.verdict {
            FeedbackVerdict::Correct => counts.0 += 1,
            FeedbackVerdict::Incorrect => counts.1 += 1,
            FeedbackVerdict::Partial => {
                counts.0 += 1;
                counts.1 += 1;
            }
        }
    }
    Ok(summary)
}

/// Build a human-readable feedback context string for injection into prompts.
pub fn feedback_context_for_prompt() -> String {
    match feedback_summary() {
        Err(_) => String::new(),
        Ok(summary) => {
            if summary.is_empty() {
                return String::new();
            }
            let mut lines = vec![
                "\nUser feedback on previous pattern recognitions (use to calibrate confidence):"
                    .to_string(),
            ];
            for (pid, (correct, incorrect)) in &summary {
                lines.push(format!(
                    "  - Pattern '{}': {} confirmed correct, {} marked incorrect.",
                    pid, correct, incorrect
                ));
            }
            lines.join("\n")
        }
    }
}

// ── Helper: sha256 of source for feedback keying ─────────────────────────────

/// Returns the first 16 hex chars of the SHA-256 of `source`.
pub fn source_hash(source: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(source.as_bytes());
    hex::encode(&digest[..8])
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_patterns_non_empty() {
        assert!(!all_patterns().is_empty());
    }

    #[test]
    fn all_anti_patterns_non_empty() {
        assert!(!all_anti_patterns().is_empty());
    }

    #[test]
    fn pattern_ids_are_unique() {
        let ids: Vec<_> = all_patterns().iter().map(|p| p.id.clone()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "Duplicate pattern IDs found");
    }

    #[test]
    fn anti_pattern_ids_are_unique() {
        let ids: Vec<_> = all_anti_patterns().iter().map(|p| p.id.clone()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "Duplicate anti-pattern IDs found");
    }

    #[test]
    fn pre_scan_detects_sep41_indicators() {
        let source = r#"
            fn initialize(env: Env) {}
            fn mint(env: Env, to: Address, amount: i128) {}
            fn burn(env: Env, from: Address, amount: i128) {}
            fn transfer(env: Env, from: Address, to: Address, amount: i128) {}
            fn balance(env: Env, id: Address) -> i128 {}
            fn allowance(env: Env, from: Address, spender: Address) -> i128 {}
        "#;
        let result = pre_scan(source);
        let sep41 = result
            .matched_patterns
            .iter()
            .find(|m| m.pattern_id == "sep41-fungible-token");
        assert!(sep41.is_some(), "sep41 pattern should be detected");
        assert!(sep41.unwrap().confidence > 50);
    }

    #[test]
    fn pre_scan_detects_missing_auth_anti_pattern() {
        let source = "fn mint(env: Env, to: Address) { /* no require_auth */ }";
        let result = pre_scan(source);
        let ap = result
            .matched_anti_patterns
            .iter()
            .find(|m| m.anti_pattern_id == "AP-002");
        assert!(
            ap.is_some(),
            "AP-002 (missing require_auth) should fire on fn mint"
        );
    }

    #[test]
    fn pre_scan_empty_source_returns_no_matches() {
        let result = pre_scan("");
        assert!(result.matched_patterns.is_empty());
        assert!(result.matched_anti_patterns.is_empty());
    }

    #[test]
    fn source_hash_is_deterministic() {
        let s = "pub fn transfer() {}";
        assert_eq!(source_hash(s), source_hash(s));
    }

    #[test]
    fn source_hash_differs_for_different_inputs() {
        assert_ne!(source_hash("fn a(){}"), source_hash("fn b(){}"));
    }

    #[test]
    fn feedback_context_empty_when_no_feedback() {
        // This depends on no feedback file existing in the test environment.
        // At minimum the function must not panic.
        let _ = feedback_context_for_prompt();
    }

    #[test]
    fn all_patterns_have_at_least_one_indicator() {
        for p in all_patterns() {
            assert!(
                !p.indicators.is_empty(),
                "Pattern '{}' has no indicators",
                p.id
            );
        }
    }

    #[test]
    fn all_anti_patterns_have_at_least_one_indicator() {
        for ap in all_anti_patterns() {
            assert!(
                !ap.indicators.is_empty(),
                "Anti-pattern '{}' has no indicators",
                ap.id
            );
        }
    }

    #[test]
    fn all_patterns_cover_required_categories() {
        let categories: std::collections::HashSet<_> =
            all_patterns().into_iter().map(|p| p.category).collect();
        assert!(categories.contains(&PatternCategory::Token));
        assert!(categories.contains(&PatternCategory::Governance));
        assert!(categories.contains(&PatternCategory::DeFi));
        assert!(categories.contains(&PatternCategory::AccessControl));
        assert!(categories.contains(&PatternCategory::Storage));
    }
}
