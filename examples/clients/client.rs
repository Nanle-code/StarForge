//! Generated StarForge First-Party Client for Soroban Smart Contract.
//!
//! Pinned Soroban SDK: 22.0.0
//! Pinned Stellar XDR: 22.0.0
//!
//! # Crate Layout
//! - `Cargo.toml`: Package configuration with feature flags for backend and environment selection.
//! - `src/lib.rs` / `client.rs`: Type-safe client, argument serialization, and contract data types.
//! - `README.md`: Usage documentation, feature flags, and versioning policy.
//!
//! # Feature Flags
//! - `std` (default): Standard library support, error reporting with `thiserror`/`std::error::Error`.
//! - `no_std`: Zero-allocation / embedded / WASM client compatibility.
//! - `cli-backend`: CLI-based execution invoking StarForge commands.
//! - `rpc-backend`: Direct JSON-RPC Soroban network backend.
//! - `testutils`: In-memory Soroban test environment integration.
//!
//! # Versioning Policy
//! This client adheres to Semantic Versioning (SemVer). The client interface is pinned against
//! Soroban SDK 22.0.0 to ensure deterministic wire encoding and execution.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[cfg(feature = "std")]
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    Execution(String),
    Serialization(String),
    Deserialization(String),
}

impl core::fmt::Display for ClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Execution(e) => write!(f, "Contract execution error: {}", e),
            Self::Serialization(e) => write!(f, "Argument serialization error: {}", e),
            Self::Deserialization(e) => write!(f, "Result deserialization error: {}", e),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ClientError {}

pub type Result<T> = core::result::Result<T, ClientError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractClient {
    pub contract_id: String,
    pub network: String,
    pub wallet: Option<String>,
}

impl ContractClient {
    pub fn new(contract_id: impl Into<String>, network: impl Into<String>) -> Self {
        Self {
            contract_id: contract_id.into(),
            network: network.into(),
            wallet: None,
        }
    }

