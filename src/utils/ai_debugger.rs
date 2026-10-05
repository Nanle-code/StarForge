//! AI Contract Debugging Assistant utility.
//!
//! Rule-based analysis engine that interprets Soroban contract errors,
//! identifies common bugs, suggests fixes, and provides root-cause explanations.
//! No external AI API required — all analysis is performed locally.

use serde::{Deserialize, Serialize};

// ── Severity ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Critical => "CRITICAL",
            Severity::High => "HIGH",
            Severity::Medium => "MEDIUM",
            Severity::Low => "LOW",
            Severity::Info => "INFO",
        }
    }
}

// ── Core result types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugFinding {
    pub id: String,
    pub severity: Severity,
    pub category: String,
    pub title: String,
    pub explanation: String,
    pub root_cause: String,
    pub fix_suggestion: String,
    pub reproduction_steps: Vec<String>,
    pub breakpoint_hints: Vec<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackTraceFrame {
    pub frame_index: usize,
    pub function: String,
    pub location: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugReport {
    pub input_summary: String,
    pub findings: Vec<DebugFinding>,
    pub variable_insights: Vec<String>,
    pub test_failure_analysis: Option<String>,
    pub overall_guidance: String,
    pub suggested_breakpoints: Vec<String>,
}

// ── Error pattern definitions ────────────────────────────────────────────────

struct ErrorPattern {
    keywords: &'static [&'static str],
    finding: fn() -> DebugFinding,
}

fn pattern_auth_required() -> DebugFinding {
    DebugFinding {
        id: "AUTH001".into(),
        severity: Severity::High,
        category: "Authorization".into(),
        title: "Missing or failed authorization check".into(),
        explanation: "The contract called require_auth() or require_auth_for_args() and the \
            invoker did not satisfy the authorization requirements. This means the transaction \
            signer lacks the necessary privileges for this operation."
            .into(),
        root_cause: "The caller's address did not match the expected authorized address, \
            or the required signature was absent from the transaction envelope."
            .into(),
        fix_suggestion: "Ensure the correct account signs the transaction. If writing the \
            contract, verify that env.require_auth(&caller) is placed before any state \
            mutation, and that the caller argument matches the transaction's source account."
            .into(),
        reproduction_steps: vec![
            "Invoke the contract function with an unauthorized account.".into(),
            "Observe the 'require_auth' error in the simulation result.".into(),
            "Re-invoke using the correct authorized account.".into(),
        ],
        breakpoint_hints: vec![
            "Set a breakpoint at the require_auth() call site.".into(),
            "Inspect the `caller` variable before the auth check.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/learn/smart-contract-internals/authorization"
                .into(),
        ],
    }
}

fn pattern_overflow() -> DebugFinding {
    DebugFinding {
        id: "ARITH001".into(),
        severity: Severity::Critical,
        category: "Arithmetic".into(),
        title: "Integer overflow or underflow".into(),
        explanation: "An arithmetic operation produced a value outside the valid range for \
            its integer type. In Soroban contracts, overflow panics by default in debug \
            builds and wraps silently in release builds unless checked arithmetic is used."
            .into(),
        root_cause: "A calculation exceeded i128/u128/i64/u64 bounds. Common causes: \
            token amounts added without bounds checking, loop counters, or fee calculations."
            .into(),
        fix_suggestion: "Replace raw arithmetic with checked_add / checked_sub / \
            checked_mul and return a contract error on None. Example: \
            `amount.checked_add(fee).ok_or(Error::Overflow)?`"
            .into(),
        reproduction_steps: vec![
            "Call the function with boundary values (e.g. i128::MAX for an amount).".into(),
            "Observe the panic or wrap-around in the return value.".into(),
            "Add checked arithmetic and verify the error is returned cleanly.".into(),
        ],
        breakpoint_hints: vec![
            "Set a breakpoint just before the arithmetic expression.".into(),
            "Inspect operand values to confirm they are near type limits.".into(),
        ],
        references: vec![
            "https://doc.rust-lang.org/std/primitive.i128.html#method.checked_add".into(),
        ],
    }
}

fn pattern_storage_missing() -> DebugFinding {
    DebugFinding {
        id: "STORE001".into(),
        severity: Severity::Medium,
        category: "Storage".into(),
        title: "Contract storage key not found".into(),
        explanation: "The contract attempted to read a storage entry that has not been \
            written yet, or the entry has expired (TTL elapsed). Soroban storage is \
            persistent, temporary, or instance-scoped, and each has different TTL rules."
            .into(),
        root_cause: "Either the contract was not initialized before calling this function, \
            or a persistent entry's ledger TTL expired and was archived."
            .into(),
        fix_suggestion: "Guard all storage reads with `env.storage().persistent().has(&key)` \
            before reading. Call an `initialize()` function to seed required state. \
            For persistent storage, extend TTL with \
            `env.storage().persistent().extend_ttl(&key, min, max)`."
            .into(),
        reproduction_steps: vec![
            "Deploy the contract fresh (no prior state).".into(),
            "Call the function that reads storage without calling initialize() first.".into(),
            "Observe the missing-key panic.".into(),
        ],
        breakpoint_hints: vec![
            "Break before the storage get() call.".into(),
            "Use `starforge inspect` to view live storage state.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/learn/smart-contract-internals/persisting-data"
                .into(),
        ],
    }
}

