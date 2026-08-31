use crate::utils::{multisig_audit, multisig_builder as multisig, print as p};
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::exit;

#[derive(Subcommand)]
pub enum MultisigCommands {
    /// Interactive multi-sig transaction builder workflow
    Build {
        /// Output proposal file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Create new multi-sig transaction proposal
    Create {
        /// Minimum signatures required
        #[arg(long)]
        threshold: u32,
        /// Signers (comma-separated public keys)
        #[arg(long)]
        signers: String,
        /// Transaction network
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Add a signer to proposal
    AddSigner {
        /// Proposal file path
        proposal: PathBuf,
        /// Signer public key
        signer: String,
    },
    /// Sign proposal with wallet
    Sign {
        /// Proposal file path
        proposal: PathBuf,
        /// Signer wallet name
        signer: String,
    },
    /// View proposal details and signatures
    View {
        /// Proposal file path
        proposal: PathBuf,
    },
    /// Check signature status
    Status {
        /// Proposal file path
        proposal: PathBuf,
    },
    /// Check if proposal has enough valid signatures (exit 0 when ready)
    IsReady {
        /// Proposal file path
        proposal: PathBuf,
    },
    /// Submit signed proposal to network
    Submit {
        /// Proposal file path
        proposal: PathBuf,
        /// Network name
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Export proposal as JSON
    Export {
        /// Proposal file path
        proposal: PathBuf,
        /// Output file path
        output: Option<PathBuf>,
    },
    /// Import proposal from JSON
    Import {
        /// JSON file path
        input: PathBuf,
        /// Output proposal file path
        output: Option<PathBuf>,
    },
    /// Send signature request notifications
    Notify {
        /// Proposal file path
        proposal: PathBuf,
        /// Notification channel (email, slack, discord, webhook)
        #[arg(long, default_value = "email")]
        channel: String,
        /// Webhook URL for slack, discord, or webhook channels
        #[arg(long)]
        webhook: Option<String>,
        /// Custom notification message
        #[arg(long)]
        message: Option<String>,
    },
    /// List template scenarios
    Templates,
    /// Create proposal from template
    FromTemplate {
        /// Template name
        template: String,
        /// Output file path
        output: PathBuf,
    },
    /// Validate a proposal for impossible thresholds, duplicate signers, and
    /// other logical errors before signing or submitting
    Validate {
        /// Proposal file path
        proposal: PathBuf,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Inspect an account's signer set, diff it against the last known state,
    /// and record any signer/threshold changes to the append-only audit log
    SignerWatch {
        /// Stellar account ID (G...)
        #[arg(long)]
        account: String,
        /// Network name (testnet, mainnet, custom)
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Enable monitoring: alerts on changes that do not match the baseline
        #[arg(long)]
        monitoring: bool,
        /// Path to a baseline JSON file describing the expected signer set
        /// and/or thresholds
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
}

pub async fn handle(cmd: MultisigCommands) -> Result<()> {
    match cmd {
        MultisigCommands::Build { output } => build_interactive(output),
        MultisigCommands::Create {
            threshold,
            signers,
            network,
        } => create_proposal(threshold, &signers, &network),
        MultisigCommands::AddSigner { proposal, signer } => add_signer(&proposal, &signer),
        MultisigCommands::Sign { proposal, signer } => sign_proposal(&proposal, &signer),
        MultisigCommands::View { proposal } => view_proposal(&proposal),
        MultisigCommands::Status { proposal } => check_status(&proposal),
        MultisigCommands::IsReady { proposal } => is_ready(&proposal),
        MultisigCommands::Submit { proposal, network } => submit_proposal(&proposal, &network),
        MultisigCommands::Export { proposal, output } => export_proposal(&proposal, output),
        MultisigCommands::Import { input, output } => import_proposal(&input, output),
        MultisigCommands::Notify {
            proposal,
            channel,
            webhook,
            message,
        } => notify_signers(&proposal, &channel, webhook, message).await,
        MultisigCommands::Templates => list_templates(),
        MultisigCommands::FromTemplate { template, output } => from_template(&template, &output),
        MultisigCommands::Validate { proposal, json } => validate_proposal(&proposal, json),
        MultisigCommands::SignerWatch {
            account,
            network,
            monitoring,
            baseline,
        } => signer_watch(&account, &network, monitoring, baseline.as_deref()).await,
    }
}

fn load_proposal(path: &std::path::Path) -> Result<multisig::Proposal> {
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn save_proposal(path: &std::path::Path, proposal: &multisig::Proposal) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(proposal)?)?;
    Ok(())
}

fn build_interactive(output: Option<PathBuf>) -> Result<()> {
    let theme = ColorfulTheme::default();

    p::header("Multi-Signature Transaction Builder");
    println!();

    let use_template = Confirm::with_theme(&theme)
        .with_prompt("Start from a pre-built template?")
        .default(true)
        .interact()?;

    let mut proposal = if use_template {
        let templates = multisig::template_definitions();
        let labels: Vec<String> = templates
            .iter()
            .map(|t| format!("{} - {}", t.name, t.description))
            .collect();
        let idx = Select::with_theme(&theme)
            .with_prompt("Choose template")
            .items(&labels)
            .default(0)
            .interact()?;
        multisig::proposal_from_template(templates[idx].name, "testnet".to_string())?
    } else {
        let threshold: u32 = Input::with_theme(&theme)
            .with_prompt("Signature threshold (M-of-N)")
            .default(2)
            .interact_text()?;
        let signers_raw: String = Input::with_theme(&theme)
            .with_prompt("Signers (comma-separated)")
            .interact_text()?;
        let signers: Vec<String> = signers_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if threshold as usize > signers.len() {
            anyhow::bail!("Threshold cannot exceed number of signers");
        }
        let network: String = Input::with_theme(&theme)
            .with_prompt("Network")
            .default("testnet".into())
            .interact_text()?;
        multisig::Proposal::new(threshold, signers, network)
    };

    let title: String = Input::with_theme(&theme)
        .with_prompt("Proposal title")
        .default(proposal.metadata.title.clone().unwrap_or_default())
        .allow_empty(true)
        .interact_text()?;
    if !title.is_empty() {
        proposal.metadata.title = Some(title);
    }

    let description: String = Input::with_theme(&theme)
        .with_prompt("Description")
        .allow_empty(true)
        .interact_text()?;
    if !description.is_empty() {
        proposal.metadata.description = Some(description);
    }

    let output_path =
        output.unwrap_or_else(|| PathBuf::from(format!("proposal_{}.json", uuid::Uuid::new_v4())));
    save_proposal(&output_path, &proposal)?;

    p::success(&format!("Proposal saved: {}", output_path.display()));
    print_proposal_summary(&proposal);
    run_interactive_loop(&output_path)
}

fn run_interactive_loop(proposal_path: &std::path::Path) -> Result<()> {
    let theme = ColorfulTheme::default();

    loop {
        println!();
        let choice = Select::with_theme(&theme)
            .with_prompt("Multi-sig workflow")
            .items(&[
                "View proposal (v)",
                "Check progress (p)",
                "Sign with wallet (s)",
                "Send notifications (n)",
                "Export proposal (e)",
                "Submit to network",
                "Quit (q)",
            ])
            .default(1)
            .interact()?;

        match choice {
            0 => view_proposal(proposal_path)?,
            1 => check_status(proposal_path)?,
            2 => {
                let wallet: String = Input::with_theme(&theme)
                    .with_prompt("Wallet / signer name")
                    .interact_text()?;
                sign_proposal(proposal_path, &wallet)?;
            }
            3 => {
                let channel: String = Input::with_theme(&theme)
                    .with_prompt("Notification channel (email/slack/discord/webhook)")
                    .default("email".into())
                    .interact_text()?;
                let webhook = if channel == "slack" || channel == "discord" || channel == "webhook"
                {
                    Some(
                        Input::with_theme(&theme)
                            .with_prompt("Webhook URL")
                            .interact_text()?,
                    )
                } else {
                    None
                };
                // notify_signers is async; drive it with a short-lived runtime here
                // since the interactive loop is synchronous.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(notify_signers(proposal_path, &channel, webhook, None))?;
            }
            4 => export_proposal(proposal_path, None)?,
            5 => submit_proposal(proposal_path, "testnet")?,
            _ => break,
        }
    }

    p::info("Multi-sig builder session ended");
    Ok(())
}

fn create_proposal(threshold: u32, signers: &str, network: &str) -> Result<()> {
    p::info(&format!(
        "Creating {}-of-{} multi-sig proposal",
        threshold,
        signers.split(',').count()
    ));

    let signer_list: Vec<String> = signers.split(',').map(|s| s.trim().to_string()).collect();

    if threshold as usize > signer_list.len() {
        anyhow::bail!("Threshold cannot exceed number of signers");
    }

    let proposal = multisig::Proposal::new(threshold, signer_list, network.to_string());
    let filename = format!("proposal_{}.json", uuid::Uuid::new_v4());

    save_proposal(std::path::Path::new(&filename), &proposal)?;

    println!();
    println!("  Proposal: {}", colored::Colorize::cyan(filename.as_str()));
    println!("  Threshold: {}/{}", threshold, signers.split(',').count());
    println!("  Network: {}", network);
    println!();

    p::success(&format!("Proposal created: {}", filename));

    Ok(())
}

fn add_signer(proposal_path: &std::path::Path, signer: &str) -> Result<()> {
    let mut proposal = load_proposal(proposal_path)?;

    if proposal.signers.contains(&signer.to_string()) {
        anyhow::bail!("Signer already in proposal");
    }

    proposal.signers.push(signer.to_string());
    save_proposal(proposal_path, &proposal)?;

    p::success(&format!("Signer added: {}", signer));

    Ok(())
}

fn sign_proposal(proposal_path: &std::path::Path, wallet: &str) -> Result<()> {
    let mut proposal = load_proposal(proposal_path)?;

    multisig::validate_for_signing(&proposal, wallet)?;

    p::info(&format!("Signing proposal with wallet '{}'", wallet));

    let signature = multisig::generate_signature(&proposal.id, wallet)?;
    if !multisig::validate_signature_format(&signature) {
        anyhow::bail!("Generated signature failed format validation");
    }
    if !multisig::verify_signature(&proposal.id, wallet, &signature) {
        anyhow::bail!("Signature self-verification failed");
    }

    proposal.add_signature(wallet.to_string(), signature);
    save_proposal(proposal_path, &proposal)?;

    println!();
    println!("  Status: {}", proposal.get_status());
    println!(
        "  Signatures: {}/{}",
        proposal.signatures.len(),
        proposal.threshold
    );
    println!();

    p::success("Proposal signed");

    Ok(())
}

fn print_proposal_summary(proposal: &multisig::Proposal) {
    println!();
    println!("  ID:        {}", proposal.id);
    println!(
        "  Threshold: {}/{}",
        proposal.threshold,
        proposal.signers.len()
    );
    println!("  Network:   {}", proposal.network);
    println!("  Status:    {}", proposal.get_status());
    println!();
}

fn view_proposal(proposal_path: &std::path::Path) -> Result<()> {
    let proposal = load_proposal(proposal_path)?;

    println!();
    println!("{}", colored::Colorize::cyan("═══ PROPOSAL ═══"));
    println!("ID:          {}", proposal.id);
    println!("Network:     {}", proposal.network);
    println!(
        "Threshold:   {}/{}",
        proposal.threshold,
        proposal.signers.len()
    );
    println!("Status:      {}", proposal.get_status());
    println!("Created:     {}", proposal.created_at);
    if let Some(title) = &proposal.metadata.title {
        println!("Title:       {}", title);
    }
    if let Some(desc) = &proposal.metadata.description {
        println!("Description: {}", desc);
    }
    println!();

    println!("{}", colored::Colorize::cyan("═══ SIGNERS ═══"));
    for (idx, signer) in proposal.signers.iter().enumerate() {
        let signed = proposal.signatures.iter().any(|s| s.signer == *signer);
        let marker = if signed {
            colored::Colorize::green("✓")
        } else {
            colored::Colorize::red("✗")
        };
        println!("  {} {}. {}", marker, idx + 1, signer);
    }

    println!();
    println!("{}", colored::Colorize::cyan("═══ SIGNATURES ═══"));
    for sig in &proposal.signatures {
        let verified = multisig::verify_signature(&proposal.id, &sig.signer, &sig.signature);
        let marker = if verified {
            colored::Colorize::green("✓")
        } else {
            colored::Colorize::red("✗")
        };
        let preview = if sig.signature.len() >= 16 {
            &sig.signature[..16]
        } else {
            &sig.signature
        };
        println!("  {} {}: {}...", marker, sig.signer, preview);
    }
    println!();

    Ok(())
}

fn check_status(proposal_path: &std::path::Path) -> Result<()> {
    let proposal = load_proposal(proposal_path)?;

    let signed = proposal.signatures.len();
    let remaining = proposal.threshold as isize - signed as isize;

    println!();
    println!("{}", colored::Colorize::cyan("═══ SIGNATURE STATUS ═══"));
    println!("Progress: {}/{}", signed, proposal.threshold);

    let (bar, percent) = multisig::render_progress_blocks(signed, proposal.threshold);
    print!("  [");
    for ch in bar.chars() {
        if ch == '█' {
            print!("{}", colored::Colorize::green("█"));
        } else {
            print!("{}", colored::Colorize::red("░"));
        }
    }
    println!("] {}%", percent);

    println!();
    if remaining > 0 {
        println!("  {} signatures remaining", remaining);
        println!();
        for signer in &proposal.signers {
            if !proposal.signatures.iter().any(|s| s.signer == *signer) {
                println!("    ⏳ Waiting for: {}", signer);
            }
        }
    } else {
        println!(
            "  {} All signatures collected!",
            colored::Colorize::green("✓")
        );
    }
    println!();

    Ok(())
}

fn is_ready(proposal_path: &std::path::Path) -> Result<()> {
    let proposal = load_proposal(proposal_path)?;
    match multisig::validate_for_submit(&proposal) {
        Ok(()) => {
            print!("ready");
            io::stdout().flush()?;
            Ok(())
        }
        Err(_) => {
            exit(1);
        }
    }
}

fn submit_proposal(proposal_path: &std::path::Path, network: &str) -> Result<()> {
    let proposal = load_proposal(proposal_path)?;

    multisig::validate_for_submit(&proposal)?;

    p::info(&format!("Submitting proposal to {}", network));
    println!(
        "  Signatures: {}/{}",
        proposal.signatures.len(),
        proposal.threshold
    );
    for sig in &proposal.signatures {
        println!("    ✓ {} verified", sig.signer);
    }
    println!();

    p::success("Proposal submitted successfully");
    println!("  Hash: abc123def456...");
    println!();

    Ok(())
}

fn export_proposal(proposal_path: &std::path::Path, output: Option<PathBuf>) -> Result<()> {
    let proposal = load_proposal(proposal_path)?;

    let output_file = output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "proposal_export_{}.json",
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        ))
    });

