//! Optimization suggestion engine.
//!
//! Every suggestion is backed by a concrete observation in the profile (a
//! decoded host call, a section, a measured resource) and carries that
//! observation as `evidence`, plus a `basis` saying how it was established:
//!
//! * `certain` — a structural fact Soroban enforces (e.g. the VM rejects
//!   floating point) or exact arithmetic on the fee model;
//! * `measured` — derived from a simulation / measured resource usage;
//! * `static` — derived from static analysis of reachable code. Static
//!   findings describe what the code *can* do; whether a loop runs 1 or 1,000
//!   times is only visible in a measurement.
//!
//! IDs `GAS-001`..`GAS-012` belong to the older byte-level analyzer in
//! `gas_analyzer`; this engine uses `GAS-1xx`.

use super::fees;
use super::host_fns::HostCategory;
use super::GasProfileReport;
use crate::utils::gas_analyzer::FindingSeverity;
use serde::{Deserialize, Serialize};

/// How a suggestion was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    Certain,
    Measured,
    Static,
}

/// One optimization suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: String,
    pub severity: FindingSeverity,
    /// `compatibility`, `size`, `storage`, `cpu`, `events`, `auth`,
    /// `cross-contract`, `limits`, `fees`, `accuracy`.
    pub category: String,
    /// Contract function the suggestion applies to, when specific.
    pub function: Option<String>,
    pub title: String,
    pub evidence: String,
    pub recommendation: String,
    pub basis: Basis,
    /// Savings in stroops when they can be computed exactly from the fee
    /// model (e.g. upload fee saved by stripping N bytes).
    pub estimated_savings_stroops: Option<u64>,
}

fn severity_rank(s: &FindingSeverity) -> u8 {
    match s {
        FindingSeverity::Critical => 0,
        FindingSeverity::High => 1,
        FindingSeverity::Medium => 2,
        FindingSeverity::Low => 3,
        FindingSeverity::Info => 4,
    }
}

/// Parse a `--min-severity` value.
pub fn parse_severity(s: &str) -> anyhow::Result<FindingSeverity> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "critical" => FindingSeverity::Critical,
        "high" => FindingSeverity::High,
        "medium" => FindingSeverity::Medium,
        "low" => FindingSeverity::Low,
        "info" => FindingSeverity::Info,
        other => anyhow::bail!(
            "unknown severity '{}': use critical, high, medium, low or info",
            other
        ),
    })
}

/// True when `s` is at least as severe as `min`.
pub fn at_least(s: &FindingSeverity, min: &FindingSeverity) -> bool {
    severity_rank(s) <= severity_rank(min)
}

/// 0–100 score, same penalties as the byte-level analyzer.
pub fn score(suggestions: &[Suggestion]) -> u8 {
    let penalty: u32 = suggestions
        .iter()
        .map(|s| match s.severity {
            FindingSeverity::Critical => 30,
            FindingSeverity::High => 20,
            FindingSeverity::Medium => 10,
            FindingSeverity::Low => 4,
            FindingSeverity::Info => 1,
        })
        .sum();
    100u32.saturating_sub(penalty) as u8
}

struct Builder {
    out: Vec<Suggestion>,
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        id: &str,
        severity: FindingSeverity,
        category: &str,
        function: Option<&str>,
        title: String,
        evidence: String,
        recommendation: &str,
        basis: Basis,
        savings: Option<u64>,
    ) {
        self.out.push(Suggestion {
            id: id.to_string(),
            severity,
            category: category.to_string(),
            function: function.map(str::to_string),
            title,
            evidence,
            recommendation: recommendation.to_string(),
            basis,
            estimated_savings_stroops: savings,
        });
    }
}

fn names_in(map: &std::collections::BTreeMap<String, u32>, wanted: &[&str]) -> String {
    let v: Vec<String> = map
        .iter()
        .filter(|(k, _)| wanted.contains(&k.as_str()))
        .map(|(k, n)| format!("{} x{}", k, n))
        .collect();
    v.join(", ")
}

