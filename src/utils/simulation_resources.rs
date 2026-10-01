//! Soroban simulation resource accounting and transaction fee planning.
//!
//! `simulateTransaction` is the only source of truth for what a Soroban
//! transaction will actually cost: the RPC server runs the invocation against a
//! real ledger snapshot and reports the CPU instructions burned, the linear
//! memory touched, the ledger footprint that must be declared, and the
//! **minimum resource fee** the transaction has to carry to be accepted.
//!
//! This module turns a raw `simulateTransaction` JSON-RPC response into a
//! strongly-typed [`SimulationResources`] value and derives a
//! [`ResourceFeePlan`] from it, so `simulate`, `cost`, and `deploy` all report
//! the same numbers instead of guessing at a hard-coded fee.
//!
//! ## Compatibility
//!
//! | RPC field          | Protocol 20 | Protocol 21/22 | Notes |
//! |--------------------|-------------|----------------|-------|
//! | `minResourceFee`   | yes         | yes            | required; parse fails without it |
//! | `cost.cpuInsns`    | yes         | deprecated     | falls back to `transactionData` |
//! | `cost.memBytes`    | yes         | deprecated     | reported as "unavailable" when absent |
//! | `transactionData`  | yes         | yes            | source of the footprint |
//! | `restorePreamble`  | no          | yes            | archived entries need a restore tx first |
//!
//! Every numeric field is accepted either as a JSON number or as a JSON string
//! (stellar-rpc emits `u64` values as strings), and anything that is not a
//! non-negative integer is rejected rather than silently coerced to zero.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stellar_xdr::curr::{Limits, ReadXdr, SorobanTransactionData};

/// Stroops in one XLM.
pub const STROOPS_PER_XLM: f64 = 10_000_000.0;

/// Default safety margin applied on top of the simulated minimum resource fee.
///
/// Ledger state can move between simulation and submission, so submitting
/// exactly `minResourceFee` is a coin flip. 20% is the margin the Stellar CLI
/// uses by default.
pub const DEFAULT_FEE_MARGIN_PERCENT: u32 = 20;

/// Upper bound accepted for `--margin`. Anything higher is almost certainly a
/// typo (e.g. passing stroops instead of a percentage).
pub const MAX_FEE_MARGIN_PERCENT: u32 = 1_000;

/// Default per-operation inclusion fee in stroops (the network base fee).
pub const DEFAULT_INCLUSION_FEE_STROOPS: u64 = 100;

/// Largest simulation response we are willing to parse, in bytes. Guards the
/// `--file` code path against accidentally reading a multi-gigabyte artifact.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

/// Everything that can go wrong while turning a simulation response into a
/// resource plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationResourceError {
    /// The payload was not a JSON object (e.g. an array, a bare string, null).
    NotAnObject,
    /// The JSON-RPC envelope carried an `error` member.
    RpcError(String),
    /// The simulation itself failed on the host (contract panic, bad auth, …).
    SimulationFailed(String),
    /// A field was present but not usable.
    InvalidField { field: String, reason: String },
    /// A field required for fee planning was absent.
    MissingField(String),
    /// The response is missing the resource accounting this command needs,
    /// typically because the endpoint is not a Soroban RPC server.
    UnsupportedResponse(String),
    /// The requested margin is outside the accepted range.
    InvalidMargin(u32),
    /// Fee arithmetic overflowed `u64`.
    FeeOverflow,
    /// Input exceeded [`MAX_RESPONSE_BYTES`].
    ResponseTooLarge { bytes: usize, limit: usize },
    /// An unknown simulation profile name was requested.
    UnknownProfile {
        name: String,
        available: Vec<String>,
    },
    /// Simulation exceeded the resource ceilings asserted by the active profile.
    ProfileCeilingExceeded {
        profile: String,
        violations: Vec<ProfileViolation>,
    },
}

impl std::fmt::Display for SimulationResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject => write!(
                f,
                "simulation response is not a JSON object; expected the result of a \
                 `simulateTransaction` call"
            ),
            Self::RpcError(msg) => write!(f, "Soroban RPC returned an error: {}", msg),
            Self::SimulationFailed(msg) => write!(f, "simulation failed on the host: {}", msg),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid `{}` in simulation response: {}", field, reason)
            }
            Self::MissingField(field) => write!(
                f,
                "simulation response is missing `{}`; cannot plan transaction resources",
                field
            ),
            Self::UnsupportedResponse(msg) => write!(f, "unsupported simulation response: {}", msg),
            Self::InvalidMargin(pct) => write!(
                f,
                "fee margin {}% is out of range (expected 0..={})",
                pct, MAX_FEE_MARGIN_PERCENT
            ),
            Self::FeeOverflow => write!(f, "resource fee calculation overflowed u64 stroops"),
            Self::ResponseTooLarge { bytes, limit } => write!(
                f,
                "simulation response is {} bytes, above the {} byte limit",
                bytes, limit
            ),
            Self::UnknownProfile { name, available } => write!(
                f,
                "unknown simulation profile '{}'; available profiles: {}",
                name,
                available.join(", ")
            ),
            Self::ProfileCeilingExceeded {
                profile,
                violations,
            } => {
                write!(
                    f,
                    "simulation exceeded profile '{}' ceilings ({} violation(s)): {}",
                    profile,
                    violations.len(),
                    violations
                        .iter()
                        .map(|v| v.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            }
        }
    }
}

impl std::error::Error for SimulationResourceError {}