fn pattern_insufficient_balance() -> DebugFinding {
    DebugFinding {
        id: "TOKEN001".into(),
        severity: Severity::High,
        category: "Token / Balance".into(),
        title: "Insufficient token balance".into(),
        explanation: "The account or contract does not hold enough tokens to complete \
            the requested transfer or operation. The Soroban token interface enforces \
            balance constraints at the host level."
            .into(),
        root_cause: "The sender's balance is less than the amount being transferred, \
            or fees were not accounted for when computing available balance."
            .into(),
        fix_suggestion: "Check the balance with `token_client.balance(&sender)` before \
            initiating a transfer. Add a guard: \
            `if balance < amount { return Err(Error::InsufficientFunds); }`"
            .into(),
        reproduction_steps: vec![
            "Attempt to transfer more tokens than the sender holds.".into(),
            "Observe the error from the SEP-41 token contract.".into(),
            "Fund the account and retry.".into(),
        ],
        breakpoint_hints: vec![
            "Inspect the `balance` variable before the transfer call.".into(),
            "Log sender address and amount to confirm they are correct.".into(),
        ],
        references: vec!["https://developers.stellar.org/docs/tokens/token-interface".into()],
    }
}

fn pattern_panic() -> DebugFinding {
    DebugFinding {
        id: "PANIC001".into(),
        severity: Severity::High,
        category: "Runtime Panic".into(),
        title: "Contract execution panicked".into(),
        explanation: "The Soroban host caught a Rust panic inside the contract. This \
            surfaces as a generic host error in simulation output. Panics abort the \
            entire invocation and roll back all state changes."
            .into(),
        root_cause: "Common sources: unwrap() on None/Err, index out of bounds, \
            failed assertion (assert! / panic!), or integer overflow in release mode."
            .into(),
        fix_suggestion: "Replace all `.unwrap()` calls with proper error handling using \
            `?` or `.ok_or(ContractError::...)`. Replace `assert!` with explicit \
            `if` guards that return a typed error. Enable `overflow-checks = true` \
            in Cargo.toml `[profile.release]`."
            .into(),
        reproduction_steps: vec![
            "Run the contract invocation via `starforge debug start`.".into(),
            "Look for 'panicked at' in the error output or host logs.".into(),
            "Identify the unwrap/assert site and replace with error propagation.".into(),
        ],
        breakpoint_hints: vec![
            "Set breakpoints before each unwrap() call.".into(),
            "Inspect the Option/Result value before unwrapping.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/learn/smart-contract-internals/errors".into(),
        ],
    }
}

fn pattern_wasm_invalid() -> DebugFinding {
    DebugFinding {
        id: "WASM001".into(),
        severity: Severity::Critical,
        category: "WASM / Build".into(),
        title: "Invalid or corrupt WASM binary".into(),
        explanation: "The WASM file could not be parsed or validated by the Soroban host. \
            This can happen if the file is truncated, built with incompatible settings, \
            or is not a Soroban contract at all."
            .into(),
        root_cause: "The binary was not compiled for a Soroban-compatible WASM target, \
            or build artifacts are stale after a `cargo clean`."
            .into(),
        fix_suggestion: "Rebuild from source: `stellar contract build`. Verify the output \
            path ends in `.wasm`. Run `wasm-validate <file>` to confirm the binary is \
            valid WebAssembly. Use `--optimize` for production deployments."
            .into(),
        reproduction_steps: vec![
            "Attempt to deploy or simulate the suspect WASM file.".into(),
            "Note the 'invalid wasm' error from the host.".into(),
            "Run `stellar contract build` and retry with the fresh binary.".into(),
        ],
        breakpoint_hints: vec![
            "Check file size — a valid Soroban WASM is rarely under 1 KB.".into(),
            "Run `xxd <file> | head` to verify the \\0asm magic header.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/build/smart-contracts/getting-started/setup"
                .into(),
        ],
    }
}