/// Run every rule against the report.
pub fn suggest(r: &GasProfileReport) -> Vec<Suggestion> {
    let mut b = Builder { out: Vec::new() };
    compatibility_rules(r, &mut b);
    size_rules(r, &mut b);
    execution_rules(r, &mut b);
    measured_rules(r, &mut b);
    b.out.sort_by(|x, y| {
        severity_rank(&x.severity)
            .cmp(&severity_rank(&y.severity))
            .then_with(|| x.id.cmp(&y.id))
            .then_with(|| x.function.cmp(&y.function))
    });
    b.out
}

fn compatibility_rules(r: &GasProfileReport, b: &mut Builder) {
    let m = &r.module;
    if m.has_start {
        b.push(
            "GAS-101",
            FindingSeverity::Critical,
            "compatibility",
            None,
            "Module has a Wasm start section".into(),
            "start section present".into(),
            "Soroban's VM rejects modules with a start function, so this contract cannot be \
             uploaded. Move initialization into an explicit contract function (or a \
             `__constructor` on protocol 22+).",
            Basis::Certain,
            None,
        );
    }
    for feature in &m.rejected_features {
        let (id, advice) = match feature.as_str() {
            "floating-point" | "saturating-float-to-int" => (
                "GAS-102",
                "Soroban's VM is configured without floating point. Replace f32/f64 with \
                 integer or fixed-point arithmetic (e.g. i128 scaled by 10^7) and check that \
                 no dependency pulls float formatting in.",
            ),
            _ => (
                "GAS-103",
                "Soroban's VM does not enable this Wasm proposal. Build with `starforge \
                 contract build` and avoid `-C target-feature` flags that enable it.",
            ),
        };
        let fns: Vec<&str> = r
            .functions
            .iter()
            .filter(|f| f.rejected_features.iter().any(|x| x == feature))
            .map(|f| f.name.as_str())
            .collect();
        let evidence = if fns.is_empty() {
            format!("`{}` instructions found in the module", feature)
        } else {
            format!(
                "`{}` instructions reachable from: {}",
                feature,
                fns.join(", ")
            )
        };
        b.push(
            id,
            FindingSeverity::Critical,
            "compatibility",
            None,
            format!("Uses the `{}` Wasm feature, which Soroban rejects", feature),
            evidence,
            advice,
            Basis::Certain,
            None,
        );
    }
    if !m.unknown_imports.is_empty() {
        b.push(
            "GAS-104",
            FindingSeverity::Critical,
            "compatibility",
            None,
            format!(
                "{} import(s) are not Soroban host functions",
                m.unknown_imports.len()
            ),
            format!("unresolvable imports: {}", m.unknown_imports.join(", ")),
            "Instantiation fails when an import cannot be linked to the host. This usually \
             means a std/`wasm-bindgen`/libc dependency leaked into the contract; build with \
             `#![no_std]` and `soroban-sdk` only.",
            Basis::Certain,
            None,
        );
    }
    if !m.has_env_meta && m.exported_functions > 0 {
        b.push(
            "GAS-105",
            FindingSeverity::High,
            "compatibility",
            None,
            "Missing `contractenvmetav0` custom section".into(),
            "no contractenvmetav0 section".into(),
            "The host refuses contracts without an interface-version meta section. Build \
             with `soroban-sdk` (it embeds the section) and make sure your optimizer or \
             strip step keeps `contractenvmetav0`, `contractmetav0` and `contractspecv0`.",
            Basis::Certain,
            None,
        );
    }
}

