//! Stable, documented error identifiers for user-facing CLI failures.

use crate::utils::exit_codes::{self, ExitCode};
use anyhow::anyhow;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

const DOCS_BASE: &str = "https://github.com/Nanle-code/StarForge/blob/master/docs/ERRORS.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    GeneralFailure,
    GeneralUsageFailure,
    GeneralConfigFailure,
    GeneralSigningFailure,
    GeneralEnvironmentFailure,
    GeneralBreakingChange,
    GeneralExecutionFailure,
    WalletNotFound,
    WalletSecretMissing,
    WalletSigningFailed,
    WalletOperationFailed,
    NetworkUnavailable,
    NetworkRequestFailed,
    NetworkOperationFailed,
    DeployArtifactInvalid,
    DeploySimulationFailed,
    DeploySubmissionFailed,
    DeployOperationFailed,
    TemplateNotFound,
    TemplateInvalid,
    TemplateOperationFailed,
    PluginUnavailable,
    PluginExecutionFailed,
    PluginOperationFailed,
}

impl ErrorCode {
    pub const ALL: [Self; 24] = [
        Self::GeneralFailure, Self::GeneralUsageFailure, Self::GeneralConfigFailure,
        Self::GeneralSigningFailure, Self::GeneralEnvironmentFailure, Self::GeneralBreakingChange,
        Self::GeneralExecutionFailure, Self::WalletNotFound, Self::WalletSecretMissing,
        Self::WalletSigningFailed, Self::WalletOperationFailed, Self::NetworkUnavailable,
        Self::NetworkRequestFailed, Self::NetworkOperationFailed, Self::DeployArtifactInvalid,
        Self::DeploySimulationFailed, Self::DeploySubmissionFailed, Self::DeployOperationFailed,
        Self::TemplateNotFound, Self::TemplateInvalid, Self::TemplateOperationFailed,
        Self::PluginUnavailable, Self::PluginExecutionFailed, Self::PluginOperationFailed,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::GeneralFailure => "SF0001",
            Self::GeneralUsageFailure => "SF0002",
            Self::GeneralConfigFailure => "SF0003",
            Self::GeneralSigningFailure => "SF0004",
            Self::GeneralEnvironmentFailure => "SF0005",
            Self::GeneralBreakingChange => "SF0006",
            Self::GeneralExecutionFailure => "SF0007",
            Self::WalletNotFound => "SF1001",
            Self::WalletSecretMissing => "SF1002",
            Self::WalletSigningFailed => "SF1003",
            Self::WalletOperationFailed => "SF1099",
            Self::NetworkUnavailable => "SF1101",
            Self::NetworkRequestFailed => "SF1102",
            Self::NetworkOperationFailed => "SF1199",
            Self::DeployArtifactInvalid => "SF1201",
            Self::DeploySimulationFailed => "SF1202",
            Self::DeploySubmissionFailed => "SF1203",
            Self::DeployOperationFailed => "SF1299",
            Self::TemplateNotFound => "SF1301",
            Self::TemplateInvalid => "SF1302",
            Self::TemplateOperationFailed => "SF1399",
            Self::PluginUnavailable => "SF1401",
            Self::PluginExecutionFailed => "SF1402",
            Self::PluginOperationFailed => "SF1499",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::GeneralFailure => "GeneralFailure",
            Self::GeneralUsageFailure => "GeneralUsageFailure",
            Self::GeneralConfigFailure => "GeneralConfigFailure",
            Self::GeneralSigningFailure => "GeneralSigningFailure",
            Self::GeneralEnvironmentFailure => "GeneralEnvironmentFailure",
            Self::GeneralBreakingChange => "GeneralBreakingChange",
            Self::GeneralExecutionFailure => "GeneralExecutionFailure",
            Self::WalletNotFound => "WalletNotFound",
            Self::WalletSecretMissing => "WalletSecretMissing",
            Self::WalletSigningFailed => "WalletSigningFailed",
            Self::WalletOperationFailed => "WalletOperationFailed",
            Self::NetworkUnavailable => "NetworkUnavailable",
            Self::NetworkRequestFailed => "NetworkRequestFailed",
            Self::NetworkOperationFailed => "NetworkOperationFailed",
            Self::DeployArtifactInvalid => "DeployArtifactInvalid",
            Self::DeploySimulationFailed => "DeploySimulationFailed",
            Self::DeploySubmissionFailed => "DeploySubmissionFailed",
            Self::DeployOperationFailed => "DeployOperationFailed",
            Self::TemplateNotFound => "TemplateNotFound",
            Self::TemplateInvalid => "TemplateInvalid",
            Self::TemplateOperationFailed => "TemplateOperationFailed",
            Self::PluginUnavailable => "PluginUnavailable",
            Self::PluginExecutionFailed => "PluginExecutionFailed",
            Self::PluginOperationFailed => "PluginOperationFailed",
        }
    }

    pub fn domain(self) -> &'static str {
        match self {
            Self::GeneralFailure | Self::GeneralUsageFailure | Self::GeneralConfigFailure
            | Self::GeneralSigningFailure | Self::GeneralEnvironmentFailure
            | Self::GeneralBreakingChange | Self::GeneralExecutionFailure => "general",
            Self::WalletNotFound | Self::WalletSecretMissing | Self::WalletSigningFailed
            | Self::WalletOperationFailed => "wallet",
            Self::NetworkUnavailable | Self::NetworkRequestFailed | Self::NetworkOperationFailed => "network",
            Self::DeployArtifactInvalid | Self::DeploySimulationFailed | Self::DeploySubmissionFailed
            | Self::DeployOperationFailed => "deploy",
            Self::TemplateNotFound | Self::TemplateInvalid | Self::TemplateOperationFailed => "template",
            Self::PluginUnavailable | Self::PluginExecutionFailed | Self::PluginOperationFailed => "plugin",
        }
    }

    pub fn cause(self) -> &'static str {
        match self {
            Self::GeneralFailure => "The command failed for an unclassified reason.",
            Self::GeneralUsageFailure => "The command arguments or input values are invalid.",
            Self::GeneralConfigFailure => "The local StarForge configuration is missing or invalid.",
            Self::GeneralSigningFailure => "A signing key, secret, or cryptographic operation failed.",
            Self::GeneralEnvironmentFailure => "A required system dependency or permission is unavailable.",
            Self::GeneralBreakingChange => "The contract interface contains an unacknowledged breaking change.",
            Self::GeneralExecutionFailure => "Contract execution, compilation, or verification failed.",
            Self::WalletNotFound => "The requested wallet is not present in the local configuration.",
            Self::WalletSecretMissing => "The wallet has no locally stored signing secret.",
            Self::WalletSigningFailed => "The wallet secret could not be unlocked or used to sign.",
            Self::WalletOperationFailed => "The wallet command could not complete its requested operation.",
            Self::NetworkUnavailable => "The configured Stellar network endpoint could not be reached.",
            Self::NetworkRequestFailed => "The network endpoint returned an unsuccessful or invalid response.",
            Self::NetworkOperationFailed => "The network command could not complete its requested operation.",
            Self::DeployArtifactInvalid => "The deployment WASM artifact is missing or invalid.",
            Self::DeploySimulationFailed => "Soroban RPC could not simulate the deployment transaction.",
            Self::DeploySubmissionFailed => "The deployment transaction was rejected or failed during submission.",
            Self::DeployOperationFailed => "The deployment command could not complete its requested operation.",
            Self::TemplateNotFound => "The requested template is not available in the selected source.",
            Self::TemplateInvalid => "The template does not satisfy the required schema or validation rules.",
            Self::TemplateOperationFailed => "The template command could not complete its requested operation.",
            Self::PluginUnavailable => "The requested plugin could not be found or loaded.",
            Self::PluginExecutionFailed => "The plugin returned an error while executing the command.",
            Self::PluginOperationFailed => "The plugin command could not complete its requested operation.",
        }
    }

    pub fn fix(self) -> &'static str {
        match self {
            Self::GeneralFailure => "Retry with --json and include the full error and correlation ID in a support report.",
            Self::GeneralUsageFailure => "Check the command's `--help` output and correct the arguments or input values.",
            Self::GeneralConfigFailure => "Run `starforge info` and correct the configuration file or selected settings.",
            Self::GeneralSigningFailure => "Verify the configured key and passphrase, then retry the signing operation.",
            Self::GeneralEnvironmentFailure => "Install the required dependency or correct filesystem permissions, then retry.",
            Self::GeneralBreakingChange => "Review the interface diff and pass `--acknowledge` only after approving the change.",
            Self::GeneralExecutionFailure => "Review the build or execution diagnostics, then correct the contract and retry.",
            Self::WalletNotFound => "Run `starforge wallet list` and use an existing wallet name, or create it with `starforge wallet create <name>`. ",
            Self::WalletSecretMissing => "Import or create a wallet with a signing secret, then retry the command.",
            Self::WalletSigningFailed => "Verify the wallet passphrase and signing key, then retry with the intended network.",
            Self::WalletOperationFailed => "Run `starforge wallet --help` and verify the wallet name and required options.",
            Self::NetworkUnavailable => "Check the endpoint URL and connectivity, then retry with `starforge network test`. ",
            Self::NetworkRequestFailed => "Run `starforge network test` and retry after the endpoint returns a valid response.",
            Self::NetworkOperationFailed => "Run `starforge network show` to verify the active network and endpoint settings.",
            Self::DeployArtifactInvalid => "Build the contract and pass an existing valid `.wasm` file with `--wasm`.",
            Self::DeploySimulationFailed => "Check Soroban RPC connectivity and simulation diagnostics, then retry the deployment.",
            Self::DeploySubmissionFailed => "Review the transaction response, wallet balance, and network before retrying.",
            Self::DeployOperationFailed => "Run `starforge deploy --help` and verify the artifact, wallet, and network options.",
            Self::TemplateNotFound => "Check the template name and source with `starforge template list` or `starforge registry search`.",
            Self::TemplateInvalid => "Validate the template manifest against the documented schema and correct reported fields.",
            Self::TemplateOperationFailed => "Run `starforge template --help` and verify the template name and requested operation.",
            Self::PluginUnavailable => "Install or enable the plugin, then verify it with `starforge plugin list`.",
            Self::PluginExecutionFailed => "Check the plugin's diagnostic output and compatibility, then retry the command.",
            Self::PluginOperationFailed => "Run `starforge plugin --help` and verify the plugin name and command arguments.",
        }
    }

    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::GeneralFailure => ExitCode::GeneralFailure,
            Self::GeneralUsageFailure => ExitCode::Usage,
            Self::GeneralConfigFailure => ExitCode::Config,
            Self::GeneralSigningFailure => ExitCode::Signing,
            Self::GeneralEnvironmentFailure => ExitCode::Environment,
            Self::GeneralBreakingChange => ExitCode::BreakingChange,
            Self::GeneralExecutionFailure => ExitCode::Execution,
            Self::WalletNotFound | Self::WalletOperationFailed => ExitCode::Config,
            Self::WalletSecretMissing | Self::WalletSigningFailed => ExitCode::Signing,
            Self::NetworkUnavailable | Self::NetworkRequestFailed | Self::NetworkOperationFailed => ExitCode::Network,
            Self::DeployArtifactInvalid | Self::DeploySimulationFailed | Self::DeploySubmissionFailed
            | Self::DeployOperationFailed => ExitCode::Execution,
            Self::TemplateNotFound | Self::TemplateOperationFailed => ExitCode::Config,
            Self::TemplateInvalid => ExitCode::Usage,
            Self::PluginUnavailable | Self::PluginExecutionFailed | Self::PluginOperationFailed => ExitCode::Environment,
        }
    }

    pub fn docs_url(self) -> String {
        format!("{DOCS_BASE}#{}", self.id().to_ascii_lowercase())
    }

    pub fn parse_id(value: &str) -> Option<Self> {
        Self::from_str(value).ok()
    }

    fn from_exit_code(code: ExitCode) -> Self {
        match code {
            ExitCode::Success | ExitCode::GeneralFailure => Self::GeneralFailure,
            ExitCode::Usage => Self::GeneralUsageFailure,
            ExitCode::Config => Self::GeneralConfigFailure,
            ExitCode::Network => Self::NetworkOperationFailed,
            ExitCode::Signing => Self::GeneralSigningFailure,
            ExitCode::Execution => Self::GeneralExecutionFailure,
            ExitCode::Environment => Self::GeneralEnvironmentFailure,
            ExitCode::BreakingChange => Self::GeneralBreakingChange,
        }
    }

    /// Classify legacy anyhow errors while honoring explicitly coded errors first.
    pub fn classify(command: &str, err: &anyhow::Error) -> Self {
        if let Some(coded) = err.chain().find_map(|cause| cause.downcast_ref::<CodedError>()) {
            return coded.code;
        }
        let message = err.to_string().to_ascii_lowercase();
        if message.contains("wallet") && (message.contains("not found") || message.contains("no secret")) {
            return if message.contains("secret") { Self::WalletSecretMissing } else { Self::WalletNotFound };
        }
        if message.contains("timed out") || message.contains("timeout")
            || message.contains("connection refused") || message.contains("unreachable")
            || message.contains("dns error")
        {
            return Self::NetworkUnavailable;
        }
        match command {
            "wallet" => {
                if message.contains("not found") || message.contains("no wallet") { Self::WalletNotFound }
                else if message.contains("no secret") || message.contains("secret key") { Self::WalletSecretMissing }
                else if message.contains("passphrase") || message.contains("decrypt") || message.contains("sign") { Self::WalletSigningFailed }
            else { Self::from_exit_code(exit_codes::determine_exit_code(err)) }
            }
            "network" => {
                if message.contains("timed out") || message.contains("timeout") || message.contains("connection refused")
                    || message.contains("unreachable") || message.contains("dns") { Self::NetworkUnavailable }
                else if message.contains("horizon") || message.contains("soroban rpc") || message.contains("friendbot")
                    || message.contains("http") { Self::NetworkRequestFailed }
                else { Self::from_exit_code(exit_codes::determine_exit_code(err)) }
            }
            "deploy" | "deployments" => {
                if message.contains("simulation") { Self::DeploySimulationFailed }
                else if message.contains("submit") || message.contains("transaction failed") || message.contains("deploy failed") { Self::DeploySubmissionFailed }
                else if message.contains("wasm") || message.contains("artifact") || message.contains("no such file") { Self::DeployArtifactInvalid }
                else if exit_codes::determine_exit_code(err) == ExitCode::Execution { Self::DeployOperationFailed }
                else { Self::from_exit_code(exit_codes::determine_exit_code(err)) }
            }
            "template" | "template-vcs" => {
                if message.contains("not found") { Self::TemplateNotFound }
                else if message.contains("invalid") || message.contains("schema") { Self::TemplateInvalid }
                else { Self::from_exit_code(exit_codes::determine_exit_code(err)) }
            }
            "plugin" | "external" => {
                if message.contains("not found") || message.contains("load") { Self::PluginUnavailable }
                else if message.contains("execute") || message.contains("failed") { Self::PluginExecutionFailed }
                else { Self::from_exit_code(exit_codes::determine_exit_code(err)) }
            }
            _ => Self::from_exit_code(exit_codes::determine_exit_code(err)),
        }
    }

    pub fn render_catalog_markdown() -> String {
        let mut markdown = String::from(
            "# StarForge Error Catalog\n\nStable error codes, causes, fixes, and process exit statuses emitted by the CLI.\n\n",
        );
        for code in Self::ALL {
            markdown.push_str(&format!(
                "### {}: {}\n\n- Domain: {}\n- Exit: {} ({})\n- Cause: {}\n- Fix: {}\n\n",
                code.id(), code.name(), code.domain(), code.exit_code().code(),
                code.exit_code().name(), code.cause(), code.fix().trim()
            ));
        }
        markdown
    }
}

