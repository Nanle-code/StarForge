use crate::commands::info;
use crate::utils::{config, horizon, print as p, soroban};
use anyhow::Result;

pub async fn run() -> Result<()> {
    p::header("StarForge Config Doctor");
    p::separator();

    let mut findings = Vec::new();
    let path = config::config_path();

    if !path.exists() {
        findings.push(config::DoctorFinding::pass(
            "schema",
            "no config.toml found; using built-in defaults",
        ));
    } else {
        match config::parse_config_file() {
            Ok(_) => findings.push(config::DoctorFinding::pass(
                "schema",
                format!("config.toml parses at {}", path.display()),
            )),
            Err(e) => findings.push(config::DoctorFinding::fail("schema", e.to_string())),
        }
    }

    let cfg = config::load()?;
    findings.extend(config::validate_config_integrity(&cfg));

    let network = cfg.network.clone();
    if horizon::check_network(&network).await {
        findings.push(config::DoctorFinding::pass(
            "horizon",
            format!("Horizon reachable for '{network}'"),
        ));
    } else {
        findings.push(config::DoctorFinding::fail(
            "horizon",
            format!("Horizon unreachable for '{network}'"),
        ));
    }

    match soroban::rpc_url(&network) {
        Ok(url) => {
            if soroban::check_soroban_rpc_url(&url).await {
                findings.push(config::DoctorFinding::pass(
                    "soroban",
                    format!("Soroban RPC reachable for '{network}'"),
                ));
            } else {
                findings.push(config::DoctorFinding::fail(
                    "soroban",
                    format!("Soroban RPC unreachable at {url}"),
                ));
            }
        }
        Err(e) => findings.push(config::DoctorFinding::fail("soroban", e.to_string())),
    }

    match soroban::get_protocol_version(&network).await {
        Ok(protocol_version) => findings.push(config::DoctorFinding::pass(
            "protocol",
            format!("Soroban protocol version {protocol_version} detected for '{network}'"),
        )),
        Err(e) => findings.push(config::DoctorFinding::fail(
            "protocol",
            format!("Soroban protocol version detection failed for '{network}': {e:#}"),
        )),
    }

    if let Some(cli_path) = info::detect_stellar_cli() {
        findings.push(config::DoctorFinding::pass(
            "stellar",
            format!("Stellar CLI found at {}", cli_path.display()),
        ));
    } else {
        findings.push(config::DoctorFinding::fail(
            "stellar",
            "Stellar CLI not found on PATH",
        ));
    }

    let passed = findings
        .iter()
        .filter(|f| f.status == config::DoctorStatus::Pass)
        .count();
    let failed = findings
        .iter()
        .filter(|f| f.status == config::DoctorStatus::Fail)
        .count();

    for finding in &findings {
        let marker = match finding.status {
            config::DoctorStatus::Pass => "✓",
            config::DoctorStatus::Fail => "✗",
        };
        println!("  {} {:<13} {}", marker, finding.category, finding.message);
    }

    println!();
    p::kv("Passed", &passed.to_string());
    p::kv("Failed", &failed.to_string());
    p::separator();

    if failed > 0 {
        anyhow::bail!("{failed} config doctor check(s) failed");
    }

    p::success("All config doctor checks passed.");
    Ok(())
}