fn size_rules(r: &GasProfileReport, b: &mut Builder) {
    let m = &r.module;
    let fee = &r.fee_config.fees;
    if m.strippable_custom_bytes > 0 {
        let savings = fees::upload_savings(
            r.size_bytes as u64,
            m.strippable_custom_bytes as u64,
            fee,
        );
        let names: Vec<String> = m
            .custom_sections
            .iter()
            .filter(|(n, _)| !super::wasm::is_soroban_metadata_section(n))
            .map(|(n, bytes)| format!("{} ({} B)", n, bytes))
            .collect();
        let severity = if m.strippable_custom_bytes >= 4 * 1024 {
            FindingSeverity::Medium
        } else {
            FindingSeverity::Low
        };
        b.push(
            "GAS-110",
            severity,
            "size",
            None,
            format!(
                "{} bytes of non-runtime custom sections",
                m.strippable_custom_bytes
            ),
            format!("strippable sections: {}", names.join(", ")),
            "Run `stellar contract optimize` (or `wasm-opt --strip-debug --strip-producers`) \
             and set `strip = \"symbols\"` / `debug = 0` in `[profile.release]`. Keep \
             `contractspecv0`, `contractmetav0` and `contractenvmetav0`.",
            Basis::Certain,
            Some(savings),
        );
    }
    let log_fns: Vec<&str> = r
        .functions
        .iter()
        .filter(|f| f.host_functions.contains_key("log_from_linear_memory"))
        .map(|f| f.name.as_str())
        .collect();
    if !log_fns.is_empty() {
        let fns = log_fns;
        b.push(
            "GAS-111",
            FindingSeverity::Medium,
            "size",
            None,
            "Debug logging is compiled in".into(),
            format!(
                "log_from_linear_memory reachable from: {}",
                fns.join(", ")
            ),
            "`soroban_sdk::log!` only emits code when `debug_assertions` is on, so this \
             usually means a debug build is being profiled or deployed. Build with \
             `stellar contract build` (release profile) before measuring or deploying.",
            Basis::Static,
            None,
        );
    }
}

