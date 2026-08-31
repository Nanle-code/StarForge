//! Standardized CLI exit codes and classification logic.
//!
//! Issue #672: Maps usage, configuration, network, signing, execution,
//! and environment failures to stable exit codes.

use std::fmt;

/// Standardized CLI exit codes for StarForge commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ExitCode {
    /// Command completed successfully (0).
    Success = 0,
    /// General unclassified failure (1).
    GeneralFailure = 1,
    /// Usage or input validation error (2) — e.g. bad CLI arguments, invalid correlation ID.
    Usage = 2,
    /// Configuration error (3) — e.g. missing/corrupted config file, unsupported network, bad wallet name.
    Config = 3,
    /// Network or RPC failure (4) — e.g. connection timeout, Horizon HTTP error, Friendbot unreachable.
    Network = 4,
    /// Signing or cryptographic failure (5) — e.g. invalid keypair, secret decryption passphrase mismatch.
    Signing = 5,
    /// Contract or WASM execution error (6) — e.g. build failure, transaction revert, WASM hash mismatch.
    Execution = 6,
    /// System or dependency error (7) — e.g. missing Docker/curl, permission denied, unsupported OS/arch.
    Environment = 7,
}

impl ExitCode {
    /// Return the numeric i32 code associated with this exit state.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Return the machine-readable name of the exit code.
    pub fn name(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::GeneralFailure => "GENERAL_FAILURE",
            Self::Usage => "USAGE_ERROR",
            Self::Config => "CONFIG_ERROR",
            Self::Network => "NETWORK_ERROR",
            Self::Signing => "SIGNING_ERROR",
            Self::Execution => "EXECUTION_ERROR",
            Self::Environment => "ENVIRONMENT_ERROR",
        }
    }

    /// Return a human-readable description of the exit code.
    pub fn description(self) -> &'static str {
        match self {
            Self::Success => "Command executed successfully",
            Self::GeneralFailure => "General runtime or execution failure",
            Self::Usage => "Invalid CLI arguments, syntax, or input parameter",
            Self::Config => "Configuration file missing, invalid, or schema migration failed",
            Self::Network => "Network connectivity, RPC node, or Horizon request failure",
            Self::Signing => "Cryptographic keypair, secret decryption, or signature failure",
            Self::Execution => "Contract WASM compilation, verification, or transaction revert",
            Self::Environment => {
                "Missing system dependency, permission denied, or unsupported host OS/arch"
            }
        }
    }

    /// Immediately exit the current process with this exit code.
    pub fn exit(self) -> ! {
        std::process::exit(self.code())
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (code {})", self.name(), self.code())
    }
}

/// Classify an `anyhow::Error` into a stable [`ExitCode`].
///
/// Inspects error messages and cause chains to match domain patterns.
pub fn determine_exit_code(err: &anyhow::Error) -> ExitCode {
    let msg = err.to_string().to_lowercase();
    let full_chain_msg = err
        .chain()
        .map(|c| c.to_string().to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    // 1. Usage / Input Validation (Code 2)
    if msg.contains("invalid correlation id")
        || msg.contains("invalid public key") && msg.contains("56 characters")
        || msg.contains("invalid argument")
        || msg.contains("unknown subcommand")
        || msg.contains("missing required")
        || msg.contains("invalid flag")
        || msg.contains("unsupported flag")
    {
        return ExitCode::Usage;
    }

    // 2. Signing / Crypto (Code 5) — checked before Config so secret errors aren't misclassified
    if msg.contains("secret key")
        || msg.contains("passphrase")
        || msg.contains("decrypt")
        || msg.contains("encrypted secret")
        || msg.contains("invalid secret")
        || msg.contains("signature")
        || msg.contains("keypair")
        || msg.contains("ed25519")
        || msg.contains("wallet encryption")
    {
        return ExitCode::Signing;
    }

    // 3. Network (Code 4) — checked before Config so network endpoint errors are classified accurately
    if msg.contains("horizon")
        || msg.contains("friendbot")
        || msg.contains("soroban rpc")
        || msg.contains("connection refused")
        || msg.contains("connection reset")
        || msg.contains("dns error")
        || msg.contains("timed out")
        || msg.contains("timeout")
        || msg.contains("http error")
        || msg.contains("failed to fetch")
        || msg.contains("network")
            && (msg.contains("failed") || msg.contains("unreachable") || msg.contains("connect"))
    {
        return ExitCode::Network;
    }

    // 4. Configuration (Code 3)
    if msg.contains("config")
        || msg.contains("config.toml")
        || msg.contains("unsupported network")
        || msg.contains("duplicate wallet name")
        || msg.contains("parse_config")
        || msg.contains("schema version")
        || msg.contains("toml")
        || full_chain_msg.contains("config migration")
    {
        return ExitCode::Config;
    }

    // 5. Execution / Contract (Code 6)
    if msg.contains("wasm")
        || msg.contains("contract")
        || msg.contains("simulation")
        || msg.contains("reverted")
        || msg.contains("compilation")
        || msg.contains("build failed")
        || msg.contains("gas limit")
        || msg.contains("tx failed")
        || msg.contains("transaction failed")
        || msg.contains("stellar contract")
    {
        return ExitCode::Execution;
    }

    // 6. Environment / System (Code 7)
    if msg.contains("docker")
        || msg.contains("permission denied")
        || msg.contains("unsupported architecture")
        || msg.contains("unsupported operating system")
        || msg.contains("unsupported os")
        || msg.contains("command not found")
        || msg.contains("no such file or directory") && (msg.contains("usr") || msg.contains("bin"))
    {
        return ExitCode::Environment;
    }

    // Default to General Failure (Code 1)
    ExitCode::GeneralFailure
}