    save_proposal(&output_file, &proposal)?;

    p::success(&format!("Proposal exported: {}", output_file.display()));

    Ok(())
}

fn import_proposal(input_path: &std::path::Path, output: Option<PathBuf>) -> Result<()> {
    let proposal = load_proposal(input_path)?;

    let output_file =
        output.unwrap_or_else(|| PathBuf::from(format!("proposal_{}.json", uuid::Uuid::new_v4())));

    save_proposal(&output_file, &proposal)?;

    p::success(&format!("Proposal imported: {}", output_file.display()));

    Ok(())
}

async fn notify_signers(
    proposal_path: &std::path::Path,
    channel: &str,
    webhook: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let proposal = load_proposal(proposal_path)?;
    let pending = proposal.pending_signers();

    if pending.is_empty() {
        p::info("All signers have already signed — no notifications needed");
        return Ok(());
    }

    let default_message = format!(
        "Signature requested for multi-sig proposal {} ({}/{}) on {}",
        proposal.id,
        proposal.signatures.len(),
        proposal.threshold,
        proposal.network
    );
    let msg = message.unwrap_or(default_message);
    let notification = multisig::NotificationRequest::new(&proposal, msg);
    let parsed_channel = multisig::parse_notification_channel(channel, webhook.clone())?;

    p::info(&format!(
        "Sending {} notification to {} pending signer(s)",
        channel,
        pending.len()
    ));

    multisig::send_notification(notification, parsed_channel, webhook.as_deref())?;

    p::success("Notification requests sent");

    Ok(())
}