fn execution_rules(r: &GasProfileReport, b: &mut Builder) {
    for f in &r.functions {
        let name = Some(f.name.as_str());
        let reads = f.loop_sites(HostCategory::StorageRead);
        if reads > 0 {
            b.push(
                "GAS-120",
                FindingSeverity::High,
                "storage",
                name,
                format!("Storage read inside a loop in `{}`", f.name),
                format!(
                    "{} storage-read call site(s) run inside loops ({})",
                    reads,
                    names_in(
                        &f.host_functions_in_loops,
                        &["get_contract_data", "has_contract_data"]
                    )
                ),
                "Every distinct key read is a footprint read entry (plus its bytes), and each \
                 call deserializes the entry again. Read once before the loop, or keep the \
                 collection under a single key (a `Vec`/`Map`) instead of one key per item.",
                Basis::Static,
                None,
            );
        }
        let writes = f.loop_sites(HostCategory::StorageWrite);
        if writes > 0 {
            b.push(
                "GAS-121",
                FindingSeverity::High,
                "storage",
                name,
                format!("Storage write inside a loop in `{}`", f.name),
                format!(
                    "{} storage-write call site(s) run inside loops ({})",
                    writes,
                    names_in(
                        &f.host_functions_in_loops,
                        &["put_contract_data", "del_contract_data"]
                    )
                ),
                "Writes are the most expensive resource: each distinct key is a write entry \
                 (also charged as a read) plus write bytes, and the per-transaction write-entry \
                 limit caps how many iterations can succeed. Accumulate in memory and write \
                 once after the loop.",
                Basis::Static,
                None,
            );
        }
        let calls = f.loop_sites(HostCategory::CrossContract);
        if calls > 0 {
            b.push(
                "GAS-122",
                FindingSeverity::High,
                "cross-contract",
                name,
                format!("Cross-contract call inside a loop in `{}`", f.name),
                format!("{} call/try_call site(s) run inside loops", calls),
                "Each cross-contract call instantiates the callee's VM and adds its footprint. \
                 Prefer a batch entry point on the callee (one call with a Vec of inputs).",
                Basis::Static,
                None,
            );
        }
        let crypto = f.loop_sites(HostCategory::Crypto);
        if crypto > 0 {
            b.push(
                "GAS-123",
                FindingSeverity::Medium,
                "cpu",
                name,
                format!("Cryptographic host call inside a loop in `{}`", f.name),
                format!(
                    "{} crypto call site(s) run inside loops",
                    crypto
                ),
                "Hashing and signature checks are metered per byte/operation and dominate CPU \
                 when repeated. Hash a concatenated buffer once, or verify an aggregate.",
                Basis::Static,
                None,
            );
        }
        let events = f.loop_sites(HostCategory::Event);
        if events > 0 {
            b.push(
                "GAS-124",
                FindingSeverity::Medium,
                "events",
                name,
                format!("Event emitted inside a loop in `{}`", f.name),
                format!("{} contract_event site(s) run inside loops", events),
                "Event bytes are charged per KB and capped per transaction. Emit one summary \
                 event after the loop (e.g. with a Vec payload) instead of one per item.",
                Basis::Static,
                None,
            );
        }
        let auth = f.loop_sites(HostCategory::Auth);
        if auth > 0 {
            b.push(
                "GAS-125",
                FindingSeverity::Medium,
                "auth",
                name,
                format!("`require_auth` inside a loop in `{}`", f.name),
                format!("{} auth call site(s) run inside loops", auth),
                "Authorizing the same address repeatedly adds host work and auth entries. \
                 Call `require_auth` once per address before the loop.",
                Basis::Static,
                None,
            );
        }
        let ttl = f.loop_sites(HostCategory::StorageTtl);
        if ttl > 0 {
            b.push(
                "GAS-126",
                FindingSeverity::Medium,
                "storage",
                name,
                format!("TTL extension inside a loop in `{}`", f.name),
                format!("{} extend_*_ttl site(s) run inside loops", ttl),
                "Each extension touches the entry and pays rent. Extend once per entry, and \
                 use thresholds so entries are only bumped when close to expiry.",
                Basis::Static,
                None,
            );
        }
        let write_sites = f.sites(HostCategory::StorageWrite);
        if write_sites >= 6 {
            b.push(
                "GAS-127",
                FindingSeverity::Low,
                "storage",
                name,
                format!("`{}` has {} storage-write call sites", f.name, write_sites),
                format!(
                    "{} put/del_contract_data sites reachable; each distinct key costs {} + {} stroops in entry fees at the configured rates",
                    write_sites,
                    r.fee_config.fees.fee_per_write_entry,
                    r.fee_config.fees.fee_per_read_entry
                ),
                "If these fields are always updated together, store them as one \
                 `#[contracttype]` struct under one key to pay for one entry instead of many.",
                Basis::Static,
                None,
            );
        }
        if f.max_loop_depth >= 3 {
            b.push(
                "GAS-128",
                FindingSeverity::Medium,
                "cpu",
                name,
                format!("Loops nested {} deep in `{}`", f.max_loop_depth, f.name),
                format!(
                    "{} loops reachable, maximum nesting depth {}",
                    f.loops, f.max_loop_depth
                ),
                "CPU grows with the product of loop bounds. Bound input sizes, precompute \
                 lookups (e.g. a Map instead of a nested search), or move work off-chain.",
                Basis::Static,
                None,
            );
        }
        let ser = f.loop_sites(HostCategory::Serialization);
        if ser > 0 {
            b.push(
                "GAS-129",
                FindingSeverity::Low,
                "cpu",
                name,
                format!("XDR (de)serialization inside a loop in `{}`", f.name),
                format!("{} serialize/deserialize site(s) run inside loops", ser),
                "Serialize the whole collection once rather than element by element.",
                Basis::Static,
                None,
            );
        }
        if f.indirect_call_sites > 0 {
            b.push(
                "GAS-130",
                FindingSeverity::Info,
                "accuracy",
                name,
                format!("`{}` uses dynamic dispatch", f.name),
                format!(
                    "{} call_indirect site(s) reachable; their targets are not followed",
                    f.indirect_call_sites
                ),
                "Static host-call counts for this function are a lower bound. Add a \
                 simulation (`--sim`) to profile its real cost.",
                Basis::Static,
                None,
            );
        }
    }
}