fn pattern_contract_not_found() -> DebugFinding {
    DebugFinding {
        id: "NET001".into(),
        severity: Severity::High,
        category: "Network / Contract".into(),
        title: "Contract not found on network".into(),
        explanation: "The contract ID supplied does not exist on the target network, or \
            the contract has been deleted / archived. This produces a 'not found' error \
            during simulation or invocation."
            .into(),
        root_cause: "Wrong network selected (e.g. using a testnet ID on mainnet), \
            the contract was never deployed, or the deployment used a different account \
            than expected."
            .into(),
        fix_suggestion: "Verify the active network with `starforge network show`. \
            Confirm the contract ID by inspecting recent deployments: \
            `starforge deployments list`. Redeploy if necessary."
            .into(),
        reproduction_steps: vec![
            "Try to invoke the contract using its ID.".into(),
            "Observe the 'contract not found' error.".into(),
            "Switch to the correct network and retry.".into(),
        ],
        breakpoint_hints: vec![
            "Print the contract ID and network before invocation.".into(),
            "Use `starforge contract inspect <id>` to verify existence.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/data/rpc/api-reference/methods/getLedgerEntries"
                .into(),
        ],
    }
}

fn pattern_ttl_expired() -> DebugFinding {
    DebugFinding {
        id: "TTL001".into(),
        severity: Severity::Medium,
        category: "Storage / TTL".into(),
        title: "Storage entry TTL has expired".into(),
        explanation: "A persistent or temporary storage entry was not accessed or extended \
            within its time-to-live window and has been archived by the Soroban host. \
            Archived entries cannot be read until restored."
            .into(),
        root_cause: "The contract did not call `extend_ttl` frequently enough. \
            High-throughput contracts that go quiet for many ledgers are at risk."
            .into(),
        fix_suggestion: "In read-heavy functions, proactively extend TTL: \
            `env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL)`. \
            Consider a background keeper job that touches keys regularly."
            .into(),
        reproduction_steps: vec![
            "Deploy a contract and write a persistent entry.".into(),
            "Advance the ledger past the TTL (use sandbox time controls).".into(),
            "Read the entry and observe the 'entry archived' error.".into(),
        ],
        breakpoint_hints: vec![
            "Inspect `env.ledger().sequence()` relative to the entry's TTL.".into(),
            "Log TTL values after each extend_ttl call.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/learn/smart-contract-internals/state-archival"
                .into(),
        ],
    }
}

fn pattern_test_failure() -> DebugFinding {
    DebugFinding {
        id: "TEST001".into(),
        severity: Severity::Medium,
        category: "Test Failure".into(),
        title: "Contract test assertion failed".into(),
        explanation: "A Rust test for this contract failed. The test expected a specific \
            return value or state but the contract produced different output. This may \
            indicate a logic error, incorrect mock setup, or a changed function signature."
            .into(),
        root_cause: "The most common causes are: wrong argument ordering, state not reset \
            between test cases, or a business logic condition that was not accounted for \
            in the test setup."
            .into(),
        fix_suggestion: "Run `cargo test -- --nocapture` to see full output. Add \
            `println!` or `eprintln!` inside the contract (debug builds only) to trace \
            values. Verify the `TestClient` is constructed from a fresh `Env::default()` \
            for each test."
            .into(),
        reproduction_steps: vec![
            "Run `cargo test <test_name>` in the contract crate.".into(),
            "Inspect the 'left' and 'right' values in the assertion output.".into(),
            "Add intermediate assertions to isolate the diverging value.".into(),
        ],
        breakpoint_hints: vec![
            "Break at the function entry inside the contract under test.".into(),
            "Inspect all arguments passed by the test client.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/build/smart-contracts/example-contracts/hello-world#writing-tests".into(),
        ],
    }
}

fn pattern_type_mismatch() -> DebugFinding {
    DebugFinding {
        id: "TYPE001".into(),
        severity: Severity::Medium,
        category: "Type / ABI".into(),
        title: "Argument type mismatch or ABI incompatibility".into(),
        explanation: "The supplied argument could not be converted to the type expected by \
            the contract function. Soroban performs strict XDR type-checking at the host \
            boundary."
            .into(),
        root_cause: "A caller passed a string where an address was expected, used the \
            wrong numeric type (i64 vs u128), or the contract was upgraded with a \
            different function signature than the client was compiled against."
            .into(),
        fix_suggestion: "Check the contract ABI with `starforge contract inspect <id>`. \
            Regenerate language bindings with `starforge contract generate-bindings`. \
            Ensure the types in your invocation match the contract's `#[contracttype]` \
            definitions."
            .into(),
        reproduction_steps: vec![
            "Invoke the contract with a mismatched argument type.".into(),
            "Observe the XDR conversion error.".into(),
            "Correct the argument type and reinvoke.".into(),
        ],
        breakpoint_hints: vec![
            "Log the raw XDR of each argument before invocation.".into(),
            "Compare argument types against the contract source or ABI spec.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/learn/smart-contract-internals/contract-interactions/cross-contract".into(),
        ],
    }
}

// ── Deployment-specific error patterns (Issue #542) ──────────────────────────

