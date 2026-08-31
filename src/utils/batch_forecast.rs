//! Aggregate fee forecasting for planned batches of contract invokes.
//!
//! A batch of Soroban invokes submitted in quick succession can run the sender
//! out of XLM partway through, stranding the calls after the first few. This
//! module prices a *batch manifest* of invoke intents **before** any of them
//! are submitted so the full cost is known up front.
//!
//! Every intent is priced by a live `simulateTransaction` RPC call when the
//! network is reachable — that is the only authoritative source of Soroban fees.
//! When simulation is unavailable (contract not deployed, RPC down, no network
//! configured) the call falls back to a deterministic local heuristic and is
//! flagged as **high variance** so the user knows the number is an estimate,
//! not a measurement.
//!
//! The aggregate is a [`BatchForecast`]: per-item estimates plus totals, the
//! fraction of the batch that was simulated vs. heuristically estimated, and an
//! optional budget cross-check that surfaces mid-batch insolvency risk before
//! submission.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::utils::config;
use crate::utils::simulation_resources as sr;
use crate::utils::soroban;

/// Schema version this module understands. Bump when the manifest format
/// changes incompatibly.
pub const MANIFEST_VERSION: u32 = 1;

/// A call whose fee is more than this multiple away from the batch median is
/// flagged as a variance outlier.
pub const OUTLIER_FACTOR: u64 = 3;

// ── Heuristic fallback constants (stroops) ──────────────────────────────────
// Used only when `simulateTransaction` is unavailable. Conservative on purpose:
// a heuristic that is too low hides insolvency risk, the exact failure this
// module exists to catch.

/// Base dispatch fee for any invoke.
pub const HEURISTIC_BASE_FEE_STROOPS: u64 = 100_000;
/// Flat fee per argument.
pub const HEURISTIC_FEE_PER_ARG: u64 = 1_000;
/// Per-byte fee for argument values.
pub const HEURISTIC_FEE_PER_ARG_BYTE: u64 = 25;
/// Per-character fee for the function name (longer names cost more to encode).
pub const HEURISTIC_FEE_PER_FUNCTION_CHAR: u64 = 500;

/// Root document for a batch manifest of invoke intents. JSON or YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeBatchManifest {
    /// Manifest schema version. Must be [`MANIFEST_VERSION`].
    pub version: u32,
    /// Default network for invokes that do not specify their own.
    pub network: Option<String>,
    /// Optional cap on the aggregate batch cost, in XLM. When the forecast
    /// exceeds it, `would_exceed_budget` is set (and `--enforce` fails).
    pub budget_xlm: Option<f64>,
    /// The invokes to price, in submission order.
    pub invokes: Vec<InvokeIntent>,
}

/// A single invoke intent in a batch manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeIntent {
    /// Optional human-readable label, e.g. "mint 100 to alice".
    pub name: Option<String>,
    /// Contract to call (Stellar contract strkey, starts with `C`).
    pub contract_id: String,
    /// Function to invoke.
    pub function: String,
    /// Positional arguments as `{ value, type }` pairs.
    #[serde(default)]
    pub args: Vec<ManifestArgument>,
    /// Per-invoke network override; falls back to manifest / CLI default.
    pub network: Option<String>,
    /// Optional per-call fee cap in stroops. A forecast above it flags the
    /// call (`would_exceed_cap`) and `--enforce` fails the batch.
    pub max_fee_stroops: Option<u64>,
}

/// A positional function argument: raw string value plus its Soroban type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestArgument {
    /// Raw string value, as passed to the CLI (`${VAR}` env interpolation
    /// supported).
    pub value: String,
    /// Soroban type: `string`, `symbol`, `int`, `bool`, or `address`.
    #[serde(rename = "type")]
    pub arg_type: String,
}

/// A validated intent with all fields resolved and interpolated.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedInvoke {
    /// Zero-based position in the manifest.
    pub index: usize,
    /// Resolved label (falls back to `#{index+1}`).
    pub name: String,
    pub contract_id: String,
    pub function: String,
    pub args: Vec<ManifestArgument>,
    pub network: String,
    pub max_fee_stroops: Option<u64>,
}