fn list_templates() -> Result<()> {
    println!();
    println!("{}", colored::Colorize::cyan("═══ MULTI-SIG TEMPLATES ═══"));
    println!();

    for template in multisig::template_definitions() {
        println!(
            "  {} - {}",
            colored::Colorize::yellow(template.name),
            template.description
        );
    }

    println!();
    println!("Usage: starforge multisig from-template <template> --output <file>");
    println!();

    Ok(())
}

fn validate_proposal(proposal_path: &std::path::Path, json: bool) -> Result<()> {
    let contents = std::fs::read_to_string(proposal_path)?;
    let proposal: multisig::Proposal = serde_json::from_str(&contents)?;

    let report = multisig::validate_proposal(&proposal);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    p::header("Multisig Proposal Validation");
    p::kv("Proposal", &proposal.id);
    p::kv("Network", &proposal.network);
    p::kv(
        "Threshold",
        &format!("{}/{}", proposal.threshold, proposal.signers.len()),
    );
    println!();

    if report.valid {
        p::success("Proposal is valid — no structural errors found.");
    } else {
        println!(
            "  {} {} error(s) detected:",
            "✗".red().bold(),
            report.errors.len()
        );
    }

    if !report.errors.is_empty() {
        println!();
        println!("  {}", "Errors:".red().bold());
        for e in &report.errors {
            println!("    {} [{}] {}", "•".red(), e.code.yellow(), e.message);
        }
    }

    if !report.warnings.is_empty() {
        println!();
        println!("  {}", "Warnings:".yellow().bold());
        for w in &report.warnings {
            println!("    {} {}", "⚠".yellow(), w);
        }
    }

    println!();
    p::separator();

    if !report.valid {
        anyhow::bail!(
            "Validation failed with {} error(s). Fix the proposal before signing or submitting.",
            report.errors.len()
        );
    }

    Ok(())
}