fn pattern_deployment_network_error() -> DebugFinding {
    DebugFinding {
        id: "DEPLOY001".into(),
        severity: Severity::High,
        category: "Deployment / Network".into(),
        title: "Deployment failed due to network connectivity".into(),
        explanation: "The deployment transaction could not be submitted to the Stellar \
            network or Soroban RPC endpoint. This typically indicates network connectivity \
            issues, incorrect RPC URL, or the network being down."
            .into(),
        root_cause: "Common causes: no internet connection, incorrect network configuration, \
            Stellar testnet/mainnet is experiencing downtime, or firewall blocking the RPC endpoint."
            .into(),
        fix_suggestion: "Verify network connectivity with `starforge network test`. \
            Check the network configuration with `starforge network show`. \
            Ensure the RPC endpoint URL is correct and accessible. \
            For testnet, check https://status.stellar.org for service status."
            .into(),
        reproduction_steps: vec![
            "Attempt to deploy with incorrect network configuration.".into(),
            "Observe connection timeout or network error.".into(),
            "Verify network settings and retry deployment.".into(),
        ],
        breakpoint_hints: vec![
            "Check network configuration in ~/.starforge/config.toml".into(),
            "Test RPC endpoint connectivity with curl or starforge network test".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/networks".into(),
        ],
    }
}

fn pattern_deployment_wasm_size_exceeded() -> DebugFinding {
    DebugFinding {
        id: "DEPLOY002".into(),
        severity: Severity::Critical,
        category: "Deployment / WASM".into(),
        title: "WASM binary exceeds size limit".into(),
        explanation: "The compiled WASM contract exceeds the Soroban size limit of 128KB. \
            Contracts larger than this cannot be deployed to the network."
            .into(),
        root_cause: "The contract contains too much code, excessive dependencies, or \
            was not optimized before deployment. Debug builds are significantly larger \
            than release builds."
            .into(),
        fix_suggestion: "Build with `starforge contract build`. \
            Use starforge's built-in optimizer: `starforge deploy --optimize`. \
            Remove unused dependencies from Cargo.toml. Consider splitting large contracts \
            into multiple smaller contracts."
            .into(),
        reproduction_steps: vec![
            "Build a large contract in debug mode.".into(),
            "Attempt deployment without optimization.".into(),
            "Observe size limit error.".into(),
            "Apply optimization and retry.".into(),
        ],
        breakpoint_hints: vec![
            "Check WASM file size under target/<selected-target>/release/*.wasm".into(),
            "Run wasm-opt with --optimize flag for maximum size reduction.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/build/smart-contracts/getting-started/deploy-to-testnet".into(),
        ],
    }
}

fn pattern_deployment_insufficient_funds() -> DebugFinding {
    DebugFinding {
        id: "DEPLOY003".into(),
        severity: Severity::High,
        category: "Deployment / Wallet".into(),
        title: "Insufficient funds for deployment transaction".into(),
        explanation: "The wallet account does not have enough XLM to pay for the deployment \
            transaction fees. Soroban contract deployment requires XLM for transaction fees \
            and resource costs."
            .into(),
        root_cause: "The source account balance is below the required amount for deployment fees. \
            Deployment transactions typically cost 1000-5000 stroops (0.0001-0.0005 XLM) plus \
            resource costs."
            .into(),
        fix_suggestion: "Fund the wallet account before deployment. For testnet, use friendbot: \
            `starforge wallet fund --wallet <name>`. For mainnet, transfer XLM from another \
            account. Check account balance with `starforge wallet show <name>`."
            .into(),
        reproduction_steps: vec![
            "Attempt to deploy with an unfunded or low-balance account.".into(),
            "Observe insufficient funds error from Horizon.".into(),
            "Fund the account and retry deployment.".into(),
        ],
        breakpoint_hints: vec![
            "Check account balance before deployment.".into(),
            "Verify base fee and resource costs in simulation output.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/learn/fundamentals/fees-resource-limits-metering"
                .into(),
        ],
    }
}

fn pattern_deployment_transaction_failed() -> DebugFinding {
    DebugFinding {
        id: "DEPLOY004".into(),
        severity: Severity::Critical,
        category: "Deployment / Transaction".into(),
        title: "Deployment transaction failed on-chain".into(),
        explanation: "The deployment transaction was submitted successfully but failed during \
            execution on the Stellar network. This indicates the transaction was malformed, \
            lacked proper authorization, or violated network constraints."
            .into(),
        root_cause: "Common causes: incorrect source account, missing signatures, sequence \
            number mismatch, invalid operation parameters, or failed precondition checks."
            .into(),
        fix_suggestion: "Check the transaction result code in the error message. \
            Verify the source account has the correct sequence number. \
            Ensure the wallet is properly authorized to deploy. \
            Use `starforge deploy --simulate` to test the transaction before submission. \
            Check deployment history for recent failures: `starforge deployments history`."
            .into(),
        reproduction_steps: vec![
            "Review the transaction result XDR for error details.".into(),
            "Verify wallet authorization and account state.".into(),
            "Simulate the deployment before execution.".into(),
        ],
        breakpoint_hints: vec![
            "Inspect transaction envelope and result XDR.".into(),
            "Check source account sequence number matches network state.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/data/horizon/api-reference/errors/http-status-codes/standard".into(),
        ],
    }
}