/// Where a per-call fee estimate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EstimateSource {
    /// Fee came from a live `simulateTransaction` RPC response.
    Simulated,
    /// Simulation was unavailable; the fee is a local heuristic.
    Heuristic,
}

/// Per-item cost estimate for one invoke in a batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeFeeEstimate {
    /// Zero-based position in the manifest.
    pub index: usize,
    pub name: String,
    pub contract_id: String,
    pub function: String,
    pub network: String,
    /// Total estimated fee in stroops (simulated recommended fee, or heuristic).
    pub fee_stroops: u64,
    /// `fee_stroops` expressed in XLM.
    pub fee_xlm: f64,
    pub source: EstimateSource,
    /// Whether this call's fee is uncertain or an outlier relative to the
    /// batch. Marked when it was not simulated, exceeds its per-call cap, or
    /// is far from the batch median.
    pub high_variance: bool,
    /// Human-readable reasons behind `high_variance`.
    pub variance_reasons: Vec<String>,
    /// Non-fatal notes surfaced by simulation or aggregation.
    pub warnings: Vec<String>,
    /// Errors that forced a heuristic fallback (e.g. RPC failure).
    pub errors: Vec<String>,
    /// The manifest's per-call cap, if any.
    pub max_fee_stroops: Option<u64>,
    /// True when `fee_stroops` exceeds `max_fee_stroops`.
    pub would_exceed_cap: bool,
}

impl InvokeFeeEstimate {
    /// Display label for tables: the resolved name.
    pub fn label(&self) -> &str {
        &self.name
    }
}

/// Aggregate cost forecast for an entire batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchForecast {
    /// Network the forecast was produced for (the first intent's network).
    pub network: String,
    /// The manifest's aggregate budget cap, if any.
    pub budget_xlm: Option<f64>,
    /// Sum of all per-item fees, in stroops.
    pub total_fee_stroops: u64,
    pub total_fee_xlm: f64,
    pub avg_fee_stroops: u64,
    pub median_fee_stroops: u64,
    pub min_fee_stroops: u64,
    pub max_fee_stroops: u64,
    pub simulated_count: usize,
    pub heuristic_count: usize,
    pub high_variance_count: usize,
    /// True when `total_fee_xlm` exceeds `budget_xlm`.
    pub would_exceed_budget: bool,
    /// Per-item estimates, in submission order.
    pub items: Vec<InvokeFeeEstimate>,
    /// Batch-level notes (e.g. how many calls were not simulated).
    pub warnings: Vec<String>,
}

impl BatchForecast {
    /// Display string for the total, in XLM.
    pub fn total_fee_xlm_display(&self) -> String {
        format!("{:.7} XLM", self.total_fee_xlm)
    }
}

// ── Loading / validation ─────────────────────────────────────────────────────

/// Load and validate a batch manifest from a JSON or YAML file.
pub fn load_manifest(path: &Path) -> Result<InvokeBatchManifest> {
    config::validate_file_path(path, None)?;
    let text = fs::read_to_string(path)
        .with_context(|| format!("Unable to read batch manifest '{}'.", path.display()))?;
    let manifest = match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(&text)
            .with_context(|| format!("Invalid JSON batch manifest '{}'.", path.display()))?,
        Some("yaml") | Some("yml") => serde_yaml::from_str(&text)
            .with_context(|| format!("Invalid YAML batch manifest '{}'.", path.display()))?,
        Some(ext) => bail!(
            "Unsupported batch manifest extension '.{}'. Use .json, .yaml, or .yml.",
            ext
        ),
        None => bail!("Batch manifest '{}' has no file extension.", path.display()),
    };
    validate(&manifest)?;
    Ok(manifest)
}