type Result<T> = std::result::Result<T, SimulationResourceError>;

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// The ledger footprint a transaction must declare, decoded from the
/// `transactionData` XDR returned by simulation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationFootprint {
    /// Number of read-only ledger keys.
    pub read_only_entries: usize,
    /// Number of read-write ledger keys.
    pub read_write_entries: usize,
    /// Bytes read from the ledger.
    pub read_bytes: u32,
    /// Bytes written back to the ledger.
    pub write_bytes: u32,
    /// CPU instructions declared in the resource section of `transactionData`.
    pub instructions: u32,
    /// Resource fee (stroops) baked into `transactionData` by the simulator.
    pub resource_fee_stroops: i64,
}

impl SimulationFootprint {
    /// Total number of ledger entries touched.
    pub fn total_entries(&self) -> usize {
        self.read_only_entries + self.read_write_entries
    }
}

/// Resource accounting extracted from a `simulateTransaction` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationResources {
    /// Minimum resource fee in stroops the transaction must carry.
    pub min_resource_fee_stroops: u64,
    /// CPU instructions consumed. `None` when the RPC server no longer reports
    /// the deprecated `cost` object and `transactionData` was unavailable.
    pub cpu_instructions: Option<u64>,
    /// Linear memory high-water mark in bytes. `None` on RPC servers that have
    /// dropped the deprecated `cost` object.
    pub memory_bytes: Option<u64>,
    /// Declared ledger footprint, when `transactionData` was present.
    pub footprint: Option<SimulationFootprint>,
    /// Ledger the simulation ran against.
    pub latest_ledger: Option<u32>,
    /// Extra resource fee for the restore transaction that must run first when
    /// the footprint touches archived entries.
    pub restore_fee_stroops: Option<u64>,
    /// Base64 `transactionData` from `restorePreamble`, used to build a
    /// [`RestoreFootprint`](stellar_xdr::curr::RestoreFootprintOp) transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_transaction_data: Option<String>,
    /// Non-fatal notes about fields the server did not provide.
    pub warnings: Vec<String>,
}

impl SimulationResources {
    /// True when the RPC response told us archived ledger entries must be
    /// restored before this transaction can succeed.
    pub fn requires_restore(&self) -> bool {
        self.restore_fee_stroops.is_some()
    }

    /// Minimum resource fee expressed in XLM.
    pub fn min_resource_fee_xlm(&self) -> f64 {
        self.min_resource_fee_stroops as f64 / STROOPS_PER_XLM
    }
}

/// A submittable fee derived from simulation output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceFeePlan {
    /// Minimum resource fee reported by simulation.
    pub min_resource_fee_stroops: u64,
    /// Resource fee for the prerequisite restore transaction, if any.
    pub restore_fee_stroops: u64,
    /// Per-operation inclusion (base) fee.
    pub inclusion_fee_stroops: u64,
    /// Safety margin applied to the resource fees, as a percentage.
    pub margin_percent: u32,
    /// Absolute stroops the margin adds.
    pub margin_stroops: u64,
    /// Fee to put on the transaction: resource + restore + margin + inclusion.
    pub recommended_fee_stroops: u64,
}

impl ResourceFeePlan {
    /// Recommended fee expressed in XLM.
    pub fn recommended_fee_xlm(&self) -> f64 {
        self.recommended_fee_stroops as f64 / STROOPS_PER_XLM
    }
}

/// A deterministic simulation profile defining resource ceilings and defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationProfile {
    /// Name identifier for the profile (e.g. `ci-smoke`, `ci-full`, `dev-fast`).
    pub name: String,
    /// Human description of the profile scope.
    pub description: String,
    /// Maximum allowed CPU instructions burned.
    pub max_cpu_instructions: u64,
    /// Maximum allowed linear memory in bytes.
    pub max_memory_bytes: u64,
    /// Maximum allowed ledger read bytes.
    pub max_read_bytes: u64,
    /// Maximum allowed ledger write bytes.
    pub max_write_bytes: u64,
    /// Maximum allowed total footprint entries (read-only + read-write).
    pub max_total_entries: usize,
    /// Maximum allowed recommended fee in stroops.
    pub max_fee_stroops: u64,
    /// Default safety margin percent applied in this profile.
    pub default_margin_percent: u32,
}

/// A specific resource ceiling breach when evaluating against a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileViolation {
    pub metric: String,
    pub actual: u64,
    pub ceiling: u64,
    pub message: String,
}

/// Return all built-in deterministic simulation profiles.
pub fn built_in_profiles() -> Vec<SimulationProfile> {
    vec![
        SimulationProfile {
            name: "ci-smoke".to_string(),
            description: "Strict tight resource limits for fast CI sanity checks".to_string(),
            max_cpu_instructions: 2_000_000,
            max_memory_bytes: 2_097_152, // 2 MB
            max_read_bytes: 32_768,      // 32 KB
            max_write_bytes: 8_192,      // 8 KB
            max_total_entries: 10,
            max_fee_stroops: 100_000,
            default_margin_percent: 20,
        },
        SimulationProfile {
            name: "ci-full".to_string(),
            description: "Standard production-grade ceilings for full CI test pipelines"
                .to_string(),
            max_cpu_instructions: 100_000_000,
            max_memory_bytes: 41_943_040, // 40 MB
            max_read_bytes: 200_000,
            max_write_bytes: 100_000,
            max_total_entries: 100,
            max_fee_stroops: 10_000_000,
            default_margin_percent: 20,
        },
        SimulationProfile {
            name: "dev-fast".to_string(),
            description: "Relaxed developer iteration limits with lower margin for rapid loops"
                .to_string(),
            max_cpu_instructions: 50_000_000,
            max_memory_bytes: 20_971_520, // 20 MB
            max_read_bytes: 100_000,
            max_write_bytes: 50_000,
            max_total_entries: 50,
            max_fee_stroops: 5_000_000,
            default_margin_percent: 10,
        },
    ]
}