fn pattern_deployment_wasm_hash_mismatch() -> DebugFinding {
    DebugFinding {
        id: "DEPLOY005".into(),
        severity: Severity::Medium,
        category: "Deployment / Verification".into(),
        title: "Deployed WASM hash doesn't match expected hash".into(),
        explanation: "The WASM hash of the deployed contract on-chain does not match the \
            hash of the local WASM file. This suggests the wrong file was deployed, the file \
            was modified after deployment, or the deployment was corrupted."
            .into(),
        root_cause: "Mismatch between local build artifacts and deployed bytecode. Possible \
            causes: deploying a different WASM file than intended, stale build cache, or \
            file corruption during deployment."
            .into(),
        fix_suggestion: "Rebuild the contract from source: `cargo clean && cargo build --release`. \
            Verify the WASM hash before deployment: `sha256sum <wasm-file>`. \
            Use `starforge deployments verify --id <deployment-id>` to check on-chain hash. \
            Redeploy with the correct WASM file if verification fails."
            .into(),
        reproduction_steps: vec![
            "Deploy a contract and note the deployment ID.".into(),
            "Verify the deployment with starforge deployments verify.".into(),
            "Compare expected vs actual WASM hash.".into(),
        ],
        breakpoint_hints: vec![
            "Compute the local WASM hash from target/<selected-target>/release/*.wasm".into(),
            "Fetch on-chain WASM hash from contract metadata.".into(),
        ],
        references: vec![
            "https://developers.stellar.org/docs/build/smart-contracts/getting-started/deploy-to-testnet#verification".into(),
        ],
    }
}

// ── Compilation & configuration error patterns (Issue #511) ─────────────────

fn pattern_compilation_error() -> DebugFinding {
    DebugFinding {
        id: "COMPILE001".into(),
        severity: Severity::High,
        category: "Compilation".into(),
        title: "Rust compiler error in contract source".into(),
        explanation: "The contract source failed to compile. This is typically a type \
            mismatch, an unresolved import/module path, a missing trait implementation, or \
            a borrow-checker violation reported by `rustc`/`cargo build`."
            .into(),
        root_cause: "Common causes: mismatched types between a function's declared return \
            type and its actual returned value (E0308), a `use` path that doesn't resolve \
            to an existing module/item (E0433), a missing `derive` or trait bound required by \
            the soroban_sdk macros, or moving a value that is used again afterwards."
            .into(),
        fix_suggestion: "Run `starforge contract build` \
            and read the first reported error — later errors are often just consequences of \
            the first one. For E0308, check the exact types on both sides of the mismatch. \
            For E0433/E0432, verify the `use` path and that the target module is declared \
            with `pub mod` in its parent `mod.rs`. For borrow errors, consider cloning the \
            value or restructuring so it is moved only once."
            .into(),
        reproduction_steps: vec![
            "Run `starforge contract build`.".into(),
            "Copy the first `error[EXXXX]` block reported.".into(),
            "Apply the compiler's suggested fix, then rebuild.".into(),
        ],
        breakpoint_hints: vec![
            "Use `cargo check` for faster iteration than a full build.".into(),
            "Run `rustc --explain EXXXX` for a detailed explanation of any error code.".into(),
        ],
        references: vec!["https://doc.rust-lang.org/error_codes/error-index.html".into()],
    }
}

fn pattern_configuration_error() -> DebugFinding {
    DebugFinding {
        id: "CONFIG001".into(),
        severity: Severity::Medium,
        category: "Configuration".into(),
        title: "Invalid or missing StarForge configuration".into(),
        explanation: "StarForge could not read a required configuration value — typically \
            an unset network entry, a malformed `~/.starforge/config.toml`, a missing wallet, \
            or a required environment variable that isn't set."
            .into(),
        root_cause: "Common causes: `config.toml` has invalid TOML syntax or a network name \
            that isn't defined under `[networks]`, the active `--network` doesn't match any \
            configured network, a referenced wallet was never created, or an env var like \
            `STARFORGE_AI_API_KEY` / `ANTHROPIC_API_KEY` is required but unset."
            .into(),
        fix_suggestion: "Run `starforge config show` to inspect the current configuration. \
            Validate network settings with `starforge network show`. Re-create missing \
            wallets with `starforge wallet create <name>`. If an API-key error is shown, \
            export the required environment variable before retrying."
            .into(),
        reproduction_steps: vec![
            "Run a command that depends on config (e.g. `starforge deploy`).".into(),
            "Observe the configuration/parse error.".into(),
            "Run `starforge config show` to locate the invalid or missing value.".into(),
        ],
        breakpoint_hints: vec![
            "Inspect `~/.starforge/config.toml` for syntax errors.".into(),
            "Check for required environment variables referenced in the error message.".into(),
        ],
        references: vec!["https://developers.stellar.org/docs/networks".into()],
    }
}