/// Validate a manifest's structure. Communal checks only — env interpolation
/// and network resolution happen in [`plan`].
pub fn validate(manifest: &InvokeBatchManifest) -> Result<()> {
    if manifest.version != MANIFEST_VERSION {
        bail!(
            "Unsupported batch manifest version {}. Expected {}.",
            manifest.version,
            MANIFEST_VERSION
        );
    }
    if manifest.invokes.is_empty() {
        bail!("Batch manifest must contain at least one invoke.");
    }
    if manifest
        .budget_xlm
        .is_some_and(|budget| !budget.is_finite() || budget < 0.0)
    {
        bail!("budget_xlm must be a non-negative finite number.");
    }
    for (index, invoke) in manifest.invokes.iter().enumerate() {
        if invoke.contract_id.trim().is_empty() {
            bail!("invokes[{}].contract_id must not be empty.", index);
        }
        if invoke.function.trim().is_empty() {
            bail!("invokes[{}].function must not be empty.", index);
        }
        if invoke.max_fee_stroops.is_some_and(|max| max == 0) {
            bail!(
                "invokes[{}].max_fee_stroops must be positive when set.",
                index
            );
        }
        for (arg_index, arg) in invoke.args.iter().enumerate() {
            if arg.value.is_empty() {
                bail!(
                    "invokes[{}].args[{}].value must not be empty.",
                    index,
                    arg_index
                );
            }
            if !matches!(
                arg.arg_type.as_str(),
                "string" | "symbol" | "int" | "bool" | "address"
            ) {
                bail!(
                    "invokes[{}].args[{}].type '{}' is invalid. Expected string, symbol, int, bool, or address.",
                    index, arg_index, arg.arg_type
                );
            }
        }
    }
    Ok(())
}