impl FromStr for ErrorCode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|code| code.id().eq_ignore_ascii_case(value.trim())).ok_or(())
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.id()) }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct CodedError {
    pub code: ErrorCode,
    message: String,
}

pub fn coded(code: ErrorCode, message: impl Into<String>) -> anyhow::Error {
    anyhow!(CodedError { code, message: message.into() })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorExplanation {
    pub code: String,
    pub cause: &'static str,
    pub fix: &'static str,
    pub docs: String,
    pub exit_code: i32,
    pub exit_name: &'static str,
}

impl From<ErrorCode> for ErrorExplanation {
    fn from(code: ErrorCode) -> Self {
        let exit_code = code.exit_code();
        Self {
            code: code.id().to_string(), cause: code.cause(), fix: code.fix().trim(),
            docs: code.docs_url(), exit_code: exit_code.code(), exit_name: exit_code.name(),
        }
    }
}

pub fn explain(code: &str) -> anyhow::Result<ErrorExplanation> {
    let code = ErrorCode::parse_id(code)
        .ok_or_else(|| anyhow!("Unknown error code '{code}'. See `starforge explain-error --help`."))?;
    Ok(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_catalog_docs_match_generated_catalog() {
        assert_eq!(include_str!("../../docs/ERRORS.md"), ErrorCode::render_catalog_markdown());
    }

    #[test]
    fn known_codes_parse_and_explain_with_exit_mapping() {
        let explanation = explain("sf1203").unwrap();
        assert_eq!(explanation.code, "SF1203");
        assert_eq!(explanation.exit_code, ExitCode::Execution.code());
        assert!(explanation.docs.ends_with("#sf1203"));
        assert!(ErrorCode::parse_id("SF9999").is_none());
    }

    #[test]
    fn command_failures_receive_domain_codes() {
        assert_eq!(ErrorCode::classify("wallet", &anyhow!("Wallet 'alice' not found")), ErrorCode::WalletNotFound);
        assert_eq!(ErrorCode::classify("network", &anyhow!("Horizon request timed out")), ErrorCode::NetworkUnavailable);
        assert_eq!(ErrorCode::classify("deploy", &anyhow!("Deployment transaction failed")), ErrorCode::DeploySubmissionFailed);
    }

    #[test]
    fn legacy_exit_classes_keep_their_numeric_status() {
        for (error, expected) in [
            (anyhow!("Invalid argument '--bad'"), ExitCode::Usage),
            (anyhow!("Failed to parse config.toml"), ExitCode::Config),
            (anyhow!("Breaking interface change detected"), ExitCode::BreakingChange),
        ] {
            assert_eq!(ErrorCode::classify("other", &error).exit_code(), expected);
        }
    }
}