/// Retrieve a simulation profile by name, failing loudly if not found.
pub fn get_profile(name: &str) -> Result<SimulationProfile> {
    let profiles = built_in_profiles();
    for p in &profiles {
        if p.name.eq_ignore_ascii_case(name) {
            return Ok(p.clone());
        }
    }

    let available = profiles.into_iter().map(|p| p.name).collect();
    Err(SimulationResourceError::UnknownProfile {
        name: name.to_string(),
        available,
    })
}

/// Validate simulation resources and fee plan against a profile's resource ceilings.
pub fn validate_against_profile(
    resources: &SimulationResources,
    plan: &ResourceFeePlan,
    profile: &SimulationProfile,
) -> Vec<ProfileViolation> {
    let mut violations = Vec::new();

    if let Some(cpu) = resources.cpu_instructions {
        if cpu > profile.max_cpu_instructions {
            violations.push(ProfileViolation {
                metric: "cpu_instructions".to_string(),
                actual: cpu,
                ceiling: profile.max_cpu_instructions,
                message: format!(
                    "CPU instructions {} exceeded profile ceiling of {}",
                    cpu, profile.max_cpu_instructions
                ),
            });
        }
    }

    if let Some(mem) = resources.memory_bytes {
        if mem > profile.max_memory_bytes {
            violations.push(ProfileViolation {
                metric: "memory_bytes".to_string(),
                actual: mem,
                ceiling: profile.max_memory_bytes,
                message: format!(
                    "Memory bytes {} exceeded profile ceiling of {}",
                    mem, profile.max_memory_bytes
                ),
            });
        }
    }

    if let Some(fp) = &resources.footprint {
        let total_entries = fp.total_entries() as u64;
        if total_entries > profile.max_total_entries as u64 {
            violations.push(ProfileViolation {
                metric: "footprint_entries".to_string(),
                actual: total_entries,
                ceiling: profile.max_total_entries as u64,
                message: format!(
                    "Footprint total entries {} exceeded profile ceiling of {}",
                    total_entries, profile.max_total_entries
                ),
            });
        }

        let read_bytes = u64::from(fp.read_bytes);
        if read_bytes > profile.max_read_bytes {
            violations.push(ProfileViolation {
                metric: "read_bytes".to_string(),
                actual: read_bytes,
                ceiling: profile.max_read_bytes,
                message: format!(
                    "Ledger read bytes {} exceeded profile ceiling of {}",
                    read_bytes, profile.max_read_bytes
                ),
            });
        }

        let write_bytes = u64::from(fp.write_bytes);
        if write_bytes > profile.max_write_bytes {
            violations.push(ProfileViolation {
                metric: "write_bytes".to_string(),
                actual: write_bytes,
                ceiling: profile.max_write_bytes,
                message: format!(
                    "Ledger write bytes {} exceeded profile ceiling of {}",
                    write_bytes, profile.max_write_bytes
                ),
            });
        }
    }

    if plan.recommended_fee_stroops > profile.max_fee_stroops {
        violations.push(ProfileViolation {
            metric: "recommended_fee_stroops".to_string(),
            actual: plan.recommended_fee_stroops,
            ceiling: profile.max_fee_stroops,
            message: format!(
                "Recommended fee {} stroops exceeded profile ceiling of {} stroops",
                plan.recommended_fee_stroops, profile.max_fee_stroops
            ),
        });
    }

    violations
}

// ─────────────────────────────────────────────────────────────────────────────
// Parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a raw JSON document (either a full JSON-RPC envelope or a bare
/// `result` object) into [`SimulationResources`].
///
/// Rejects inputs above [`MAX_RESPONSE_BYTES`] before touching the JSON parser.
pub fn parse_simulation_response_str(raw: &str) -> Result<SimulationResources> {
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err(SimulationResourceError::ResponseTooLarge {
            bytes: raw.len(),
            limit: MAX_RESPONSE_BYTES,
        });
    }
    let value: Value =
        serde_json::from_str(raw).map_err(|e| SimulationResourceError::InvalidField {
            field: "<document>".to_string(),
            reason: format!("not valid JSON: {}", e),
        })?;
    parse_simulation_resources(&value)
}