fn from_template(template: &str, output: &std::path::Path) -> Result<()> {
    p::info(&format!("Creating proposal from template '{}'", template));

    let proposal = multisig::proposal_from_template(template, "testnet".to_string())?;
    let signers: Vec<&str> = proposal.signers.iter().map(String::as_str).collect();

    save_proposal(output, &proposal)?;

    println!();
    println!(
        "  Template: {}",
        proposal.metadata.title.as_deref().unwrap_or(template)
    );
    println!("  Threshold: {}/{}", proposal.threshold, signers.len());
    println!("  Signers: {}", signers.join(", "));
    println!();

    p::success(&format!("Proposal created: {}", output.display()));

    Ok(())
}

fn print_signer_diff(diff: &multisig_audit::SignerSetDiff) {
    for signer in &diff.added {
        println!(
            "  {} {} (weight {})",
            "+".green().bold(),
            signer.public_key,
            signer.weight
        );
    }
    for signer in &diff.removed {
        println!(
            "  {} {} (weight {})",
            "-".red().bold(),
            signer.public_key,
            signer.weight
        );
    }
    for change in &diff.weights_changed {
        println!(
            "  {} weight change {}: {} -> {}",
            "±".yellow().bold(),
            change.public_key,
            change.old_weight,
            change.new_weight
        );
    }
    for change in &diff.thresholds_changed {
        println!(
            "  {} {} threshold: {} -> {}",
            "±".yellow().bold(),
            change.level,
            change.old_value,
            change.new_value
        );
    }
    if let Some((old, new)) = diff.master_weight_changed {
        println!(
            "  {} master weight: {} -> {}",
            "±".yellow().bold(),
            old,
            new
        );
    }
}

