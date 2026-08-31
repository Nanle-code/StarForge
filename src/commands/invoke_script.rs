use crate::utils::{config, print as p, soroban};
use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct InvokeScriptArgs {
    /// YAML or JSON invocation script
    pub file: PathBuf,
    /// Print resolved calls without simulating or submitting them
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeScript {
    pub version: u32,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub steps: Vec<InvokeStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeStep {
    pub name: String,
    pub contract_id: String,
    pub function: String,
    pub wallet: String,
    pub network: Option<String>,
    #[serde(default)]
    pub args: Vec<InvokeArgument>,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeArgument {
    pub value: String,
    #[serde(rename = "type")]
    pub arg_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    #[serde(default)]
    pub equals: Option<String>,
    #[serde(default)]
    pub contains: Option<String>,
}

pub async fn handle(args: InvokeScriptArgs) -> Result<()> {
    let script = load(&args.file)?;
    validate(&script)?;
    let cfg = config::load()?;
    let network_default = cfg.network;
    p::header(if args.dry_run {
        "Invoke Script (dry run)"
    } else {
        "Invoke Script"
    });

    for (index, step) in script.steps.iter().enumerate() {
        let network = interpolate(
            step.network.as_deref().unwrap_or(&network_default),
            &script.env,
        )
        .with_context(|| format!("steps[{}].network", index))?;
        let contract_id = interpolate(&step.contract_id, &script.env)
            .with_context(|| format!("steps[{}].contract_id", index))?;
        let function = interpolate(&step.function, &script.env)
            .with_context(|| format!("steps[{}].function", index))?;
        let wallet_name = interpolate(&step.wallet, &script.env)
            .with_context(|| format!("steps[{}].wallet", index))?;
        config::validate_contract_id(&contract_id)
            .with_context(|| format!("steps[{}].contract_id", index))?;
        config::validate_network(&network).with_context(|| format!("steps[{}].network", index))?;
        let call_args = step
            .args
            .iter()
            .map(|arg| interpolate(&arg.value, &script.env))
            .collect::<Result<Vec<_>>>()?;
        let types = step
            .args
            .iter()
            .map(|arg| arg.arg_type.clone())
            .collect::<Vec<_>>();
        validate_types(&types).with_context(|| format!("steps[{}].args", index))?;
        println!(
            "[{}] {}::{} on {}",
            step.name, contract_id, function, network
        );
        for (arg_index, (value, arg_type)) in call_args.iter().zip(types.iter()).enumerate() {
            println!("  arg[{}] ({}) = {}", arg_index, arg_type, value);
        }
        if args.dry_run {
            continue;
        }

        let wallet = cfg
            .wallets
            .iter()
            .find(|entry| entry.name == wallet_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "steps[{}].wallet: wallet '{}' not found",
                    index,
                    wallet_name
                )
            })?;
        let outcome = soroban::invoke_contract(
            &contract_id,
            &function,
            &call_args,
            &types,
            &network,
            Some(wallet),
            None,
        )
        .await
        .with_context(|| format!("steps[{}] '{}' failed", index, step.name))?;
        let return_value = outcome
            .transaction
            .as_ref()
            .map(|tx| tx.return_value.as_str())
            .unwrap_or(&outcome.simulation.return_value);
        let assertions = step
            .assertions
            .iter()
            .map(|assertion| {
                Ok(Assertion {
                    equals: assertion
                        .equals
                        .as_deref()
                        .map(|value| interpolate(value, &script.env))
                        .transpose()?,
                    contains: assertion
                        .contains
                        .as_deref()
                        .map(|value| interpolate(value, &script.env))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        assert_result(index, &assertions, return_value)?;
        p::success(&format!("Step '{}' completed", step.name));
    }
    Ok(())
}

fn load(path: &Path) -> Result<InvokeScript> {
    let text =
        fs::read_to_string(path).with_context(|| format!("reading script {}", path.display()))?;
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("json") => serde_json::from_str(&text).context("invalid invocation JSON schema"),
        Some("yaml") | Some("yml") => {
            serde_yaml::from_str(&text).context("invalid invocation YAML schema")
        }
        _ => anyhow::bail!("invocation script must use .json, .yaml, or .yml extension"),
    }
}

fn validate(script: &InvokeScript) -> Result<()> {
    if script.version != 1 {
        anyhow::bail!(
            "version: unsupported schema version {}; expected 1",
            script.version
        );
    }
    if script.steps.is_empty() {
        anyhow::bail!("steps: must contain at least one step");
    }
    for (index, step) in script.steps.iter().enumerate() {
        if step.name.trim().is_empty() {
            anyhow::bail!("steps[{}].name: must not be empty", index);
        }
        if step.function.trim().is_empty() {
            anyhow::bail!("steps[{}].function: must not be empty", index);
        }
        if step.wallet.trim().is_empty() {
            anyhow::bail!("steps[{}].wallet: must not be empty", index);
        }
        for (arg_index, arg) in step.args.iter().enumerate() {
            validate_types(std::slice::from_ref(&arg.arg_type))
                .with_context(|| format!("steps[{}].args[{}].type", index, arg_index))?;
        }
        for (assert_index, assertion) in step.assertions.iter().enumerate() {
            if assertion.equals.is_none() == assertion.contains.is_none() {
                anyhow::bail!(
                    "steps[{}].assertions[{}]: set exactly one of equals or contains",
                    index,
                    assert_index
                );
            }
        }
    }
    Ok(())
}

fn validate_types(types: &[String]) -> Result<()> {
    for arg_type in types {
        if !matches!(
            arg_type.as_str(),
            "string" | "symbol" | "int" | "bool" | "address"
        ) {
            anyhow::bail!("unsupported type '{}'", arg_type);
        }
    }
    Ok(())
}

fn interpolate(value: &str, vars: &BTreeMap<String, String>) -> Result<String> {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let end = rest[start + 2..]
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("unterminated environment variable in '{}'", value))?
            + start
            + 2;
        let key = &rest[start + 2..end];
        let replacement = vars
            .get(key)
            .cloned()
            .or_else(|| std::env::var(key).ok())
            .ok_or_else(|| anyhow::anyhow!("environment variable '{}' is not set", key))?;
        output.push_str(&replacement);
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

fn assert_result(index: usize, assertions: &[Assertion], value: &str) -> Result<()> {
    for (assert_index, assertion) in assertions.iter().enumerate() {
        if let Some(expected) = &assertion.equals {
            if value != expected {
                anyhow::bail!(
                    "steps[{}].assertions[{}]: expected return value '{}', got '{}'",
                    index,
                    assert_index,
                    expected,
                    value
                );
            }
        }
        if let Some(expected) = &assertion.contains {
            if !value.contains(expected) {
                anyhow::bail!(
                    "steps[{}].assertions[{}]: return value does not contain '{}'",
                    index,
                    assert_index,
                    expected
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_fixture_and_interpolates() {
        let script: InvokeScript =
            serde_yaml::from_str(include_str!("../../tests/fixtures/invoke_script.yaml")).unwrap();
        assert_eq!(
            interpolate(&script.steps[0].contract_id, &script.env).unwrap(),
            "C..."
        );
        validate(&script).unwrap();
    }

    #[test]
    fn rejects_unknown_fields_and_bad_assertions() {
        assert!(
            serde_json::from_str::<InvokeScript>(r#"{"version":1,"steps":[],"extra":true}"#)
                .is_err()
        );
        let script: InvokeScript = serde_json::from_str(r#"{"version":1,"steps":[{"name":"x","contract_id":"C...","function":"f","wallet":"w","assertions":[{}]}]}"#).unwrap();
        assert!(validate(&script)
            .unwrap_err()
            .to_string()
            .contains("exactly one"));
    }

    #[test]
    fn assertions_cover_equals_and_contains() {
        let assertions = vec![
            Assertion {
                equals: Some("ok".into()),
                contains: None,
            },
            Assertion {
                equals: None,
                contains: Some("o".into()),
            },
        ];
        assert_result(0, &assertions, "ok").unwrap();
        assert!(assert_result(0, &assertions, "no").is_err());
    }
}