/// Parse a `simulateTransaction` response into [`SimulationResources`].
///
/// Accepts both the full JSON-RPC envelope (`{"jsonrpc":…,"result":{…}}`) and a
/// bare result object, so callers can feed it either a saved response or the
/// already-unwrapped value from [`crate::utils::soroban`].
pub fn parse_simulation_resources(value: &Value) -> Result<SimulationResources> {
    let obj = value
        .as_object()
        .ok_or(SimulationResourceError::NotAnObject)?;

    // JSON-RPC transport error takes precedence over anything else.
    if let Some(err) = obj.get("error") {
        // A bare `result` object also uses `error` for host failures; the
        // transport variant is an object with `code`/`message`.
        if err.is_object() {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(SimulationResourceError::RpcError(msg.to_string()));
        }
        if let Some(msg) = err.as_str() {
            if !msg.is_empty() {
                return Err(SimulationResourceError::SimulationFailed(msg.to_string()));
            }
        }
    }

    // Unwrap the JSON-RPC envelope when present.
    if let Some(result) = obj.get("result") {
        if result.is_object() {
            return parse_simulation_resources(result);
        }
    }

    if !obj.contains_key("minResourceFee") && !obj.contains_key("transactionData") {
        return Err(SimulationResourceError::UnsupportedResponse(
            "no `minResourceFee` or `transactionData`; the endpoint does not look like a \
             Soroban RPC `simulateTransaction` result"
                .to_string(),
        ));
    }

    let mut warnings = Vec::new();

    let min_resource_fee_stroops = match obj.get("minResourceFee") {
        Some(v) => parse_u64_field(v, "minResourceFee")?,
        None => {
            return Err(SimulationResourceError::MissingField(
                "minResourceFee".to_string(),
            ))
        }
    };

    let footprint = match obj.get("transactionData") {
        Some(Value::Null) | None => {
            warnings.push(
                "`transactionData` absent: footprint and declared resources are unavailable"
                    .to_string(),
            );
            None
        }
        Some(v) => {
            let encoded = v
                .as_str()
                .ok_or_else(|| SimulationResourceError::InvalidField {
                    field: "transactionData".to_string(),
                    reason: "expected a base64-encoded XDR string".to_string(),
                })?;
            if encoded.is_empty() {
                warnings.push("`transactionData` is empty: footprint unavailable".to_string());
                None
            } else {
                Some(decode_footprint(encoded)?)
            }
        }
    };

    let cost = obj.get("cost");
    let mut cpu_instructions = match cost.and_then(|c| c.get("cpuInsns")) {
        Some(v) => Some(parse_u64_field(v, "cost.cpuInsns")?),
        None => None,
    };
    let memory_bytes = match cost.and_then(|c| c.get("memBytes")) {
        Some(v) => Some(parse_u64_field(v, "cost.memBytes")?),
        None => {
            warnings.push(
                "`cost.memBytes` not reported by this RPC server (deprecated since protocol 21)"
                    .to_string(),
            );
            None
        }
    };

    // Protocol 21+ servers drop `cost`; the declared instruction count in
    // `transactionData` is the remaining CPU signal.
    if cpu_instructions.is_none() {
        if let Some(fp) = footprint.as_ref() {
            cpu_instructions = Some(u64::from(fp.instructions));
            warnings.push(
                "CPU instructions taken from `transactionData` because `cost.cpuInsns` is absent"
                    .to_string(),
            );
        }
    }

    let latest_ledger = match obj.get("latestLedger") {
        Some(v) => Some(
            u32::try_from(parse_u64_field(v, "latestLedger")?).map_err(|_| {
                SimulationResourceError::InvalidField {
                    field: "latestLedger".to_string(),
                    reason: "value does not fit in a u32 ledger sequence".to_string(),
                }
            })?,
        ),
        None => None,
    };

    let (restore_fee_stroops, restore_transaction_data) = match obj.get("restorePreamble") {
        Some(Value::Null) | None => (None, None),
        Some(preamble) => {
            let fee = preamble.get("minResourceFee").ok_or_else(|| {
                SimulationResourceError::MissingField("restorePreamble.minResourceFee".to_string())
            })?;
            let fee = Some(parse_u64_field(fee, "restorePreamble.minResourceFee")?);
            let tx_data = preamble
                .get("transactionData")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            (fee, tx_data)
        }
    };
    if restore_fee_stroops.is_some() {
        warnings.push(
            "footprint touches archived entries: a restore transaction must be submitted first"
                .to_string(),
        );
    }

    Ok(SimulationResources {
        min_resource_fee_stroops,
        cpu_instructions,
        memory_bytes,
        footprint,
        latest_ledger,
        restore_fee_stroops,
        restore_transaction_data,
        warnings,
    })
}

/// Accept a `u64` supplied either as a JSON number or a JSON string.
///
/// stellar-rpc serialises 64-bit counters as strings to survive JavaScript
/// clients; both spellings must round-trip, but floats, negatives, and
/// non-numeric text are hard errors rather than silent zeroes.
fn parse_u64_field(value: &Value, field: &str) -> Result<u64> {
    let invalid = |reason: &str| SimulationResourceError::InvalidField {
        field: field.to_string(),
        reason: reason.to_string(),
    };

    match value {
        Value::Number(n) => {
            if let Some(v) = n.as_u64() {
                Ok(v)
            } else if n.as_i64().is_some() {
                Err(invalid("expected a non-negative integer"))
            } else {
                Err(invalid("expected an integer, got a fractional number"))
            }
        }
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err(invalid("value is empty"));
            }
            if !trimmed.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid("expected a decimal non-negative integer string"));
            }
            trimmed
                .parse::<u64>()
                .map_err(|_| invalid("value does not fit in u64 stroops"))
        }
        Value::Null => Err(invalid("value is null")),
        _ => Err(invalid("expected a number or a numeric string")),
    }
}