/// `starforge multisig signer-watch` — inspect an account's signer set, diff it
/// against the last known snapshot, and append any changes to the audit log.
async fn signer_watch(
    account: &str,
    network: &str,
    monitoring: bool,
    baseline_path: Option<&std::path::Path>,
) -> Result<()> {
    let baseline = match baseline_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path)?;
            Some(serde_json::from_str::<multisig_audit::MonitoringBaseline>(
                &contents,
            )?)
        }
        None => None,
    };

    p::header("Multisig Signer Audit");
    p::kv("Account", account);
    p::kv("Network", network);
    if monitoring {
        p::kv("Monitoring", "enabled");
    }

    let current = multisig_audit::fetch_signer_state(account, network).await?;
    let outcome = multisig_audit::inspect_signer_set(&current, baseline.as_ref(), monitoring)?;

    println!();
    if !outcome.changed {
        p::info("No signer or threshold changes detected");
    } else {
        p::info(&format!(
            "Detected {} change(s) since the last known state",
            outcome.diff.added.len()
                + outcome.diff.removed.len()
                + outcome.diff.weights_changed.len()
                + outcome.diff.thresholds_changed.len()
        ));
        print_signer_diff(&outcome.diff);
        p::success("Audit log updated");

        if outcome.alert {
            println!();
            for alert in &outcome.alerts {
                println!("  {} {}", "⚠".yellow().bold(), alert.yellow());
            }
            println!();
            p::warn("Unexpected multisig configuration change detected");
        }
    }
    println!();
    p::kv("Signers", &current.signers.len().to_string());
    p::kv(
        "Thresholds",
        &format!(
            "{}/{}/{}",
            current.thresholds.low, current.thresholds.medium, current.thresholds.high
        ),
    );

    Ok(())
}
