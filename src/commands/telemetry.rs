use crate::utils::{config, print as p, telemetry};
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum TelemetryCommands {
    /// Enable telemetry collections
    Enable,
    /// Disable telemetry collections
    Disable,
    /// Show current telemetry status
    Status,
    /// Show the exact last recorded telemetry payload
    Payload,
    /// Delete local telemetry data and the anonymous ID
    Reset,
}

pub async fn handle(cmd: TelemetryCommands) -> Result<()> {
    match cmd {
        TelemetryCommands::Enable => {
            telemetry::set_telemetry_enabled(true)?;
            p::success("Telemetry collections enabled.");
        }
        TelemetryCommands::Disable => {
            telemetry::set_telemetry_enabled(false)?;
            p::success("Telemetry collections disabled.");
        }
        TelemetryCommands::Status => {
            let cfg = config::load()?;
            let enabled = cfg.telemetry_enabled.unwrap_or(false);
            let env_override = std::env::var("STARFORGE_TELEMETRY").ok();

            p::header("Telemetry Status");
            p::separator();
            p::kv("Configured Enabled", &enabled.to_string());
            p::kv(
                "Effective Enabled",
                &telemetry::is_telemetry_enabled().to_string(),
            );
            if let Some(env_val) = env_override {
                p::kv("Environment Override (STARFORGE_TELEMETRY)", &env_val);
            }
            p::separator();
        }
        TelemetryCommands::Payload => {
            let payload = telemetry::show_payload()?;
            if let Some(json) = payload {
                println!("{}", json);
            } else {
                p::info("No telemetry payloads recorded yet.");
            }
        }
        TelemetryCommands::Reset => {
            telemetry::reset()?;
            p::success("Local telemetry payload and anonymous ID reset.");
        }
    }
    Ok(())
}
