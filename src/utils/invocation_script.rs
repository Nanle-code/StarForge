use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationScript {
    pub version: u32,
    pub steps: Vec<InvocationStep>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationStep {
    pub name: Option<String>,
    pub contract_id: String,
    pub function: String,
    #[serde(default)]
    pub args: Vec<InvocationArgument>,
    pub network: Option<String>,
    pub wallet: Option<String>,
    #[serde(default)]
    pub submit: bool,
    #[serde(default)]
    pub assertions: Vec<InvocationAssertion>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InvocationArgument {
    pub value: String,
    #[serde(rename = "type")]
    pub arg_type: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum InvocationAssertion {
    #[serde(rename = "return_equals")]
    ReturnEquals { value: String },
    #[serde(rename = "return_contains")]
    ReturnContains { value: String },
    #[serde(rename = "error_contains")]
    ErrorContains { value: String },
    #[serde(rename = "event_contains")]
    EventContains { value: String },
    #[serde(rename = "fee_at_most")]
    FeeAtMost { value: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedInvocation {
    pub step: usize,
    pub contract_id: String,
    pub function: String,
    pub args: Vec<InvocationArgument>,
    pub network: String,
    pub wallet: Option<String>,
    pub submit: bool,
    pub assertions: Vec<InvocationAssertion>,
}

#[derive(Debug, Clone, Default)]
pub struct InvocationResult {
    pub return_value: String,
    pub errors: Vec<String>,
    pub events: Vec<String>,
    pub fee: u64,
}

pub fn load(path: &Path) -> Result<InvocationScript> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Unable to read invocation script '{}'.", path.display()))?;
    let script = match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_str(&text)
            .with_context(|| format!("Invalid JSON invocation script '{}'.", path.display()))?,
        Some("yaml") | Some("yml") => serde_yaml::from_str(&text)
            .with_context(|| format!("Invalid YAML invocation script '{}'.", path.display()))?,
        Some(extension) => bail!(
            "Unsupported invocation script extension '.{}'. Use .json, .yaml, or .yml.",
            extension
        ),
        None => bail!(
            "Invocation script '{}' has no file extension.",
            path.display()
        ),
    };
    validate(&script)?;
    Ok(script)
}

pub fn validate(script: &InvocationScript) -> Result<()> {
    if script.version != 1 {
        bail!(
            "Unsupported invocation script version {}. Expected 1.",
            script.version
        );
    }
    if script.steps.is_empty() {
        bail!("Invocation script must contain at least one step.");
    }
    for (index, step) in script.steps.iter().enumerate() {
        if step.contract_id.trim().is_empty() {
            bail!("steps[{}].contract_id must not be empty.", index);
        }
        if step.function.trim().is_empty() {
            bail!("steps[{}].function must not be empty.", index);
        }
        if step.submit && step.wallet.is_none() {
            bail!("steps[{}].wallet is required when submit is true.", index);
        }
        for (arg_index, arg) in step.args.iter().enumerate() {
            if arg.value.is_empty() {
                bail!(
                    "steps[{}].args[{}].value must not be empty.",
                    index,
                    arg_index
                );
            }
            if !matches!(
                arg.arg_type.as_str(),
                "string" | "symbol" | "int" | "bool" | "address"
            ) {
                bail!(
                    "steps[{}].args[{}].type '{}' is invalid. Expected string, symbol, int, bool, or address.",
                    index, arg_index, arg.arg_type
                );
            }
        }
    }
    Ok(())
}

pub fn plan(script: &InvocationScript, default_network: &str) -> Result<Vec<PlannedInvocation>> {
    validate(script)?;
    script
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            Ok(PlannedInvocation {
                step: index + 1,
                contract_id: interpolate(&step.contract_id)?,
                function: interpolate(&step.function)?,
                args: step
                    .args
                    .iter()
                    .map(|arg| {
                        Ok(InvocationArgument {
                            value: interpolate(&arg.value)?,
                            arg_type: arg.arg_type.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                network: interpolate(step.network.as_deref().unwrap_or(default_network))?,
                wallet: step.wallet.as_deref().map(interpolate).transpose()?,
                submit: step.submit,
                assertions: step
                    .assertions
                    .iter()
                    .map(interpolate_assertion)
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect()
}

pub fn interpolate(value: &str) -> Result<String> {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find("${") {
        output.push_str(&remaining[..start]);
        let end = remaining[start + 2..]
            .find('}')
            .ok_or_else(|| anyhow!("Unclosed environment variable in '{}'.", value))?;
        let name = &remaining[start + 2..start + 2 + end];
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            bail!("Invalid environment variable '{}'.", name);
        }
        output.push_str(
            &std::env::var(name)
                .with_context(|| format!("Environment variable '{}' is not set.", name))?,
        );
        remaining = &remaining[start + 3 + end..];
    }
    output.push_str(remaining);
    Ok(output)
}

pub fn assert_result(assertion: &InvocationAssertion, result: &InvocationResult) -> Result<()> {
    let (passed, message) = match assertion {
        InvocationAssertion::ReturnEquals { value } => (
            &result.return_value == value,
            format!("return value equals '{}'", value),
        ),
        InvocationAssertion::ReturnContains { value } => (
            result.return_value.contains(value),
            format!("return value contains '{}'", value),
        ),
        InvocationAssertion::ErrorContains { value } => (
            result.errors.iter().any(|error| error.contains(value)),
            format!("an error contains '{}'", value),
        ),
        InvocationAssertion::EventContains { value } => (
            result.events.iter().any(|event| event.contains(value)),
            format!("an event contains '{}'", value),
        ),
        InvocationAssertion::FeeAtMost { value } => (
            result.fee <= *value,
            format!("fee is at most {} stroops", value),
        ),
    };
    if passed {
        Ok(())
    } else {
        bail!("Assertion failed: {}.", message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_and_preserves_step_order() {
        std::env::set_var("STARFORGE_CONTRACT_ID", "c1");
        std::env::set_var("STARFORGE_VALUE", "value");
        let script = load(std::path::Path::new(
            "tests/fixtures/invocation-script.yaml",
        ))
        .unwrap();
        let planned = plan(&script, "testnet").unwrap();
        assert_eq!(planned[0].function, "set_value");
        assert_eq!(planned[1].function, "get_value");
        assert_eq!(planned[0].args[0].value, "value");
    }

    #[test]
    fn rejects_unknown_fields_precisely() {
        let error = serde_json::from_str::<InvocationScript>(
            r#"{"version":1,"steps":[],"unexpected":true}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown field `unexpected`"));
    }

    #[test]
    fn parses_json_fixture() {
        let script = load(std::path::Path::new(
            "tests/fixtures/invocation-script.json",
        ))
        .unwrap();
        assert_eq!(plan(&script, "testnet").unwrap()[0].function, "ping");
    }

    #[test]
    fn interpolates_environment_values() {
        std::env::set_var("STARFORGE_SCRIPT_TEST", "expanded");
        assert_eq!(
            interpolate("before-${STARFORGE_SCRIPT_TEST}").unwrap(),
            "before-expanded"
        );
    }

    #[test]
    fn evaluates_result_assertions() {
        let result = InvocationResult {
            return_value: "ok-value".into(),
            events: vec!["transfer:done".into()],
            fee: 12,
            ..Default::default()
        };
        assert!(assert_result(
            &InvocationAssertion::ReturnContains { value: "ok".into() },
            &result
        )
        .is_ok());
        assert!(assert_result(&InvocationAssertion::FeeAtMost { value: 10 }, &result).is_err());
    }
}

fn interpolate_assertion(assertion: &InvocationAssertion) -> Result<InvocationAssertion> {
    Ok(match assertion {
        InvocationAssertion::ReturnEquals { value } => InvocationAssertion::ReturnEquals {
            value: interpolate(value)?,
        },
        InvocationAssertion::ReturnContains { value } => InvocationAssertion::ReturnContains {
            value: interpolate(value)?,
        },
        InvocationAssertion::ErrorContains { value } => InvocationAssertion::ErrorContains {
            value: interpolate(value)?,
        },
        InvocationAssertion::EventContains { value } => InvocationAssertion::EventContains {
            value: interpolate(value)?,
        },
        InvocationAssertion::FeeAtMost { value } => {
            InvocationAssertion::FeeAtMost { value: *value }
        }
    })
}