fn measured_rules(r: &GasProfileReport, b: &mut Builder) {
    for m in &r.measurements {
        let name = Some(m.function.as_str());
        let ranked = m.utilization.ranked();
        if let Some((resource, pct)) = ranked.first() {
            if *pct >= 70.0 {
                let severity = if *pct >= 90.0 {
                    FindingSeverity::Critical
                } else {
                    FindingSeverity::High
                };
                b.push(
                    "GAS-140",
                    severity,
                    "limits",
                    name,
                    format!(
                        "`{}` uses {:.0}% of the per-transaction {} limit",
                        m.function, pct, resource
                    ),
                    format!("measured from {}", m.source),
                    "Close to a hard limit the transaction fails outright when inputs grow. \
                     Cap input sizes, paginate work across transactions, or reduce the \
                     dominant resource.",
                    Basis::Measured,
                    None,
                );
            }
        }
        let fee = &m.model_fee;
        if fee.total > 0 {
            let writes = fee.write_entries + fee.write_bytes;
            let reads = fee.read_entries + fee.read_bytes;
            let share = |v: u64| v as f64 / fee.total as f64 * 100.0;
            if share(writes) >= 50.0 {
                b.push(
                    "GAS-141",
                    FindingSeverity::Medium,
                    "fees",
                    name,
                    format!(
                        "Ledger writes are {:.0}% of `{}`'s resource fee",
                        share(writes),
                        m.function
                    ),
                    format!(
                        "{} write entries, {} write bytes -> {} stroops of {} (model)",
                        m.usage.write_entries, m.usage.write_bytes, writes, fee.total
                    ),
                    "Reduce the number of entries written (pack fields updated together) and \
                     their size (smaller types, avoid rewriting unchanged data).",
                    Basis::Measured,
                    None,
                );
            } else if share(reads) >= 50.0 {
                b.push(
                    "GAS-142",
                    FindingSeverity::Medium,
                    "fees",
                    name,
                    format!(
                        "Ledger reads are {:.0}% of `{}`'s resource fee",
                        share(reads),
                        m.function
                    ),
                    format!(
                        "{} read-only + {} read-write entries, {} read bytes -> {} stroops of {} (model)",
                        m.usage.read_entries,
                        m.usage.write_entries,
                        m.usage.read_bytes,
                        reads,
                        fee.total
                    ),
                    "Each footprint entry is charged. Keep small, always-needed config in \
                     instance storage (loaded with the contract), and avoid reading entries \
                     the call does not use.",
                    Basis::Measured,
                    None,
                );
            } else if share(fee.cpu) >= 50.0 {
                b.push(
                    "GAS-143",
                    FindingSeverity::Medium,
                    "cpu",
                    name,
                    format!(
                        "CPU is {:.0}% of `{}`'s resource fee",
                        share(fee.cpu),
                        m.function
                    ),
                    format!(
                        "{} instructions -> {} stroops of {} (model)",
                        m.usage.instructions, fee.cpu, fee.total
                    ),
                    "Look at the static profile for loops and crypto calls in this function; \
                     prefer SDK host types and functions (Vec/Map/Bytes, crypto) over \
                     re-implementing them in Wasm; native host code is usually metered more \
                     cheaply than the equivalent interpreted Wasm.",
                    Basis::Measured,
                    None,
                );
            }
            if share(fee.events) >= 20.0 {
                b.push(
                    "GAS-144",
                    FindingSeverity::Low,
                    "events",
                    name,
                    format!(
                        "Events are {:.0}% of `{}`'s resource fee",
                        share(fee.events),
                        m.function
                    ),
                    format!(
                        "{} event bytes -> {} stroops (model)",
                        m.usage.contract_events_bytes, fee.events
                    ),
                    "Keep topics short (Symbols, not Strings) and put bulky data in the body \
                     only when indexers need it.",
                    Basis::Measured,
                    None,
                );
            }
        }
        if m.requires_restore {
            b.push(
                "GAS-145",
                FindingSeverity::Medium,
                "storage",
                name,
                format!("`{}` touches archived entries", m.function),
                format!(
                    "simulation requires a restore transaction ({} stroops)",
                    m.restore_fee_stroops.unwrap_or(0)
                ),
                "Restore costs an extra transaction. Extend TTLs of long-lived persistent \
                 entries proactively (e.g. `extend_ttl` with a threshold on access).",
                Basis::Measured,
                None,
            );
        }
        if let Some(reported) = m.reported_min_resource_fee {
            let model = fee.total;
            if reported > 0 {
                let diff = (model as f64 - reported as f64).abs() / reported as f64 * 100.0;
                if diff > 25.0 {
                    b.push(
                        "GAS-146",
                        FindingSeverity::Info,
                        "accuracy",
                        name,
                        format!(
                            "Fee model differs from simulation by {:.0}% for `{}`",
                            diff, m.function
                        ),
                        format!(
                            "simulated minResourceFee {} vs model {} stroops ({})",
                            reported, model, r.fee_config.fees.source
                        ),
                        "The simulated fee is authoritative. Pass `--fee-config` with the \
                         target network's current fee settings to align the per-resource \
                         breakdown (rent for new entries is not modelled).",
                        Basis::Measured,
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fees::{NetworkFeeConfig, ResourceUsage};
    use super::super::measure;
    use super::super::profile_bytes;
    use super::super::wasm::builder::{op, ModuleBuilder};
    use super::*;

    fn cfg() -> NetworkFeeConfig {
        NetworkFeeConfig::default()
    }

    fn ids(r: &GasProfileReport) -> Vec<String> {
        r.suggestions.iter().map(|s| s.id.clone()).collect()
    }

    #[test]
    fn clean_contract_has_no_suggestions() {
        let wasm = ModuleBuilder::new()
            .import("l", "1")
            .func(Some("get"), [op::call(0), op::drop()].concat())
            .custom("contractenvmetav0", vec![0; 4])
            .build();
        let r = profile_bytes(&wasm, "c", "c.wasm", &cfg()).unwrap();
        assert!(r.suggestions.is_empty(), "{:?}", r.suggestions);
        assert_eq!(r.score, 100);
    }

    #[test]
    fn flags_storage_in_loop_only_for_the_right_function() {
        let wasm = ModuleBuilder::new()
            .import("l", "_")
            .func(
                Some("batch"),
                [
                    op::loop_start(),
                    op::call(0),
                    op::drop(),
                    op::i32_const_zero(),
                    op::br_if(0),
                    op::end(),
                ]
                .concat(),
            )
            .func(Some("single"), [op::call(0), op::drop()].concat())
            .custom("contractenvmetav0", vec![0; 4])
            .build();
        let r = profile_bytes(&wasm, "c", "c.wasm", &cfg()).unwrap();
        let s: Vec<_> = r.suggestions.iter().filter(|s| s.id == "GAS-121").collect();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].function.as_deref(), Some("batch"));
        assert_eq!(s[0].severity, FindingSeverity::High);
    }

    #[test]
    fn flags_compatibility_blockers() {
        let wasm = ModuleBuilder::new()
            .import("env", "abort")
            .func(Some("f"), [op::f64_const_zero(), op::drop()].concat())
            .start(1)
            .build();
        let r = profile_bytes(&wasm, "c", "c.wasm", &cfg()).unwrap();
        let ids = ids(&r);
        for id in ["GAS-101", "GAS-102", "GAS-104", "GAS-105"] {
            assert!(ids.contains(&id.to_string()), "missing {} in {:?}", id, ids);
        }
        assert!(r.has_critical());
    }

    #[test]
    fn strip_savings_are_exact() {
        let wasm = ModuleBuilder::new()
            .func(Some("f"), vec![])
            .custom("contractenvmetav0", vec![0; 4])
            .custom(".debug_info", vec![7; 8 * 1024])
            .build();
        let r = profile_bytes(&wasm, "c", "c.wasm", &cfg()).unwrap();
        let s = r.suggestions.iter().find(|s| s.id == "GAS-110").unwrap();
        let expected = fees::upload_savings(
            r.size_bytes as u64,
            r.module.strippable_custom_bytes as u64,
            &cfg().fees,
        );
        assert_eq!(s.estimated_savings_stroops, Some(expected));
        assert!(expected > 0);
        assert_eq!(s.severity, FindingSeverity::Medium);
    }

    #[test]
    fn measured_rules_use_measurements() {
        let wasm = ModuleBuilder::new()
            .func(Some("mint"), vec![])
            .custom("contractenvmetav0", vec![0; 4])
            .build();
        let r = profile_bytes(&wasm, "c", "c.wasm", &cfg()).unwrap();
        let usage = ResourceUsage {
            instructions: 95_000_000,
            write_entries: 3,
            write_bytes: 4_000,
            ..Default::default()
        };
        let m = measure::from_usage("mint", "test", usage, &cfg());
        let r = r.with_measurements(vec![m]);
        let limit = r.suggestions.iter().find(|s| s.id == "GAS-140").unwrap();
        assert_eq!(limit.severity, FindingSeverity::Critical);
        assert!(limit.title.contains("instructions"));
    }

    #[test]
    fn severity_helpers() {
        assert!(at_least(&FindingSeverity::Critical, &FindingSeverity::High));
        assert!(!at_least(&FindingSeverity::Low, &FindingSeverity::High));
        assert!(parse_severity("HIGH").is_ok());
        assert!(parse_severity("nope").is_err());
    }
}