// ── Pattern registry ─────────────────────────────────────────────────────────

fn all_patterns() -> Vec<ErrorPattern> {
    vec![
        ErrorPattern {
            keywords: &[
                "require_auth",
                "auth",
                "unauthorized",
                "not authorized",
                "auth failed",
            ],
            finding: pattern_auth_required,
        },
        ErrorPattern {
            keywords: &[
                "overflow",
                "underflow",
                "attempt to add with overflow",
                "attempt to subtract with overflow",
                "attempt to multiply with overflow",
            ],
            finding: pattern_overflow,
        },
        ErrorPattern {
            keywords: &[
                "not found",
                "missing key",
                "storage",
                "no entry",
                "key not found",
                "missing storage",
            ],
            finding: pattern_storage_missing,
        },
        ErrorPattern {
            keywords: &[
                "insufficient",
                "balance",
                "not enough",
                "insufficient funds",
                "insufficient balance",
            ],
            finding: pattern_insufficient_balance,
        },
        ErrorPattern {
            keywords: &[
                "panic",
                "panicked",
                "unwrap",
                "called `option::unwrap` on a `none`",
                "called `result::unwrap` on an `err`",
            ],
            finding: pattern_panic,
        },
        ErrorPattern {
            keywords: &[
                "invalid wasm",
                "wasm",
                "webassembly",
                "binary",
                "magic",
                "malformed",
            ],
            finding: pattern_wasm_invalid,
        },
        ErrorPattern {
            keywords: &[
                "contract not found",
                "no contract",
                "does not exist",
                "ledger entry not found",
            ],
            finding: pattern_contract_not_found,
        },
        ErrorPattern {
            keywords: &[
                "ttl",
                "expired",
                "archived",
                "state archival",
                "entry expired",
            ],
            finding: pattern_ttl_expired,
        },
        ErrorPattern {
            keywords: &[
                "test",
                "assert",
                "assertion",
                "expected",
                "left =",
                "right =",
                "#[test]",
                "failed",
            ],
            finding: pattern_test_failure,
        },
        ErrorPattern {
            keywords: &[
                "type",
                "abi",
                "xdr",
                "conversion",
                "mismatch",
                "invalid argument",
                "wrong type",
            ],
            finding: pattern_type_mismatch,
        },
        // Deployment-specific patterns (Issue #542)
        ErrorPattern {
            keywords: &[
                "network",
                "connection",
                "timeout",
                "unreachable",
                "rpc",
                "horizon",
                "failed to connect",
            ],
            finding: pattern_deployment_network_error,
        },
        ErrorPattern {
            keywords: &[
                "size",
                "limit",
                "too large",
                "exceeds",
                "128",
                "kb",
                "wasm size",
            ],
            finding: pattern_deployment_wasm_size_exceeded,
        },
        ErrorPattern {
            keywords: &[
                "insufficient funds",
                "low balance",
                "not enough xlm",
                "underfunded",
                "account balance",
            ],
            finding: pattern_deployment_insufficient_funds,
        },
        ErrorPattern {
            keywords: &[
                "transaction failed",
                "tx failed",
                "bad sequence",
                "operation failed",
                "result code",
            ],
            finding: pattern_deployment_transaction_failed,
        },
        ErrorPattern {
            keywords: &[
                "hash mismatch",
                "wasm hash",
                "verification failed",
                "bytecode mismatch",
                "checksum",
            ],
            finding: pattern_deployment_wasm_hash_mismatch,
        },
        // Compilation & configuration patterns (Issue #511)
        ErrorPattern {
            keywords: &[
                "error[e",
                "expected `",
                "mismatched types",
                "cannot find",
                "unresolved import",
                "the trait bound",
                "borrow checker",
                "cargo build",
                "rustc",
                "compil",
            ],
            finding: pattern_compilation_error,
        },
        ErrorPattern {
            keywords: &[
                "config.toml",
                "configuration",
                "unknown network",
                "no such network",
                "config not found",
                "missing wallet",
                "environment variable",
                "api key not configured",
                "api_key not configured",
            ],
            finding: pattern_configuration_error,
        },
    ]
}

// ── Stack trace parser ────────────────────────────────────────────────────────

/// Parse a raw stack trace string into structured frames.
pub fn parse_stack_trace(trace: &str) -> Vec<StackTraceFrame> {
    let mut frames = Vec::new();
    for (i, line) in trace.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Try to parse lines like "  0: function_name at src/lib.rs:42"
        let (function, location) = if let Some(at_pos) = line.find(" at ") {
            let func_part = line[..at_pos]
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == ':' || c == ' ');
            let loc_part = &line[at_pos + 4..];
            (func_part.to_string(), Some(loc_part.to_string()))
        } else {
            let func_part =
                line.trim_start_matches(|c: char| c.is_ascii_digit() || c == ':' || c == ' ');
            (func_part.to_string(), None)
        };

        if !function.is_empty() {
            frames.push(StackTraceFrame {
                frame_index: i,
                function,
                location,
                context: None,
            });
        }
    }
    frames
}