/// Decode the base64 `SorobanTransactionData` XDR into a footprint summary.
fn decode_footprint(encoded: &str) -> Result<SimulationFootprint> {
    let bytes =
        BASE64
            .decode(encoded.trim())
            .map_err(|e| SimulationResourceError::InvalidField {
                field: "transactionData".to_string(),
                reason: format!("not valid base64: {}", e),
            })?;

    let data = SorobanTransactionData::from_xdr(&bytes, Limits::depth(100)).map_err(|e| {
        SimulationResourceError::InvalidField {
            field: "transactionData".to_string(),
            reason: format!("not a valid SorobanTransactionData XDR envelope: {}", e),
        }
    })?;

    Ok(SimulationFootprint {
        read_only_entries: data.resources.footprint.read_only.len(),
        read_write_entries: data.resources.footprint.read_write.len(),
        read_bytes: data.resources.read_bytes,
        write_bytes: data.resources.write_bytes,
        instructions: data.resources.instructions,
        resource_fee_stroops: data.resource_fee,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Planning
// ─────────────────────────────────────────────────────────────────────────────

/// Derive a submittable fee from simulation output.
///
/// `margin_percent` is applied to the resource fees only (the inclusion fee is
/// a flat network base fee and is added afterwards). All arithmetic is
/// overflow-checked so a hostile or corrupted response cannot wrap the total.
pub fn plan_fee(
    resources: &SimulationResources,
    margin_percent: u32,
    inclusion_fee_stroops: u64,
) -> Result<ResourceFeePlan> {
    if margin_percent > MAX_FEE_MARGIN_PERCENT {
        return Err(SimulationResourceError::InvalidMargin(margin_percent));
    }

    let restore_fee_stroops = resources.restore_fee_stroops.unwrap_or(0);
    let resource_total = resources
        .min_resource_fee_stroops
        .checked_add(restore_fee_stroops)
        .ok_or(SimulationResourceError::FeeOverflow)?;

    let margin_stroops = resource_total
        .checked_mul(u64::from(margin_percent))
        .ok_or(SimulationResourceError::FeeOverflow)?
        / 100;

    let recommended_fee_stroops = resource_total
        .checked_add(margin_stroops)
        .and_then(|v| v.checked_add(inclusion_fee_stroops))
        .ok_or(SimulationResourceError::FeeOverflow)?;

    Ok(ResourceFeePlan {
        min_resource_fee_stroops: resources.min_resource_fee_stroops,
        restore_fee_stroops,
        inclusion_fee_stroops,
        margin_percent,
        margin_stroops,
        recommended_fee_stroops,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Reporting
// ─────────────────────────────────────────────────────────────────────────────

/// Render a human-readable resource + fee report using the shared print helpers.
pub fn render_report(resources: &SimulationResources, plan: &ResourceFeePlan) {
    use crate::utils::print as p;

    p::header("Simulated Transaction Resources");
    p::separator();

    p::kv(
        "CPU instructions",
        &resources
            .cpu_instructions
            .map(format_thousands)
            .unwrap_or_else(|| "not reported".to_string()),
    );
    p::kv(
        "Memory (bytes)",
        &resources
            .memory_bytes
            .map(format_thousands)
            .unwrap_or_else(|| "not reported".to_string()),
    );

    match &resources.footprint {
        Some(fp) => {
            p::kv(
                "Footprint entries",
                &format!(
                    "{} total ({} read-only, {} read-write)",
                    fp.total_entries(),
                    fp.read_only_entries,
                    fp.read_write_entries
                ),
            );
            p::kv(
                "Ledger read bytes",
                &format_thousands(u64::from(fp.read_bytes)),
            );
            p::kv(
                "Ledger write bytes",
                &format_thousands(u64::from(fp.write_bytes)),
            );
        }
        None => p::kv("Footprint", "unavailable (no transactionData)"),
    }

    if let Some(ledger) = resources.latest_ledger {
        p::kv("Simulated at ledger", &ledger.to_string());
    }

    p::separator();
    p::kv(
        "Min resource fee",
        &format!(
            "{} stroops ({:.7} XLM)",
            format_thousands(plan.min_resource_fee_stroops),
            resources.min_resource_fee_xlm()
        ),
    );
    if plan.restore_fee_stroops > 0 {
        p::kv(
            "Restore fee",
            &format!("{} stroops", format_thousands(plan.restore_fee_stroops)),
        );
    }
    p::kv(
        &format!("Safety margin ({}%)", plan.margin_percent),
        &format!("{} stroops", format_thousands(plan.margin_stroops)),
    );
    p::kv(
        "Inclusion fee",
        &format!("{} stroops", format_thousands(plan.inclusion_fee_stroops)),
    );
    p::kv_accent(
        "Recommended fee",
        &format!(
            "{} stroops ({:.7} XLM)",
            format_thousands(plan.recommended_fee_stroops),
            plan.recommended_fee_xlm()
        ),
    );
    p::separator();

    for warning in &resources.warnings {
        p::warn(warning);
    }
}

/// Render profile evaluation results and violations.
pub fn render_profile_report(profile: &SimulationProfile, violations: &[ProfileViolation]) {
    use crate::utils::print as p;

    p::header(&format!("Simulation Profile: {}", profile.name));
    p::separator();
    p::kv("Description", &profile.description);
    p::kv("Max CPU", &format_thousands(profile.max_cpu_instructions));
    p::kv(
        "Max Memory",
        &format!("{} bytes", format_thousands(profile.max_memory_bytes)),
    );
    p::kv(
        "Max Read Bytes",
        &format!("{} bytes", format_thousands(profile.max_read_bytes)),
    );
    p::kv(
        "Max Write Bytes",
        &format!("{} bytes", format_thousands(profile.max_write_bytes)),
    );
    p::kv(
        "Max Footprint Entries",
        &profile.max_total_entries.to_string(),
    );
    p::kv(
        "Max Fee",
        &format!("{} stroops", format_thousands(profile.max_fee_stroops)),
    );
    p::separator();

    if violations.is_empty() {
        p::success(&format!(
            "All simulation resource metrics within profile '{}' ceilings.",
            profile.name
        ));
    } else {
        p::error(&format!(
            "Profile '{}' ceiling assertion failed ({} violation(s)):",
            profile.name,
            violations.len()
        ));
        for v in violations {
            println!(
                "  • {}: actual {} vs ceiling {}",
                v.metric, v.actual, v.ceiling
            );
        }
    }
}

/// Machine-readable form of a resource report with optional profile evaluation.
pub fn report_json_with_profile(
    resources: &SimulationResources,
    plan: &ResourceFeePlan,
    profile_eval: Option<(&SimulationProfile, &[ProfileViolation])>,
) -> Value {
    let mut base = report_json(resources, plan);
    if let Some((profile, violations)) = profile_eval {
        base["profile"] = serde_json::json!({
            "name": profile.name,
            "description": profile.description,
            "passed": violations.is_empty(),
            "violations_count": violations.len(),
            "violations": violations,
            "ceilings": {
                "max_cpu_instructions": profile.max_cpu_instructions,
                "max_memory_bytes": profile.max_memory_bytes,
                "max_read_bytes": profile.max_read_bytes,
                "max_write_bytes": profile.max_write_bytes,
                "max_total_entries": profile.max_total_entries,
                "max_fee_stroops": profile.max_fee_stroops,
            }
        });
    }
    base
}

/// Machine-readable form of a resource report, for `--json` output.
pub fn report_json(resources: &SimulationResources, plan: &ResourceFeePlan) -> Value {
    serde_json::json!({
        "resources": resources,
        "plan": {
            "min_resource_fee_stroops": plan.min_resource_fee_stroops,
            "restore_fee_stroops": plan.restore_fee_stroops,
            "inclusion_fee_stroops": plan.inclusion_fee_stroops,
            "margin_percent": plan.margin_percent,
            "margin_stroops": plan.margin_stroops,
            "recommended_fee_stroops": plan.recommended_fee_stroops,
            "recommended_fee_xlm": plan.recommended_fee_xlm(),
        }
    })
}

/// Group a number with thousands separators for terminal output.
fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use stellar_xdr::curr::{
        ExtensionPoint, Hash, LedgerFootprint, LedgerKey, LedgerKeyContractCode, SorobanResources,
        WriteXdr,
    };

    /// Build a real `SorobanTransactionData` XDR blob so the footprint decoder
    /// is exercised against genuine XDR rather than a hand-rolled fixture.
    fn transaction_data_xdr(read_only: u32, read_write: u32) -> String {
        let key = |seed: u8| {
            LedgerKey::ContractCode(LedgerKeyContractCode {
                hash: Hash([seed; 32]),
            })
        };
        let data = SorobanTransactionData {
            ext: ExtensionPoint::V0,
            resources: SorobanResources {
                footprint: LedgerFootprint {
                    read_only: (0..read_only)
                        .map(|i| key(i as u8))
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap(),
                    read_write: (0..read_write)
                        .map(|i| key(100 + i as u8))
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap(),
                },
                instructions: 1_234_567,
                read_bytes: 4_096,
                write_bytes: 512,
            },
            resource_fee: 58_181,
        };
        BASE64.encode(data.to_xdr(Limits::none()).unwrap())
    }

    fn full_response() -> Value {
        json!({
            "latestLedger": 1_234_567u64,
            "minResourceFee": "58181",
            "cost": { "cpuInsns": "1274180", "memBytes": "1275072" },
            "transactionData": transaction_data_xdr(2, 1),
            "events": [],
        })
    }

    // ── Primary flow ────────────────────────────────────────────────────────

    #[test]
    fn parses_cpu_memory_footprint_and_min_fee() {
        let res = parse_simulation_resources(&full_response()).unwrap();

        assert_eq!(res.min_resource_fee_stroops, 58_181);
        assert_eq!(res.cpu_instructions, Some(1_274_180));
        assert_eq!(res.memory_bytes, Some(1_275_072));
        assert_eq!(res.latest_ledger, Some(1_234_567));

        let fp = res.footprint.expect("footprint decoded");
        assert_eq!(fp.read_only_entries, 2);
        assert_eq!(fp.read_write_entries, 1);
        assert_eq!(fp.total_entries(), 3);
        assert_eq!(fp.read_bytes, 4_096);
        assert_eq!(fp.write_bytes, 512);
        assert_eq!(fp.instructions, 1_234_567);
        assert_eq!(fp.resource_fee_stroops, 58_181);
    }

    #[test]
    fn plans_fee_with_default_margin() {
        let res = parse_simulation_resources(&full_response()).unwrap();
        let plan = plan_fee(
            &res,
            DEFAULT_FEE_MARGIN_PERCENT,
            DEFAULT_INCLUSION_FEE_STROOPS,
        )
        .unwrap();

        assert_eq!(plan.min_resource_fee_stroops, 58_181);
        assert_eq!(plan.margin_stroops, 58_181 * 20 / 100);
        assert_eq!(
            plan.recommended_fee_stroops,
            58_181 + (58_181 * 20 / 100) + 100
        );
        assert!(plan.recommended_fee_xlm() > 0.0);
    }

    #[test]
    fn unwraps_jsonrpc_envelope() {
        let envelope = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": full_response(),
        });
        let res = parse_simulation_resources(&envelope).unwrap();
        assert_eq!(res.min_resource_fee_stroops, 58_181);
    }

    #[test]
    fn accepts_numeric_min_resource_fee() {
        let res = parse_simulation_resources(&json!({
            "minResourceFee": 42u64,
            "transactionData": transaction_data_xdr(1, 0),
        }))
        .unwrap();
        assert_eq!(res.min_resource_fee_stroops, 42);
    }

    #[test]
    fn parses_and_reports_json() {
        let res = parse_simulation_resources(&full_response()).unwrap();
        let plan = plan_fee(&res, 10, 100).unwrap();
        let doc = report_json(&res, &plan);
        assert_eq!(doc["plan"]["margin_percent"], 10);
        assert_eq!(doc["resources"]["min_resource_fee_stroops"], 58_181);
    }

    #[test]
    fn parses_from_raw_string() {
        let raw = serde_json::to_string(&full_response()).unwrap();
        assert_eq!(
            parse_simulation_response_str(&raw)
                .unwrap()
                .min_resource_fee_stroops,
            58_181
        );
    }

    // ── Boundary cases ──────────────────────────────────────────────────────

    #[test]
    fn zero_margin_and_zero_fee_produce_inclusion_fee_only() {
        let res = SimulationResources {
            min_resource_fee_stroops: 0,
            ..Default::default()
        };
        let plan = plan_fee(&res, 0, DEFAULT_INCLUSION_FEE_STROOPS).unwrap();
        assert_eq!(plan.margin_stroops, 0);
        assert_eq!(plan.recommended_fee_stroops, DEFAULT_INCLUSION_FEE_STROOPS);
    }

    #[test]
    fn max_margin_is_accepted_and_one_over_is_rejected() {
        let res = SimulationResources {
            min_resource_fee_stroops: 100,
            ..Default::default()
        };
        assert!(plan_fee(&res, MAX_FEE_MARGIN_PERCENT, 0).is_ok());
        assert_eq!(
            plan_fee(&res, MAX_FEE_MARGIN_PERCENT + 1, 0).unwrap_err(),
            SimulationResourceError::InvalidMargin(MAX_FEE_MARGIN_PERCENT + 1)
        );
    }

    #[test]
    fn saturated_fee_overflow_is_reported_not_wrapped() {
        let res = SimulationResources {
            min_resource_fee_stroops: u64::MAX,
            ..Default::default()
        };
        assert_eq!(
            plan_fee(&res, 100, 0).unwrap_err(),
            SimulationResourceError::FeeOverflow
        );
    }

    #[test]
    fn missing_cost_object_falls_back_to_transaction_data() {
        let res = parse_simulation_resources(&json!({
            "minResourceFee": "1000",
            "transactionData": transaction_data_xdr(0, 1),
        }))
        .unwrap();

        assert_eq!(res.cpu_instructions, Some(1_234_567));
        assert_eq!(res.memory_bytes, None);
        assert!(res.warnings.iter().any(|w| w.contains("cost.memBytes")));
    }

    #[test]
    fn absent_transaction_data_yields_no_footprint_but_still_plans() {
        let res = parse_simulation_resources(&json!({ "minResourceFee": "700" })).unwrap();
        assert!(res.footprint.is_none());
        assert_eq!(res.cpu_instructions, None);
        assert!(res.warnings.iter().any(|w| w.contains("transactionData")));
        assert_eq!(plan_fee(&res, 0, 0).unwrap().recommended_fee_stroops, 700);
    }

    #[test]
    fn restore_preamble_is_added_to_the_plan() {
        let res = parse_simulation_resources(&json!({
            "minResourceFee": "1000",
            "restorePreamble": { "minResourceFee": "250", "transactionData": "" },
        }))
        .unwrap();

        assert!(res.requires_restore());
        assert!(res.restore_transaction_data.is_none()); // empty string is ignored
        let plan = plan_fee(&res, 0, 0).unwrap();
        assert_eq!(plan.restore_fee_stroops, 250);
        assert_eq!(plan.recommended_fee_stroops, 1_250);
    }

    #[test]
    fn restore_preamble_captures_transaction_data() {
        let res = parse_simulation_resources(&json!({
            "minResourceFee": "1000",
            "restorePreamble": {
                "minResourceFee": "250",
                "transactionData": "AAAAAgAAAAA="
            },
        }))
        .unwrap();
        assert_eq!(
            res.restore_transaction_data.as_deref(),
            Some("AAAAAgAAAAA=")
        );
    }

    #[test]
    fn empty_footprint_decodes_to_zero_entries() {
        let res = parse_simulation_resources(&json!({
            "minResourceFee": "1",
            "transactionData": transaction_data_xdr(0, 0),
        }))
        .unwrap();
        let fp = res.footprint.unwrap();
        assert_eq!(fp.total_entries(), 0);
    }

    // ── Failure cases ───────────────────────────────────────────────────────

    #[test]
    fn rejects_non_object_payloads() {
        assert_eq!(
            parse_simulation_resources(&json!([1, 2, 3])).unwrap_err(),
            SimulationResourceError::NotAnObject
        );
        assert_eq!(
            parse_simulation_resources(&Value::Null).unwrap_err(),
            SimulationResourceError::NotAnObject
        );
    }

    #[test]
    fn surfaces_host_simulation_errors() {
        let err = parse_simulation_resources(&json!({
            "error": "HostError: Error(Contract, #3)",
            "minResourceFee": "0",
        }))
        .unwrap_err();
        assert!(
            matches!(err, SimulationResourceError::SimulationFailed(msg) if msg.contains("HostError"))
        );
    }

    #[test]
    fn surfaces_jsonrpc_transport_errors() {
        let err = parse_simulation_resources(&json!({
            "jsonrpc": "2.0",
            "error": { "code": -32602, "message": "invalid transaction" },
        }))
        .unwrap_err();
        assert_eq!(
            err,
            SimulationResourceError::RpcError("invalid transaction".to_string())
        );
    }

    #[test]
    fn rejects_responses_without_resource_accounting() {
        let err = parse_simulation_resources(&json!({ "status": "ok" })).unwrap_err();
        assert!(matches!(
            err,
            SimulationResourceError::UnsupportedResponse(_)
        ));
    }

    #[test]
    fn rejects_malformed_numeric_fields() {
        for bad in [
            json!("not-a-number"),
            json!(-1),
            json!(1.5),
            json!(""),
            json!(null),
            json!({}),
        ] {
            let err = parse_simulation_resources(&json!({ "minResourceFee": bad })).unwrap_err();
            assert!(
                matches!(&err, SimulationResourceError::InvalidField { field, .. } if field == "minResourceFee"),
                "expected InvalidField for {:?}, got {:?}",
                bad,
                err
            );
        }
    }

    #[test]
    fn rejects_undecodable_transaction_data() {
        let err = parse_simulation_resources(&json!({
            "minResourceFee": "1",
            "transactionData": "!!!not base64!!!",
        }))
        .unwrap_err();
        assert!(
            matches!(&err, SimulationResourceError::InvalidField { field, reason }
            if field == "transactionData" && reason.contains("base64"))
        );

        let err = parse_simulation_resources(&json!({
            "minResourceFee": "1",
            "transactionData": BASE64.encode([0xffu8; 8]),
        }))
        .unwrap_err();
        assert!(
            matches!(&err, SimulationResourceError::InvalidField { field, .. }
            if field == "transactionData")
        );
    }

    #[test]
    fn rejects_oversized_documents() {
        let raw = format!("{{\"pad\":\"{}\"}}", "a".repeat(MAX_RESPONSE_BYTES));
        assert!(matches!(
            parse_simulation_response_str(&raw).unwrap_err(),
            SimulationResourceError::ResponseTooLarge { .. }
        ));
    }

    #[test]
    fn rejects_invalid_json_text() {
        assert!(matches!(
            parse_simulation_response_str("{not json").unwrap_err(),
            SimulationResourceError::InvalidField { .. }
        ));
    }

    #[test]
    fn rejects_ledger_sequence_above_u32() {
        let err = parse_simulation_resources(&json!({
            "minResourceFee": "1",
            "latestLedger": u64::MAX,
        }))
        .unwrap_err();
        assert!(
            matches!(&err, SimulationResourceError::InvalidField { field, .. }
            if field == "latestLedger")
        );
    }

    #[test]
    fn restore_preamble_without_fee_is_rejected() {
        let err = parse_simulation_resources(&json!({
            "minResourceFee": "1",
            "restorePreamble": { "transactionData": "" },
        }))
        .unwrap_err();
        assert_eq!(
            err,
            SimulationResourceError::MissingField("restorePreamble.minResourceFee".to_string())
        );
    }

    #[test]
    fn formats_thousands_separators() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1_000), "1,000");
        assert_eq!(format_thousands(1_274_180), "1,274,180");
    }

    #[test]
    fn retrieves_built_in_profiles() {
        assert!(get_profile("ci-smoke").is_ok());
        assert!(get_profile("ci-full").is_ok());
        assert!(get_profile("dev-fast").is_ok());
        assert!(get_profile("CI-SMOKE").is_ok()); // case-insensitive

        let err = get_profile("nonexistent-profile").unwrap_err();
        assert!(matches!(
            err,
            SimulationResourceError::UnknownProfile { .. }
        ));
        assert!(err.to_string().contains("unknown simulation profile"));
        assert!(err.to_string().contains("ci-smoke"));
    }

    #[test]
    fn validates_simulation_against_profile_ceilings() {
        let profile = get_profile("ci-smoke").unwrap();
        let resources = SimulationResources {
            min_resource_fee_stroops: 50_000,
            cpu_instructions: Some(1_000_000),
            memory_bytes: Some(1_000_000),
            footprint: Some(SimulationFootprint {
                read_only_entries: 2,
                read_write_entries: 2,
                read_bytes: 10_000,
                write_bytes: 2_000,
                instructions: 1_000_000,
                resource_fee_stroops: 50_000,
            }),
            latest_ledger: Some(100),
            restore_fee_stroops: None,
            restore_transaction_data: None,
            warnings: Vec::new(),
        };
        let plan = plan_fee(&resources, 20, 100).unwrap();
        let violations = validate_against_profile(&resources, &plan, &profile);
        assert!(violations.is_empty());

        // Test violations when CPU and Memory breach ceiling
        let heavy_resources = SimulationResources {
            min_resource_fee_stroops: 500_000,
            cpu_instructions: Some(10_000_000), // > 2M
            memory_bytes: Some(10_000_000),     // > 2MB
            footprint: Some(SimulationFootprint {
                read_only_entries: 10,
                read_write_entries: 10, // 20 entries > 10
                read_bytes: 50_000,     // > 32KB
                write_bytes: 20_000,    // > 8KB
                instructions: 10_000_000,
                resource_fee_stroops: 500_000,
            }),
            latest_ledger: Some(100),
            restore_fee_stroops: None,
            restore_transaction_data: None,
            warnings: Vec::new(),
        };
        let heavy_plan = plan_fee(&heavy_resources, 20, 100).unwrap();
        let heavy_violations = validate_against_profile(&heavy_resources, &heavy_plan, &profile);
        assert!(!heavy_violations.is_empty());
        assert!(heavy_violations
            .iter()
            .any(|v| v.metric == "cpu_instructions"));
        assert!(heavy_violations.iter().any(|v| v.metric == "memory_bytes"));
        assert!(heavy_violations
            .iter()
            .any(|v| v.metric == "footprint_entries"));
        assert!(heavy_violations.iter().any(|v| v.metric == "read_bytes"));
        assert!(heavy_violations.iter().any(|v| v.metric == "write_bytes"));
        assert!(heavy_violations
            .iter()
            .any(|v| v.metric == "recommended_fee_stroops"));
    }

    #[test]
    fn report_json_with_profile_includes_profile_eval() {
        let profile = get_profile("ci-smoke").unwrap();
        let res = SimulationResources::default();
        let plan = plan_fee(&res, 20, 100).unwrap();
        let violations = validate_against_profile(&res, &plan, &profile);
        let doc = report_json_with_profile(&res, &plan, Some((&profile, &violations)));
        assert_eq!(doc["profile"]["name"], "ci-smoke");
        assert_eq!(doc["profile"]["passed"], true);
    }
}