/// Resolve every invoke in the manifest into a concrete [`PlannedInvoke`]:
/// applies per-invoke / manifest / CLI-default network precedence and expands
/// `${VAR}` environment references in names, contract IDs, functions, args,
/// and networks.
pub fn plan(manifest: &InvokeBatchManifest, default_network: &str) -> Result<Vec<PlannedInvoke>> {
    validate(manifest)?;
    manifest
        .invokes
        .iter()
        .enumerate()
        .map(|(index, invoke)| {
            Ok(PlannedInvoke {
                index,
                name: match invoke.name.as_deref() {
                    Some(name) => interpolate(name)?,
                    None => format!("#{}", index + 1),
                },
                contract_id: interpolate(&invoke.contract_id)?,
                function: interpolate(&invoke.function)?,
                args: invoke
                    .args
                    .iter()
                    .map(|arg| {
                        Ok(ManifestArgument {
                            value: interpolate(&arg.value)?,
                            arg_type: arg.arg_type.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                network: interpolate(
                    invoke
                        .network
                        .as_deref()
                        .or(manifest.network.as_deref())
                        .unwrap_or(default_network),
                )?,
                max_fee_stroops: invoke.max_fee_stroops,
            })
        })
        .collect()
}

/// Expand `${VAR}` references in `value` from the process environment.
///
/// Identical semantics to the helper used by invocation scripts: any valid
/// `[A-Za-z0-9_]` name inside a `${...}` is replaced, and a missing variable
/// is an error rather than a silent empty string.
fn interpolate(value: &str) -> Result<String> {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find("${") {
        output.push_str(&remaining[..start]);
        let end = remaining[start + 2..]
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("Unclosed environment variable in '{}'.", value))?;
        let name = &remaining[start + 2..start + 2 + end];
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            anyhow::bail!("Invalid environment variable '{}'.", name);
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

// ── Heuristic estimation ─────────────────────────────────────────────────────

/// Deterministic, conservative, local-only fee estimate for one planned invoke.
///
/// Used strictly as a fallback when live simulation is unavailable. Scaled by
/// argument count, argument byte size, and function-name length so that
/// bigger calls land on bigger numbers. Never performs I/O.
pub fn estimate_heuristic_fee(planned: &PlannedInvoke) -> u64 {
    let arg_count = planned.args.len() as u64;
    let arg_bytes: u64 = planned.args.iter().map(|arg| arg.value.len() as u64).sum();
    let function_chars = planned.function.chars().count() as u64;
    HEURISTIC_BASE_FEE_STROOPS
        + arg_count * HEURISTIC_FEE_PER_ARG
        + arg_bytes * HEURISTIC_FEE_PER_ARG_BYTE
        + function_chars * HEURISTIC_FEE_PER_FUNCTION_CHAR
}

// ── Per-item evaluation ──────────────────────────────────────────────────────

/// Turn a planned invoke plus the outcome of trying to simulate it into a
/// per-item estimate.
///
/// Pure function so the simulation-success and simulation-failure paths are
/// unit-testable without touching the network. When `simulated_fee_stroops` is
/// `Some` and positive, the call is priced from simulation; otherwise it falls
/// back to the heuristic and is flagged as high variance.
pub fn evaluate_item(
    planned: &PlannedInvoke,
    simulated_fee_stroops: Option<u64>,
    sim_warnings: Vec<String>,
    sim_error: Option<String>,
) -> InvokeFeeEstimate {
    let mut warnings = sim_warnings;
    let mut variance_reasons: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let (fee_stroops, source) = match simulated_fee_stroops {
        Some(fee) if fee > 0 => (fee, EstimateSource::Simulated),
        Some(other) => {
            // A zero/absent fee from a "successful" simulation is suspicious;
            // trust that zero but say so, and still treat it as measured.
            if other == 0 {
                warnings.push(
                    "simulation reported a zero resource fee; treating the call as free \
                     (verify with `starforge simulate resources`)"
                        .to_string(),
                );
            }
            (other, EstimateSource::Simulated)
        }
        None => {
            variance_reasons.push("simulation unavailable; fee is a local heuristic".to_string());
            (estimate_heuristic_fee(planned), EstimateSource::Heuristic)
        }
    };

    if let Some(error) = sim_error {
        errors.push(error);
    }

    let would_exceed_cap = planned.max_fee_stroops.is_some_and(|cap| fee_stroops > cap);
    if would_exceed_cap {
        variance_reasons.push(format!(
            "fee exceeds the {} stroop per-call cap",
            planned.max_fee_stroops.unwrap_or(0)
        ));
    }

    let high_variance = !variance_reasons.is_empty();

    InvokeFeeEstimate {
        index: planned.index,
        name: planned.name.clone(),
        contract_id: planned.contract_id.clone(),
        function: planned.function.clone(),
        network: planned.network.clone(),
        fee_stroops,
        fee_xlm: fee_stroops as f64 / sr::STROOPS_PER_XLM,
        source,
        high_variance,
        variance_reasons,
        warnings,
        errors,
        max_fee_stroops: planned.max_fee_stroops,
        would_exceed_cap,
    }
}

// ── Aggregation ──────────────────────────────────────────────────────────────

/// Aggregate per-item estimates into a batch forecast: totals, min/max/median,
/// simulated/heuristic split, budget cross-check, and variance-outlier
/// highlighting.
///
/// Pure function. An empty `items` slice yields a zeroed forecast rather than
/// a panic, so callers can decide how to report an empty batch.
pub fn aggregate_forecast(
    network: &str,
    budget_xlm: Option<f64>,
    mut items: Vec<InvokeFeeEstimate>,
) -> BatchForecast {
    // Flag calls whose fee sits far from the rest of the batch. A single
    // spiky call is often the one that unexpectedly drains the balance.
    let median = median_stroops(&items);
    if items.len() >= 2 && median > 0 {
        for item in items.iter_mut() {
            let is_outlier = item.fee_stroops > median.saturating_mul(OUTLIER_FACTOR)
                || item.fee_stroops < median / OUTLIER_FACTOR;
            if is_outlier
                && !item
                    .variance_reasons
                    .iter()
                    .any(|reason| reason.contains("median"))
            {
                item.variance_reasons.push(format!(
                    "fee is more than {}x away from the batch median ({} stroops)",
                    OUTLIER_FACTOR, median
                ));
                item.high_variance = true;
            }
        }
    }

    let count = items.len();
    let total_fee_stroops: u64 = items.iter().map(|item| item.fee_stroops).sum();
    let simulated_count = items
        .iter()
        .filter(|item| item.source == EstimateSource::Simulated)
        .count();
    let heuristic_count = count - simulated_count;
    let high_variance_count = items.iter().filter(|item| item.high_variance).count();

    let avg_fee_stroops = if count > 0 {
        total_fee_stroops / count as u64
    } else {
        0
    };
    let min_fee_stroops = items.iter().map(|item| item.fee_stroops).min().unwrap_or(0);
    let max_fee_stroops = items.iter().map(|item| item.fee_stroops).max().unwrap_or(0);

    let total_fee_xlm = total_fee_stroops as f64 / sr::STROOPS_PER_XLM;
    let would_exceed_budget = budget_xlm.is_some_and(|budget| total_fee_xlm > budget);

    let mut warnings: Vec<String> = Vec::new();
    if heuristic_count > 0 {
        warnings.push(format!(
            "{} of {} calls could not be simulated and use local heuristic fees",
            heuristic_count, count
        ));
    }
    if high_variance_count > 0 {
        warnings.push(format!(
            "{} high-variance call(s) — re-verify these against a live simulation before submitting",
            high_variance_count
        ));
    }

    BatchForecast {
        network: network.to_string(),
        budget_xlm,
        total_fee_stroops,
        total_fee_xlm,
        avg_fee_stroops,
        median_fee_stroops: median,
        min_fee_stroops,
        max_fee_stroops,
        simulated_count,
        heuristic_count,
        high_variance_count,
        would_exceed_budget,
        items,
        warnings,
    }
}

/// Median per-item fee in stroops (0 for an empty batch).
fn median_stroops(items: &[InvokeFeeEstimate]) -> u64 {
    if items.is_empty() {
        return 0;
    }
    let mut fees: Vec<u64> = items.iter().map(|item| item.fee_stroops).collect();
    fees.sort_unstable();
    let mid = fees.len() / 2;
    if fees.len() % 2 == 1 {
        fees[mid]
    } else {
        (fees[mid - 1] + fees[mid]) / 2
    }
}

// ── Live driver ──────────────────────────────────────────────────────────────

/// Forecast the aggregate fee for a batch manifest by simulating every invoke
/// against its network and falling back to the heuristic when a simulation
/// fails. This is the entry-point used by the `cost forecast-batch` command.
pub async fn estimate_batch_forecast(
    manifest: &InvokeBatchManifest,
    default_network: &str,
    margin_percent: u32,
    inclusion_fee_stroops: u64,
) -> Result<BatchForecast> {
    if margin_percent > sr::MAX_FEE_MARGIN_PERCENT {
        bail!(
            "Fee margin {}% is out of range (expected 0..={}).",
            margin_percent,
            sr::MAX_FEE_MARGIN_PERCENT
        );
    }

    let planned = plan(manifest, default_network)?;
    if planned.is_empty() {
        bail!("Batch manifest contains no invokes to forecast.");
    }
    let network = planned[0].network.clone();

    let mut items = Vec::with_capacity(planned.len());
    for invoke in &planned {
        let args: Vec<String> = invoke.args.iter().map(|arg| arg.value.clone()).collect();
        let types: Vec<String> = invoke.args.iter().map(|arg| arg.arg_type.clone()).collect();

        match soroban::simulate_transaction(
            &invoke.contract_id,
            &invoke.function,
            &args,
            &types,
            &invoke.network,
        )
        .await
        {
            Ok(result) => {
                // Prefer the recommended fee (resource + margin + inclusion
                // fee); fall back to the raw simulated fee when planning fails.
                let fee = result
                    .resources
                    .as_ref()
                    .and_then(|resources| {
                        sr::plan_fee(resources, margin_percent, inclusion_fee_stroops).ok()
                    })
                    .map(|plan| plan.recommended_fee_stroops)
                    .unwrap_or(result.fee);
                items.push(evaluate_item(invoke, Some(fee), result.errors, None));
            }
            Err(error) => {
                items.push(evaluate_item(
                    invoke,
                    None,
                    Vec::new(),
                    Some(error.to_string()),
                ));
            }
        }
    }

    Ok(aggregate_forecast(&network, manifest.budget_xlm, items))
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Manifest parsing / validation ───────────────────────────────────

    #[test]
    fn parses_json_manifest() {
        let manifest: InvokeBatchManifest = serde_json::from_str(
            r#"{
                "version": 1,
                "network": "testnet",
                "budget_xlm": 1.5,
                "invokes": [
                    {
                        "name": "mint",
                        "contract_id": "CAF2JQ7WNBA4TR",
                        "function": "mint",
                        "args": [
                            { "value": "GAAA...", "type": "address" },
                            { "value": "1000", "type": "int" }
                        ],
                        "max_fee_stroops": 500000
                    }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.network.as_deref(), Some("testnet"));
        assert_eq!(manifest.budget_xlm, Some(1.5));
        assert_eq!(manifest.invokes.len(), 1);
        assert_eq!(manifest.invokes[0].args.len(), 2);
        assert_eq!(manifest.invokes[0].args[0].arg_type, "address");
        validate(&manifest).unwrap();
    }

    #[test]
    fn parses_yaml_manifest() {
        let manifest: InvokeBatchManifest = serde_yaml::from_str(
            r#"
version: 1
network: mainnet
invokes:
  - name: transfer
    contract_id: CABACA...
    function: transfer
    args:
      - value: "GBAAAA..."
        type: address
      - value: "5"
        type: int
"#,
        )
        .unwrap();
        assert_eq!(manifest.invokes[0].function, "transfer");
        validate(&manifest).unwrap();
    }

    #[test]
    fn rejects_empty_batch() {
        let err = validate(&InvokeBatchManifest {
            version: 1,
            network: None,
            budget_xlm: None,
            invokes: vec![],
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("at least one invoke"));
    }

    #[test]
    fn rejects_unknown_fields_and_bad_version() {
        let bad_field = serde_json::from_str::<InvokeBatchManifest>(
            r#"{"version":1,"invokes":[],"extra":true}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(bad_field.contains("unknown field `extra`"));

        let manifest: InvokeBatchManifest =
            serde_json::from_str(r#"{"version":2,"invokes":[{"contract_id":"C","function":"f"}]}"#)
                .unwrap();
        assert!(validate(&manifest)
            .unwrap_err()
            .to_string()
            .contains("Unsupported batch manifest version"));
    }

    #[test]
    fn rejects_invalid_argument_types() {
        let manifest: InvokeBatchManifest = serde_json::from_str(
            r#"{"version":1,"invokes":[{"contract_id":"C","function":"f","args":[{"value":"x","type":"tuple"}]}]}"#,
        )
        .unwrap();
        assert!(validate(&manifest)
            .unwrap_err()
            .to_string()
            .contains("type 'tuple' is invalid"));
    }

    #[test]
    fn rejects_negative_budget() {
        let manifest: InvokeBatchManifest = serde_json::from_str(
            r#"{"version":1,"budget_xlm":-1.0,"invokes":[{"contract_id":"C","function":"f"}]}"#,
        )
        .unwrap();
        assert!(validate(&manifest).is_err());
    }

    #[test]
    fn round_trips_through_json() {
        let manifest: InvokeBatchManifest =
            serde_json::from_str(r#"{"version":1,"invokes":[{"contract_id":"C","function":"f"}]}"#)
                .unwrap();
        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: InvokeBatchManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn loads_manifest_from_disk_based_on_extension() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let path = dir.path().join("batch.json");
        fs::write(
            &path,
            r#"{"version":1,"invokes":[{"contract_id":"C","function":"f"}]}"#,
        )
        .unwrap();
        let manifest = load_manifest(&path).unwrap();
        assert_eq!(manifest.invokes.len(), 1);

        // A YAML extension of the same shape parses too.
        let yaml_path = dir.path().join("batch.yaml");
        fs::write(
            &yaml_path,
            "version: 1\ninvokes:\n  - contract_id: C\n    function: f\n",
        )
        .unwrap();
        assert_eq!(load_manifest(&yaml_path).unwrap().invokes.len(), 1);
    }

    // ── Planning ────────────────────────────────────────────────────────

    #[test]
    fn plan_resolves_network_precedence() {
        std::env::set_var("STARFORGE_BATCH_CTR", "CDEF");
        let manifest: InvokeBatchManifest = serde_json::from_str(
            r#"{
                "version": 1,
                "network": "testnet",
                "invokes": [
                    { "contract_id": "${STARFORGE_BATCH_CTR}", "function": "a" },
                    { "contract_id": "C", "function": "b", "network": "mainnet" }
                ]
            }"#,
        )
        .unwrap();
        let planned = plan(&manifest, "futurenet").unwrap();
        assert_eq!(planned[0].network, "testnet");
        assert_eq!(planned[0].contract_id, "CDEF");
        assert_eq!(planned[1].network, "mainnet");
        assert_eq!(planned[0].name, "#1");
        assert_eq!(planned[1].name, "#2");
    }

    // ── Heuristic estimation ─────────────────────────────────────────────

    fn planned(index: usize, function: &str, arg_values: &[&str]) -> PlannedInvoke {
        PlannedInvoke {
            index,
            name: format!("#{}", index + 1),
            contract_id: "CAF2JQ7WNBA4TR".to_string(),
            function: function.to_string(),
            args: arg_values
                .iter()
                .map(|value| ManifestArgument {
                    value: value.to_string(),
                    arg_type: "string".to_string(),
                })
                .collect(),
            network: "testnet".to_string(),
            max_fee_stroops: None,
        }
    }

    #[test]
    fn heuristic_fee_is_deterministic_and_grows_with_call_size() {
        let small = estimate_heuristic_fee(&planned(0, "ping", &[]));
        let big = estimate_heuristic_fee(&planned(
            1,
            "mint_to_many_recipients",
            &["GBAAAA...AAA", "1000000", "symbol-here"],
        ));
        assert!(small > 0);
        assert!(big > small);
        assert_eq!(estimate_heuristic_fee(&planned(2, "ping", &[])), small);
    }

    // ── evaluate_item paths ──────────────────────────────────────────────

    #[test]
    fn simulated_fee_is_used_and_not_flagged() {
        let item = evaluate_item(&planned(0, "ping", &[]), Some(58_181), vec![], None);
        assert_eq!(item.source, EstimateSource::Simulated);
        assert_eq!(item.fee_stroops, 58_181);
        assert!(!item.high_variance);
        assert!(item.errors.is_empty());
    }

    #[test]
    fn simulation_failure_falls_back_to_heuristic_and_is_flagged() {
        let invoke = planned(0, "ping", &[]);
        let item = evaluate_item(
            &invoke,
            None,
            vec![],
            Some("Simulation request failed: connection refused".to_string()),
        );
        assert_eq!(item.source, EstimateSource::Heuristic);
        assert_eq!(item.fee_stroops, estimate_heuristic_fee(&invoke));
        assert!(item.high_variance);
        assert!(item
            .variance_reasons
            .iter()
            .any(|reason| reason.contains("local heuristic")));
        assert!(item
            .errors
            .iter()
            .any(|error| error.contains("connection refused")));
    }

    #[test]
    fn zero_simulated_fee_is_kept_but_warned() {
        let item = evaluate_item(&planned(0, "ping", &[]), Some(0), vec![], None);
        assert_eq!(item.source, EstimateSource::Simulated);
        assert_eq!(item.fee_stroops, 0);
        assert!(item
            .warnings
            .iter()
            .any(|w| w.contains("zero resource fee")));
    }

    #[test]
    fn per_call_cap_exceeded_is_flagged() {
        let mut invoke = planned(0, "ping", &[]);
        invoke.max_fee_stroops = Some(50_000);
        let item = evaluate_item(&invoke, Some(58_181), vec![], None);
        assert!(item.would_exceed_cap);
        assert!(item.high_variance);
    }

    // ── Aggregation ──────────────────────────────────────────────────────

    fn estimate(index: usize, fee: u64, source: EstimateSource) -> InvokeFeeEstimate {
        InvokeFeeEstimate {
            index,
            name: format!("#{}", index + 1),
            contract_id: "CAF2JQ7WNBA4TR".to_string(),
            function: "f".to_string(),
            network: "testnet".to_string(),
            fee_stroops: fee,
            fee_xlm: fee as f64 / sr::STROOPS_PER_XLM,
            source,
            high_variance: source == EstimateSource::Heuristic,
            variance_reasons: if source == EstimateSource::Heuristic {
                vec!["simulation unavailable".to_string()]
            } else {
                vec![]
            },
            warnings: vec![],
            errors: vec![],
            max_fee_stroops: None,
            would_exceed_cap: false,
        }
    }

    #[test]
    fn aggregate_handles_empty_batch_with_zero_totals() {
        let forecast = aggregate_forecast("testnet", None, vec![]);
        assert_eq!(forecast.items.len(), 0);
        assert_eq!(forecast.total_fee_stroops, 0);
        assert_eq!(forecast.total_fee_xlm, 0.0);
        assert_eq!(forecast.avg_fee_stroops, 0);
        assert_eq!(forecast.min_fee_stroops, 0);
        assert_eq!(forecast.max_fee_stroops, 0);
        assert_eq!(forecast.simulated_count, 0);
        assert_eq!(forecast.heuristic_count, 0);
        assert!(!forecast.would_exceed_budget);
    }

    #[test]
    fn aggregate_sums_totals_and_splits_sources() {
        let items = vec![
            estimate(0, 100_000, EstimateSource::Simulated),
            estimate(1, 200_000, EstimateSource::Simulated),
            estimate(2, 300_000, EstimateSource::Heuristic),
        ];
        let forecast = aggregate_forecast("testnet", None, items);
        assert_eq!(forecast.total_fee_stroops, 600_000);
        assert!((forecast.total_fee_xlm - 0.06).abs() < 1e-9);
        assert_eq!(forecast.avg_fee_stroops, 200_000);
        assert_eq!(forecast.min_fee_stroops, 100_000);
        assert_eq!(forecast.max_fee_stroops, 300_000);
        assert_eq!(forecast.median_fee_stroops, 200_000);
        assert_eq!(forecast.simulated_count, 2);
        assert_eq!(forecast.heuristic_count, 1);
        assert_eq!(forecast.high_variance_count, 1);
        assert!(forecast
            .warnings
            .iter()
            .any(|w| w.contains("1 of 3 calls could not be simulated")));
    }

    #[test]
    fn aggregate_flags_outlier_calls_as_high_variance() {
        let items = vec![
            estimate(0, 10_000, EstimateSource::Simulated),
            estimate(1, 20_000, EstimateSource::Simulated),
            estimate(2, 1_000_000, EstimateSource::Simulated),
            estimate(3, 15_000, EstimateSource::Simulated),
        ];
        let forecast = aggregate_forecast("testnet", None, items);
        let outlier = forecast.items.iter().find(|item| item.index == 2).unwrap();
        assert!(outlier.high_variance);
        assert!(outlier
            .variance_reasons
            .iter()
            .any(|reason| reason.contains("median")));
        let normal = forecast.items.iter().find(|item| item.index == 0).unwrap();
        assert!(!normal.high_variance);
        assert_eq!(forecast.high_variance_count, 1);
    }

    #[test]
    fn aggregate_reports_budget_exceeded() {
        let items = vec![
            estimate(0, 6_000_000, EstimateSource::Simulated),
            estimate(1, 6_000_000, EstimateSource::Simulated),
        ];
        let forecast = aggregate_forecast("testnet", Some(1.0), items);
        assert!(forecast.would_exceed_budget);
    }

    #[test]
    fn aggregate_reports_budget_ok_within_limit() {
        let items = vec![estimate(0, 600_000, EstimateSource::Simulated)];
        let forecast = aggregate_forecast("testnet", Some(1.0), items);
        assert!(!forecast.would_exceed_budget);
    }

    // ── estimate_batch_forecast guard rails ──────────────────────────────

    #[tokio::test]
    async fn batch_forecast_rejects_empty_manifest() {
        let manifest: InvokeBatchManifest =
            serde_json::from_str(r#"{"version":1,"invokes":[]}"#).unwrap();
        let result = estimate_batch_forecast(&manifest, "testnet", 20, 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn batch_forecast_rejects_out_of_range_margin() {
        let manifest: InvokeBatchManifest =
            serde_json::from_str(r#"{"version":1,"invokes":[{"contract_id":"C","function":"f"}]}"#)
                .unwrap();
        let result = estimate_batch_forecast(&manifest, "testnet", 2_000, 100).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("margin"));
    }
}
