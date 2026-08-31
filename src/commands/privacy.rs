use crate::utils::{print as p, privacy};
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum PrivacyCommands {
    /// Analyze a JSON payload for privacy risks and PII exposure
    Assess { payload: String },
    /// Anonymize freeform text input
    Anonymize { text: String },
    /// Minimize a payload to a set of allowed fields
    Minimize {
        payload: String,
        fields: Vec<String>,
    },
    /// Generate a privacy report for the current assessment
    Report { payload: String },
}

pub async fn handle(cmd: PrivacyCommands) -> Result<()> {
    match cmd {
        PrivacyCommands::Assess { payload } => {
            let parsed: serde_json::Value = serde_json::from_str(&payload)?;
            let assessment = privacy::assess_privacy_impact(&parsed, "cli", true);
            p::header("Privacy Assessment");
            p::kv("Risk Level", &assessment.risk_level);
            p::kv("Risk Score", &assessment.risk_score.to_string());
            p::kv("PII Detected", &assessment.pii_detected.join(", "));
            p::kv("Compliant", &assessment.compliant.to_string());
        }
        PrivacyCommands::Anonymize { text } => {
            let anonymized = privacy::anonymize_text(&text);
            p::header("Anonymized Output");
            println!("{}", anonymized);
        }
        PrivacyCommands::Minimize { payload, fields } => {
            let parsed: serde_json::Value = serde_json::from_str(&payload)?;
            let minimized = privacy::minimize_payload(
                &parsed,
                &fields.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            println!("{}", serde_json::to_string_pretty(&minimized)?);
        }
        PrivacyCommands::Report { payload } => {
            let parsed: serde_json::Value = serde_json::from_str(&payload)?;
            let assessment = privacy::assess_privacy_impact(&parsed, "report", true);
            let consent = privacy::ConsentRecord::new("report", true);
            let report = privacy::build_privacy_report(&assessment, &consent);
            let path = privacy::persist_privacy_report(&report)?;
            p::header("Privacy Report");
            println!("{}", report);
            p::kv("Saved To", &path);
        }
    }
    Ok(())
}