// ── Variable state inspector ──────────────────────────────────────────────────

/// Produce human-readable insights about variable values passed as name=value pairs.
pub fn inspect_variable_state(variables: &[(String, String)]) -> Vec<String> {
    let mut insights = Vec::new();
    for (name, value) in variables {
        let name_lower = name.to_lowercase();
        let value_lower = value.to_lowercase();

        // Detect potential zero-value bugs
        if (value == "0" || value == "0i128" || value == "0u128")
            && (name_lower.contains("amount")
                || name_lower.contains("balance")
                || name_lower.contains("fee"))
        {
            insights.push(format!(
                "⚠  '{}' is zero — confirm this is intentional for a value-carrying field.",
                name
            ));
        }

        // Detect max-value boundary conditions
        if value.contains("170141183460469231731687303715884105727")
            || value.contains("i128::MAX")
            || value.contains("9223372036854775807")
        {
            insights.push(format!(
                "⚠  '{}' is at or near integer maximum — arithmetic on this value will overflow.",
                name
            ));
        }

        // Detect empty / null-like address
        if (name_lower.contains("address") || name_lower.contains("account"))
            && (value_lower.contains("none") || value == "\"\"" || value.is_empty())
        {
            insights.push(format!(
                "✗  '{}' is empty or None — the contract will likely fail auth checks.",
                name
            ));
        }

        // Detect very large collections
        if name_lower.contains("vec") || name_lower.contains("map") || name_lower.contains("list") {
            if let Ok(n) = value
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<usize>()
            {
                if n > 1000 {
                    insights.push(format!(
                        "ℹ  '{}' has {} elements — consider gas cost implications for large collections.",
                        name, n
                    ));
                }
            }
        }
    }

    if insights.is_empty() {
        insights.push("No suspicious variable states detected.".into());
    }
    insights
}

// ── Core analysis engine ──────────────────────────────────────────────────────

/// Analyse an error message against all known patterns and return matching findings.
fn analyse_error_message(error_msg: &str) -> Vec<DebugFinding> {
    let lower = error_msg.to_lowercase();
    let patterns = all_patterns();
    let mut findings = Vec::new();

    for pattern in &patterns {
        if pattern.keywords.iter().any(|kw| lower.contains(kw)) {
            findings.push((pattern.finding)());
        }
    }

    // Deduplicate by id
    findings.dedup_by_key(|f| f.id.clone());
    findings
}