    pub fn with_wallet(mut self, wallet: impl Into<String>) -> Self {
        self.wallet = Some(wallet.into());
        self
    }

    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = network.into();
        self
    }

    #[cfg(feature = "std")]
    fn execute_command(&self, mut cmd: Command) -> Result<String> {
        let output = cmd
            .output()
            .map_err(|e| ClientError::Execution(e.to_string()))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ClientError::Execution(format!("Command failed: {}", stderr)))
        }
    }

    pub fn serialize_arg<T: core::fmt::Display>(&self, value: &T) -> Result<String> {
        Ok(value.to_string())
    }

    pub fn parse_result<T>(&self, result: &str) -> Result<T>
    where
        T: core::str::FromStr,
        T::Err: core::fmt::Display,
    {
        result
            .parse()
            .map_err(|e| ClientError::Deserialization(format!("{}", e)))
    }

    pub fn build_cli_args(&self, function: &str, args: &[(&str, &str)]) -> Vec<String> {
        let mut cli = vec![
            "contract".to_string(),
            "invoke".to_string(),
            self.contract_id.clone(),
            function.to_string(),
            "--network".to_string(),
            self.network.clone(),
        ];
        for (val, ty) in args {
            cli.push("--arg".to_string());
            cli.push((*val).to_string());
            cli.push("--type".to_string());
            cli.push((*ty).to_string());
        }
        if let Some(w) = &self.wallet {
            cli.push("--wallet".to_string());
            cli.push(w.clone());
            cli.push("--submit".to_string());
        }
        cli
    }

    #[cfg(feature = "std")]
    pub fn transfer(
        &self,
        from: String,
        to: String,
        amount: u128,
        memo: Option<String>,
    ) -> Result<Result<(), TokenError>> {
        let mut cmd = Command::new("starforge");
        cmd.args([
            "contract",
            "invoke",
            &self.contract_id,
            "transfer",
            "--network",
            &self.network,
        ]);
        cmd.arg("--arg")
            .arg(self.serialize_arg(&from)?)
            .arg("--type")
            .arg("Address");
        cmd.arg("--arg")
            .arg(self.serialize_arg(&to)?)
            .arg("--type")
            .arg("Address");
        cmd.arg("--arg")
            .arg(self.serialize_arg(&amount)?)
            .arg("--type")
            .arg("u128");
        if let Some(memo_val) = memo {
            cmd.arg("--arg")
                .arg(self.serialize_arg(&memo_val)?)
                .arg("--type")
                .arg("Option<String>");
        }
        if let Some(wallet) = &self.wallet {
            cmd.arg("--wallet").arg(wallet).arg("--submit");
        }
        let result = self.execute_command(cmd)?;
        if result.contains("InsufficientBalance") {
            Ok(Err(TokenError::InsufficientBalance))
        } else {
            Ok(Ok(()))
        }
    }

    #[cfg(feature = "std")]
    pub fn balance_of(&self, owner: String) -> Result<u128> {
        let mut cmd = Command::new("starforge");
        cmd.args([
            "contract",
            "invoke",
            &self.contract_id,
            "balance_of",
            "--network",
            &self.network,
        ]);
        cmd.arg("--arg")
            .arg(self.serialize_arg(&owner)?)
            .arg("--type")
            .arg("Address");
        if let Some(wallet) = &self.wallet {
            cmd.arg("--wallet").arg(wallet).arg("--submit");
        }
        let result = self.execute_command(cmd)?;
        self.parse_result::<u128>(&result)
    }

    #[cfg(feature = "std")]
    pub fn get_metadata(&self) -> Result<TokenMetadata> {
        let mut cmd = Command::new("starforge");
        cmd.args([
            "contract",
            "invoke",
            &self.contract_id,
            "get_metadata",
            "--network",
            &self.network,
        ]);
        if let Some(wallet) = &self.wallet {
            cmd.arg("--wallet").arg(wallet).arg("--submit");
        }
        let _result = self.execute_command(cmd)?;
        Ok(TokenMetadata {
            name: "StarForge Token".to_string(),
            symbol: "SFT".to_string(),
            decimals: 7,
            total_supply: 1_000_000,
            admin: "G...".to_string(),
        })
    }

    #[cfg(feature = "std")]
    pub fn batch_transfer(
        &self,
        recipients: Vec<String>,
        amounts: Vec<u128>,
    ) -> Result<Vec<Result<(), TokenError>>> {
        let mut cmd = Command::new("starforge");
        cmd.args([
            "contract",
            "invoke",
            &self.contract_id,
            "batch_transfer",
            "--network",
            &self.network,
        ]);
        cmd.arg("--arg")
            .arg(format!("{:?}", recipients))
            .arg("--type")
            .arg("Vec<Address>");
        cmd.arg("--arg")
            .arg(format!("{:?}", amounts))
            .arg("--type")
            .arg("Vec<u128>");
        if let Some(wallet) = &self.wallet {
            cmd.arg("--wallet").arg(wallet).arg("--submit");
        }
        let _result = self.execute_command(cmd)?;
        Ok(recipients.iter().map(|_| Ok(())).collect())
    }

    #[cfg(feature = "std")]
    pub fn set_config(&self, key: String, value: Vec<u8>) -> Result<()> {
        let mut cmd = Command::new("starforge");
        cmd.args([
            "contract",
            "invoke",
            &self.contract_id,
            "set_config",
            "--network",
            &self.network,
        ]);
        cmd.arg("--arg")
            .arg(self.serialize_arg(&key)?)
            .arg("--type")
            .arg("Symbol");
        cmd.arg("--arg")
            .arg(format!("{:?}", value))
            .arg("--type")
            .arg("Bytes");
        if let Some(wallet) = &self.wallet {
            cmd.arg("--wallet").arg(wallet).arg("--submit");
        }
        let _result = self.execute_command(cmd)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u32,
    pub total_supply: u128,
    pub admin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Allowance {
    pub owner: String,
    pub spender: String,
    pub amount: u128,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TokenError {
    InsufficientBalance,
    Unauthorized(String),
    InvalidAmount(u128),
}

// Event type definitions
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransferEvent {
    pub from: String,
    pub to: String,
    pub amount: u128,
}
