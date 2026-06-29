use anyhow::{anyhow, Result};
use std::process::Command;

const MIN_CARGO_VERSION: Version = Version {
    major: 1,
    minor: 75,
    patch: 0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

pub trait CommandRunner {
    fn output(&self, program: &str, args: &[&str]) -> Result<String>;
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn output(&self, program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program).args(args).output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!(
                "{} {} failed: {}",
                program,
                args.join(" "),
                stderr.trim()
            ))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ContractPreflight {
    pub cargo_version: String,
    pub wasm_target_installed: bool,
    pub stellar_cli_available: bool,
}

pub fn run_contract_preflight(interactive: bool) -> Result<ContractPreflight> {
    run_contract_preflight_with(&SystemCommandRunner, interactive)
}

pub fn run_contract_preflight_with(
    runner: &impl CommandRunner,
    _interactive: bool,
) -> Result<ContractPreflight> {
    let cargo_version_output = runner.output("cargo", &["--version"]).map_err(|err| {
        anyhow!(
            "Rust Cargo is required before scaffolding a Soroban contract.\n  Install Rust with `rustup` from https://rustup.rs/ and rerun this command.\n  Details: {}",
            err
        )
    })?;

    let cargo_version = parse_cargo_version(&cargo_version_output).ok_or_else(|| {
        anyhow!(
            "Could not parse `cargo --version` output: `{}`",
            cargo_version_output.trim()
        )
    })?;

    if cargo_version < MIN_CARGO_VERSION {
        return Err(anyhow!(
            "Cargo {} is too old for the scaffolded Soroban project.\n  Install or select Rust {}.{}.{} or newer with `rustup update stable`.",
            format_version(cargo_version),
            MIN_CARGO_VERSION.major,
            MIN_CARGO_VERSION.minor,
            MIN_CARGO_VERSION.patch
        ));
    }

    let installed_targets = runner
        .output("rustup", &["target", "list", "--installed"])
        .map_err(|err| {
            anyhow!(
                "`rustup target list --installed` failed.\n  Ensure rustup is installed and add the wasm target with `rustup target add wasm32-unknown-unknown`.\n  Details: {}",
                err
            )
        })?;

    let wasm_target_installed = installed_targets
        .lines()
        .any(|line| line.trim() == "wasm32-unknown-unknown");

    if !wasm_target_installed {
        return Err(anyhow!(
            "Missing Rust target `wasm32-unknown-unknown`.\n  Run `rustup target add wasm32-unknown-unknown` before scaffolding this contract."
        ));
    }

    let stellar_cli_available = runner.output("stellar", &["--version"]).is_ok();

    Ok(ContractPreflight {
        cargo_version: format_version(cargo_version),
        wasm_target_installed,
        stellar_cli_available,
    })
}

fn parse_cargo_version(output: &str) -> Option<Version> {
    let version = output.split_whitespace().nth(1)?;
    let mut parts = version.split('.');
    Some(Version {
        major: parts.next()?.parse().ok()?,
        minor: parts.next()?.parse().ok()?,
        patch: parts
            .next()
            .and_then(|part| {
                part.chars()
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .unwrap_or(0),
    })
}

fn format_version(version: Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::collections::HashMap;

    struct FakeRunner {
        outputs: HashMap<String, Result<String, String>>,
    }

    impl FakeRunner {
        fn new(outputs: &[(&str, Result<&str, &str>)]) -> Self {
            Self {
                outputs: outputs
                    .iter()
                    .map(|(key, value)| {
                        (
                            (*key).to_string(),
                            value
                                .as_ref()
                                .map(|ok| (*ok).to_string())
                                .map_err(|err| (*err).to_string()),
                        )
                    })
                    .collect(),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn output(&self, program: &str, args: &[&str]) -> Result<String> {
            let key = format!("{} {}", program, args.join(" "));
            match self.outputs.get(&key) {
                Some(Ok(output)) => Ok(output.clone()),
                Some(Err(err)) => bail!("{}", err),
                None => bail!("missing fake output for {}", key),
            }
        }
    }

    #[test]
    fn accepts_current_rust_toolchain_and_warns_only_for_missing_stellar() {
        let runner = FakeRunner::new(&[
            ("cargo --version", Ok("cargo 1.81.0 (2dbb1af80 2024-08-20)")),
            (
                "rustup target list --installed",
                Ok("x86_64-pc-windows-msvc\nwasm32-unknown-unknown\n"),
            ),
            ("stellar --version", Err("not installed")),
        ]);

        let result = run_contract_preflight_with(&runner, false).unwrap();

        assert_eq!(result.cargo_version, "1.81.0");
        assert!(result.wasm_target_installed);
        assert!(!result.stellar_cli_available);
    }

    #[test]
    fn fails_when_cargo_is_missing() {
        let runner = FakeRunner::new(&[("cargo --version", Err("not found"))]);

        let err = run_contract_preflight_with(&runner, false).unwrap_err();

        assert!(err.to_string().contains("Rust Cargo is required"));
    }

    #[test]
    fn fails_when_cargo_version_is_too_old() {
        let runner = FakeRunner::new(&[("cargo --version", Ok("cargo 1.70.0"))]);

        let err = run_contract_preflight_with(&runner, false).unwrap_err();

        assert!(err.to_string().contains("too old"));
    }

    #[test]
    fn fails_when_wasm_target_is_missing() {
        let runner = FakeRunner::new(&[
            ("cargo --version", Ok("cargo 1.81.0")),
            (
                "rustup target list --installed",
                Ok("x86_64-unknown-linux-gnu\n"),
            ),
        ]);

        let err = run_contract_preflight_with(&runner, false).unwrap_err();

        assert!(err.to_string().contains("wasm32-unknown-unknown"));
    }
}