/// Build the overall guidance string from findings.
fn build_overall_guidance(findings: &[DebugFinding], has_stack_trace: bool) -> String {
    if findings.is_empty() {
        let mut msg = "No specific pattern matched the provided input. \
            Try the following general debugging steps:\n\
            1. Run `cargo test -- --nocapture` for detailed output.\n\
            2. Use `starforge debug start --wasm <path>` to step through execution.\n\
            3. Check `starforge audit <path>` for static analysis findings."
            .to_string();
        if !has_stack_trace {
            msg.push_str("\n4. Provide a stack trace with --stack-trace for deeper analysis.");
        }
        return msg;
    }

    let critical = findings
        .iter()
        .filter(|f| f.severity == Severity::Critical)
        .count();
    let high = findings
        .iter()
        .filter(|f| f.severity == Severity::High)
        .count();

    let priority = if critical > 0 {
        format!(
            "{} critical issue(s) require immediate attention.",
            critical
        )
    } else if high > 0 {
        format!("{} high-severity issue(s) detected.", high)
    } else {
        "Issues detected are medium or low severity.".into()
    };

    format!(
        "{} Address findings in order of severity (CRITICAL → HIGH → MEDIUM). \
        Each finding includes a fix suggestion and reproduction steps to verify the fix.",
        priority
    )
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Analyse a contract error, optional stack trace, and optional variable state.
///
/// Returns a full [`DebugReport`] with findings, guidance, and breakpoint hints.
pub fn analyse(
    error_message: &str,
    stack_trace: Option<&str>,
    variables: Option<&[(String, String)]>,
    test_output: Option<&str>,
) -> DebugReport {
    let input_summary = format!(
        "Error: {}{}{}",
        &error_message[..error_message.len().min(120)],
        if stack_trace.is_some() {
            " | Stack trace provided"
        } else {
            ""
        },
        if variables.map(|v| !v.is_empty()).unwrap_or(false) {
            " | Variables provided"
        } else {
            ""
        },
    );

    // 1. Error pattern matching
    let mut findings = analyse_error_message(error_message);

    // 2. Stack trace analysis — augment findings with source locations
    let stack_frames = stack_trace.map(parse_stack_trace).unwrap_or_default();
    if !stack_frames.is_empty() && findings.is_empty() {
        // No error message match — scan frames for known trouble spots
        let frame_text: String = stack_frames
            .iter()
            .map(|f| format!("{} {}", f.function, f.location.as_deref().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");
        findings = analyse_error_message(&frame_text);
    }

    // 3. Test output analysis
    let test_failure_analysis = test_output.map(|output| {
        let lower = output.to_lowercase();
        let extra_findings = analyse_error_message(&lower);
        if extra_findings.is_empty() {
            // Try to extract assertion details
            let left = extract_between(output, "left: `", "`");
            let right = extract_between(output, "right: `", "`");
            match (left, right) {
                (Some(l), Some(r)) => format!(
                    "Test assertion failed: expected `{}` but got `{}`. \
                    Check the logic path that produces the actual value.",
                    r, l
                ),
                _ => "Test failed. Run with --nocapture for full output.".into(),
            }
        } else {
            format!(
                "Test output matched {} known issue pattern(s). See findings for details.",
                extra_findings.len()
            )
        }
    });

    // 4. Variable state inspection
    let variable_insights = variables.map(inspect_variable_state).unwrap_or_default();

    // 5. Collect all suggested breakpoints
    let suggested_breakpoints: Vec<String> = findings
        .iter()
        .flat_map(|f| f.breakpoint_hints.iter().cloned())
        .collect();

    let overall_guidance = build_overall_guidance(&findings, stack_trace.is_some());

    DebugReport {
        input_summary,
        findings,
        variable_insights,
        test_failure_analysis,
        overall_guidance,
        suggested_breakpoints,
    }
}

/// Helper to extract a substring between two delimiters.
fn extract_between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_pos = s.find(start)? + start.len();
    let end_pos = s[start_pos..].find(end)? + start_pos;
    Some(&s[start_pos..end_pos])
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_auth_error() {
        let report = analyse("Error: require_auth failed for address", None, None, None);
        assert!(!report.findings.is_empty(), "should detect auth finding");
        assert_eq!(report.findings[0].id, "AUTH001");
    }

    #[test]
    fn detects_overflow_error() {
        let report = analyse(
            "attempt to add with overflow in balance calculation",
            None,
            None,
            None,
        );
        assert!(report.findings.iter().any(|f| f.id == "ARITH001"));
    }

    #[test]
    fn detects_storage_missing() {
        let report = analyse("storage key not found: DataKey::Balance", None, None, None);
        assert!(report.findings.iter().any(|f| f.id == "STORE001"));
    }

    #[test]
    fn detects_compilation_error() {
        let report = analyse(
            "error[E0308]: mismatched types, expected `u64`, found `i128`",
            None,
            None,
            None,
        );
        assert!(report.findings.iter().any(|f| f.id == "COMPILE001"));
    }

    #[test]
    fn detects_configuration_error() {
        let report = analyse(
            "failed to load config.toml: unknown network 'foo'",
            None,
            None,
            None,
        );
        assert!(report.findings.iter().any(|f| f.id == "CONFIG001"));
    }

    #[test]
    fn no_match_returns_general_guidance() {
        let report = analyse("some completely unknown error xyz123", None, None, None);
        assert!(report.findings.is_empty());
        assert!(report.overall_guidance.contains("general debugging"));
    }

    #[test]
    fn parses_stack_trace_frames() {
        let trace = "  0: contract::transfer at src/lib.rs:42\n  1: host::invoke at host.rs:10";
        let frames = parse_stack_trace(trace);
        assert_eq!(frames.len(), 2);
        assert!(frames[0].function.contains("contract::transfer"));
        assert!(frames[0]
            .location
            .as_deref()
            .unwrap_or("")
            .contains("src/lib.rs:42"));
    }

    #[test]
    fn variable_inspection_flags_zero_amount() {
        let vars = vec![("amount".to_string(), "0".to_string())];
        let insights = inspect_variable_state(&vars);
        assert!(insights.iter().any(|i| i.contains("zero")));
    }

    #[test]
    fn variable_inspection_ok_for_normal_values() {
        let vars = vec![("amount".to_string(), "1000".to_string())];
        let insights = inspect_variable_state(&vars);
        assert_eq!(insights[0], "No suspicious variable states detected.");
    }

    #[test]
    fn test_failure_analysis_extracts_assertion_values() {
        let output = "thread 'main' panicked: assertion `left == right` failed\n  left: `42`\n  right: `100`";
        let report = analyse("assertion failed", None, None, Some(output));
        assert!(report.test_failure_analysis.is_some());
    }

    #[test]
    fn report_lists_breakpoint_hints_for_findings() {
        let report = analyse("require_auth failed", None, None, None);
        assert!(!report.suggested_breakpoints.is_empty());
    }
}
