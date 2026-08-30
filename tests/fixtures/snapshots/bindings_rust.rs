use std::process::Command;
use std::io::{self, Write};
use anyhow::{Result, Context};

pub struct ContractClient {
	pub contract_id: String,
	pub network: String,
	pub wallet: Option<String>,
}

impl ContractClient {
	pub fn new(contract_id: impl Into<String>, network: impl Into<String>) -> Self {
		Self { contract_id: contract_id.into(), network: network.into(), wallet: None }
	}

	pub fn with_wallet(mut self, wallet: impl Into<String>) -> Self {
		self.wallet = Some(wallet.into());
		self
	}

	fn execute_command(&self, mut cmd: Command) -> Result<String> {
		let output = cmd.output().context("Failed to execute command")?;
		if output.status.success() {
			Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
		} else {
			let stderr = String::from_utf8_lossy(&output.stderr);
			anyhow::bail!("Command failed: {}", stderr)
		}
	}

	pub fn transfer(&self, from: String, to: String, amount: u128, memo: Option<String>) -> Result<Result<(), Error>> {
		let mut cmd = Command::new("starforge");
		cmd.args(["contract", "invoke", &self.contract_id, "transfer", "--network", &self.network]);
		cmd.arg("--arg").arg(self.serialize_arg(&from)?).arg("--type").arg("Address");
		cmd.arg("--arg").arg(self.serialize_arg(&to)?).arg("--type").arg("Address");
		cmd.arg("--arg").arg(self.serialize_arg(&amount)?).arg("--type").arg("u128");
		cmd.arg("--arg").arg(self.serialize_arg(&memo)?).arg("--type").arg("Option<String>");
		if let Some(wallet) = &self.wallet {
			cmd.arg("--wallet").arg(wallet).arg("--submit");
		}
		let result = self.execute_command(cmd)?;
		// Parse result based on return type
		Ok(self.parse_result::<{return_type}>(&result)?)
	}

	pub fn balance_of(&self, owner: String) -> Result<u128> {
		let mut cmd = Command::new("starforge");
		cmd.args(["contract", "invoke", &self.contract_id, "balance_of", "--network", &self.network]);
		cmd.arg("--arg").arg(self.serialize_arg(&owner)?).arg("--type").arg("Address");
		if let Some(wallet) = &self.wallet {
			cmd.arg("--wallet").arg(wallet).arg("--submit");
		}
		let result = self.execute_command(cmd)?;
		// Parse result based on return type
		Ok(self.parse_result::<{return_type}>(&result)?)
	}

	pub fn get_metadata(&self) -> Result<TokenMetadata> {
		let mut cmd = Command::new("starforge");
		cmd.args(["contract", "invoke", &self.contract_id, "get_metadata", "--network", &self.network]);
		if let Some(wallet) = &self.wallet {
			cmd.arg("--wallet").arg(wallet).arg("--submit");
		}
		let result = self.execute_command(cmd)?;
		// Parse result based on return type
		Ok(self.parse_result::<{return_type}>(&result)?)
	}

	pub fn batch_transfer(&self, recipients: Vec<Address>, amounts: Vec<u128>) -> Result<Vec<Result<(), Error>>> {
		let mut cmd = Command::new("starforge");
		cmd.args(["contract", "invoke", &self.contract_id, "batch_transfer", "--network", &self.network]);
		cmd.arg("--arg").arg(self.serialize_arg(&recipients)?).arg("--type").arg("Vec<Address>");
		cmd.arg("--arg").arg(self.serialize_arg(&amounts)?).arg("--type").arg("Vec<u128>");
		if let Some(wallet) = &self.wallet {
			cmd.arg("--wallet").arg(wallet).arg("--submit");
		}
		let result = self.execute_command(cmd)?;
		// Parse result based on return type
		Ok(self.parse_result::<{return_type}>(&result)?)
	}

	pub fn set_config(&self, key: String, value: Vec<u8>) -> Result<()> {
		let mut cmd = Command::new("starforge");
		cmd.args(["contract", "invoke", &self.contract_id, "set_config", "--network", &self.network]);
		cmd.arg("--arg").arg(self.serialize_arg(&key)?).arg("--type").arg("Symbol");
		cmd.arg("--arg").arg(self.serialize_arg(&value)?).arg("--type").arg("Bytes");
		if let Some(wallet) = &self.wallet {
			cmd.arg("--wallet").arg(wallet).arg("--submit");
		}
		let result = self.execute_command(cmd)?;
		// Parse result based on return type
		Ok(self.parse_result::<{return_type}>(&result)?)
	}

	fn serialize_arg<T: std::fmt::Display>(&self, value: &T) -> Result<String> {
		Ok(value.to_string())
	}

	fn parse_result<T>(&self, result: &str) -> Result<T>
	where T: std::str::FromStr,
	      T::Err: std::error::Error + Send + Sync + 'static,
	{
		result.parse().context("Failed to parse result")
	}

}

pub struct TokenMetadata {
	pub name: String,
	pub symbol: String,
	pub decimals: u32,
	pub total_supply: u128,
	pub admin: String,
}

pub struct Allowance {
	pub owner: String,
	pub spender: String,
	pub amount: u128,
	pub expires_at: Option<u64>,
}

pub enum TokenError {
	InsufficientBalance,
	Unauthorized(String),
	InvalidAmount(u128),
}

// Event type definitions
pub struct TransferEvent {
	pub from: String,
	pub to: String,
	pub amount: u128,
}
