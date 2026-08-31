//! Natural Language Command Interface for StarForge.
//!
//! Enables users to interact with StarForge using natural language commands
//! instead of memorizing CLI syntax. For example:
//!
//! ```text
//! starforge nl "create a new wallet named alice and fund it"
//! starforge nl "deploy my contract to testnet"
//! starforge nl "show me the balance of wallet bob"
//! ```

use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::Args;
use colored::*;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;

// ── CLI Arguments ──────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct NlArgs {
    /// The natural language command to interpret
    pub input: String,

    /// Show the parsed intent and extracted entities without executing
    #[arg(long, short = 'n', default_value = "false")]
    pub dry_run: bool,

    /// Explain what command would be executed (educational mode)
    #[arg(long, short = 'e', default_value = "false")]
    pub explain: bool,

    /// Enable interactive clarification for ambiguous commands
    #[arg(long, short = 'i', default_value = "false")]
    pub interactive: bool,

    /// Verbose output showing the translation pipeline
    #[arg(long, short = 'v', default_value = "false")]
    pub verbose: bool,
}

// ── Intent Types ───────────────────────────────────────────────────────────

/// Represents the recognized intent from natural language input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    // Wallet intents
    CreateWallet {
        name: Option<String>,
        fund: bool,
    },
    ListWallets,
    ShowWallet {
        name: Option<String>,
    },
    FundWallet {
        name: Option<String>,
    },
    RemoveWallet {
        name: Option<String>,
    },

    // Contract intents
    DeployContract {
        wallet: Option<String>,
        network: Option<String>,
    },
    InvokeContract {
        contract_id: Option<String>,
        function: Option<String>,
    },
    InspectContract {
        contract_id: Option<String>,
    },

    // Network intents
    ShowNetwork,
    SwitchNetwork {
        network: String,
    },
    AddNetwork {
        name: String,
        horizon: Option<String>,
    },

    // Node intents
    StartNode,
    StopNode,
    NodeStatus,

    // Info intents
    ShowInfo,
    ShowHelp {
        command: Option<String>,
    },

    // Diagnostic intents
    RunDoctor,
    RunDiagnostics,

    // Transaction intents
    ShowTransaction {
        hash: Option<String>,
    },

    // Ambiguous / Unknown
    Ambiguous {
        candidates: Vec<String>,
    },
    Unknown {
        input: String,
    },
}

/// Represents an extracted entity from the input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedEntities {
    pub wallet_name: Option<String>,
    pub contract_id: Option<String>,
    pub network: Option<String>,
    pub function_name: Option<String>,
    pub file_path: Option<String>,
    pub amount: Option<String>,
    pub keywords: Vec<String>,
}

// ── Pattern Definitions ────────────────────────────────────────────────────

struct Pattern {
    keywords: &'static [&'static str],
    intent_factory: fn(&ExtractedEntities) -> Intent,
    confidence: f64,
    // Not currently called from any code path in this crate. Kept rather than
    // removed since deleting it is a product decision, not a lint-scoping one.
    #[allow(dead_code)]
    explanation: &'static str,
}

/// Static pattern table — allocated once, reused across calls.
static PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| {
    vec![
        // ── Wallet patterns ─────────────────────────────────────────
        Pattern {
            keywords: &["create", "wallet"],
            intent_factory: |e| Intent::CreateWallet {
                name: e.wallet_name.clone(),
                fund: e.keywords.contains(&"fund".to_string()),
            },
            confidence: 0.9,
            explanation: "Creates a new Stellar wallet with a keypair. If a name is \
                          provided, the wallet is saved under that name.",
        },
        Pattern {
            keywords: &["list", "wallets"],
            intent_factory: |_| Intent::ListWallets,
            confidence: 0.95,
            explanation: "Lists all saved wallets with their names and public keys.",
        },
        Pattern {
            keywords: &["show", "wallet"],
            intent_factory: |e| Intent::ShowWallet {
                name: e.wallet_name.clone(),
            },
            confidence: 0.85,
            explanation: "Displays wallet details including public key, network, and live balance.",
        },
        Pattern {
            keywords: &["fund", "wallet"],
            intent_factory: |e| Intent::FundWallet {
                name: e.wallet_name.clone(),
            },
            confidence: 0.9,
            explanation: "Funds a wallet via the network faucet (testnet only).",
        },
        Pattern {
            keywords: &["remove", "wallet"],
            intent_factory: |e| Intent::RemoveWallet {
                name: e.wallet_name.clone(),
            },
            confidence: 0.9,
            explanation: "Removes a wallet from local storage. This action is irreversible.",
        },
        // ── Contract patterns ───────────────────────────────────────
        Pattern {
            keywords: &["deploy", "contract"],
            intent_factory: |e| Intent::DeployContract {
                wallet: e.wallet_name.clone(),
                network: e.network.clone(),
            },
            confidence: 0.85,
            explanation: "Deploys a compiled Soroban contract (.wasm) to the specified network.",
        },
        Pattern {
            keywords: &["invoke", "contract"],
            intent_factory: |e| Intent::InvokeContract {
                contract_id: e.contract_id.clone(),
                function: e.function_name.clone(),
            },
            confidence: 0.85,
            explanation: "Invokes a function on a deployed Soroban contract.",
        },
        Pattern {
            keywords: &["inspect", "contract"],
            intent_factory: |e| Intent::InspectContract {
                contract_id: e.contract_id.clone(),
            },
            confidence: 0.85,
            explanation: "Inspects a compiled contract .wasm file for its functions and metadata.",
        },
        // ── Network patterns ────────────────────────────────────────
        Pattern {
            keywords: &["show", "network"],
            intent_factory: |_| Intent::ShowNetwork,
            confidence: 0.9,
            explanation:
                "Shows the currently active network (testnet/mainnet) and its configuration.",
        },
        Pattern {
            // "network" itself is rarely said in this phrasing ("switch to
            // mainnet"), so only require the distinctive verb.
            keywords: &["switch"],
            intent_factory: |e| Intent::SwitchNetwork {
                network: e.network.clone().unwrap_or_default(),
            },
            confidence: 0.85,
            explanation: "Switches the active network (e.g., from testnet to mainnet).",
        },
        // ── Node patterns ───────────────────────────────────────────
        Pattern {
            keywords: &["start", "node"],
            intent_factory: |_| Intent::StartNode,
            confidence: 0.9,
            explanation: "Starts a local Soroban devnet via Docker for testing.",
        },
        Pattern {
            // Accepts synonyms like "devnet" for the local node, so only
            // require the distinctive verb.
            keywords: &["stop"],
            intent_factory: |_| Intent::StopNode,
            confidence: 0.9,
            explanation: "Stops the running local devnet.",
        },
        Pattern {
            keywords: &["node", "status"],
            intent_factory: |_| Intent::NodeStatus,
            confidence: 0.85,
            explanation: "Checks the status of the local devnet.",
        },
        // ── Info patterns ───────────────────────────────────────────
        Pattern {
            keywords: &["show", "info"],
            intent_factory: |_| Intent::ShowInfo,
            confidence: 0.8,
            explanation: "Shows StarForge configuration and environment information.",
        },
        Pattern {
            keywords: &["help"],
            intent_factory: |e| Intent::ShowHelp {
                command: e.keywords.first().cloned(),
            },
            confidence: 0.7,
            explanation: "Shows help for a specific command or overview of all commands.",
        },
        // ── Diagnostic patterns ─────────────────────────────────────
        Pattern {
            keywords: &["run", "doctor"],
            intent_factory: |_| Intent::RunDoctor,
            confidence: 0.9,
            explanation: "Runs diagnostics on your StarForge installation and checks connectivity.",
        },
        Pattern {
            keywords: &["diagnostics"],
            intent_factory: |_| Intent::RunDiagnostics,
            confidence: 0.85,
            explanation: "Runs connectivity diagnostics for attached hardware wallets.",
        },
        // ── Transaction patterns ────────────────────────────────────
        Pattern {
            keywords: &["show", "transaction"],
            intent_factory: |e| Intent::ShowTransaction {
                hash: e.keywords.iter().find(|k| k.len() == 64).cloned(),
            },
            confidence: 0.8,
            explanation: "Fetches and displays transaction details for the given hash.",
        },
    ]
});

// ── Entity Extraction ──────────────────────────────────────────────────────

/// Common English stop words to exclude from keyword extraction.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "shall", "should", "may", "might", "must", "can",
    "could", "i", "you", "he", "she", "it", "we", "they", "me", "him", "her", "us", "them", "my",
    "your", "his", "its", "our", "their", "this", "that", "these", "those", "am", "to", "of", "in",
    "for", "on", "with", "at", "by", "from", "up", "about", "into", "through", "during", "before",
    "after", "above", "below", "between", "out", "off", "over", "under", "again", "further",
    "then", "once", "and", "but", "or", "nor", "not", "so", "very", "just", "than", "too", "also",
    "here", "there", "when", "where", "why", "how", "all", "each", "every", "both", "few", "more",
    "most", "other", "some", "such", "no", "only", "own", "same", "now", "if", "please", "me",
    "want",
];

/// Fold domain synonyms onto the vocabulary the pattern table uses, so a user
/// can say "devnet" where the patterns say "node".
///
/// `show` is deliberately *not* a stop word: it is a command verb here, and
/// several patterns key on it.
fn canonical_keyword(word: &str) -> &str {
    match word {
        "devnet" | "localnet" | "sandbox" => "node",
        other => other,
    }
}

/// Extracts entities from the natural language input.
fn extract_entities(input: &str) -> ExtractedEntities {
    let input_lower = input.to_lowercase();
    let words: Vec<&str> = input_lower.split_whitespace().collect();
    let words_orig: Vec<&str> = input.split_whitespace().collect();

    let mut entities = ExtractedEntities::default();

    // Extract wallet name (after "named", "called", "as", "name", or directly after "wallet")
    for i in 0..words.len() {
        if matches!(words[i], "named" | "called" | "as" | "name") && i + 1 < words.len() {
            let name =
                words[i + 1].trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if !name.is_empty() {
                entities.wallet_name = Some(name.to_string());
                break;
            }
        }
    }

    // Fallback: extract wallet name directly after the word "wallet"
    if entities.wallet_name.is_none() {
        for i in 0..words.len() {
            if words[i] == "wallet" && i + 1 < words.len() {
                let candidate = words[i + 1]
                    .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
                // Don't treat common nouns (like "balance") as wallet names
                let common_nouns = [
                    "balance", "list", "show", "create", "fund", "remove", "info", "help",
                ];
                if !candidate.is_empty() && !common_nouns.contains(&candidate) {
                    entities.wallet_name = Some(candidate.to_string());
                    break;
                }
            }
        }
    }

    // Extract contract ID (starts with C and is 56 chars).
    //
    // Scanned against the original-case words, not the lower-cased copy:
    // contract IDs are upper-case base32 and lower-casing them destroys the
    // `C` prefix this looks for, as well as the ID itself.
    for word in &words_orig {
        let cleaned: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        if (cleaned.starts_with('c') || cleaned.starts_with('C')) && cleaned.len() == 56 {
            entities.contract_id = Some(cleaned.to_uppercase());
            break;
        }
    }

    // Extract network name
    for word in &words {
        match *word {
            "testnet" => {
                entities.network = Some("testnet".to_string());
                break;
            }
            "mainnet" => {
                entities.network = Some("mainnet".to_string());
                break;
            }
            _ => {}
        }
    }

    // Extract file path (words containing .wasm, .rs, etc.)
    for word in &words {
        if word.ends_with(".wasm") || word.ends_with(".rs") || word.ends_with(".toml") {
            entities.file_path = Some(word.to_string());
            break;
        }
    }

    // Extract amount (numbers after "fund", "send", etc.)
    for i in 0..words.len() {
        if matches!(words[i], "fund" | "send" | "pay")
            && i + 1 < words.len()
            && words[i + 1].parse::<f64>().is_ok()
        {
            entities.amount = Some(words[i + 1].to_string());
            break;
        }
    }

    // Extract function name (after "call", "run", "execute").
    //
    // "invoke" is not in this list: it introduces the *contract*, as in
    // "invoke contract <id> call <function>".
    for i in 0..words.len() {
        if matches!(words[i], "call" | "run" | "execute") && i + 1 < words.len() {
            let func = words[i + 1].trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if func == "contract" {
                continue;
            }
            if !func.is_empty() {
                entities.function_name = Some(func.to_string());
            }
        }
    }

    // Collect meaningful keywords (exclude stop words)
    for word in &words {
        let cleaned: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if cleaned.is_empty() || cleaned.len() <= 2 || STOP_WORDS.contains(&cleaned.as_str()) {
            continue;
        }
        let canonical = canonical_keyword(&cleaned).to_string();
        if !entities.keywords.contains(&canonical) {
            entities.keywords.push(canonical);
        }
    }

    entities
}

// ── Intent Recognition ─────────────────────────────────────────────────────

/// Recognizes the intent from extracted entities and keywords.
fn recognize_intent(entities: &ExtractedEntities) -> (Intent, f64) {
    let keywords = &entities.keywords;

    let mut best_match: Option<(&Pattern, f64)> = None;

    for pattern in PATTERNS.iter() {
        let match_count = pattern
            .keywords
            .iter()
            .filter(|k| {
                // A keyword naming an entity kind is satisfied by having
                // extracted that entity: "switch to mainnet" never says the
                // word "network", but it clearly names one.
                let entity_named = match **k {
                    "network" => entities.network.is_some(),
                    "contract" => entities.contract_id.is_some(),
                    _ => false,
                };
                entity_named
                    || keywords
                        .iter()
                        .any(|ek| ek.contains(*k) || k.contains(ek.as_str()))
                    || entities
                        .wallet_name
                        .as_ref()
                        .map(|w| w.contains(*k))
                        .unwrap_or(false)
            })
            .count();

        let match_ratio = match_count as f64 / pattern.keywords.len() as f64;
        let adjusted_confidence = pattern.confidence * match_ratio;

        if match_ratio >= 0.5
            && best_match
                .as_ref()
                .map(|(_, c)| adjusted_confidence > *c)
                .unwrap_or(true)
        {
            best_match = Some((pattern, adjusted_confidence));
        }
    }

    match best_match {
        Some((pattern, confidence)) => ((pattern.intent_factory)(entities), confidence),
        None => (
            Intent::Unknown {
                input: keywords.join(" "),
            },
            if keywords.is_empty() { 0.1 } else { 0.2 },
        ),
    }
}

// ── Command Translation ────────────────────────────────────────────────────

/// Translates an intent and entities into a CLI command string.
fn translate_to_command(intent: &Intent, _entities: &ExtractedEntities) -> String {
    match intent {
        Intent::CreateWallet { name, fund } => {
            let name = name.as_deref().unwrap_or("wallet");
            if *fund {
                format!("starforge wallet create {} --fund", name)
            } else {
                format!("starforge wallet create {}", name)
            }
        }
        Intent::ListWallets => "starforge wallet list".to_string(),
        Intent::ShowWallet { name } => {
            let name = name.as_deref().unwrap_or("wallet");
            format!("starforge wallet show {}", name)
        }
        Intent::FundWallet { name } => {
            let name = name.as_deref().unwrap_or("wallet");
            format!("starforge wallet fund {}", name)
        }
        Intent::RemoveWallet { name } => {
            let name = name.as_deref().unwrap_or("wallet");
            format!("starforge wallet remove {}", name)
        }
        Intent::DeployContract { wallet, network } => {
            let mut cmd = "starforge deploy --wasm contract.wasm".to_string();
            if let Some(w) = wallet {
                cmd.push_str(&format!(" --wallet {}", w));
            }
            if let Some(n) = network {
                cmd.push_str(&format!(" --network {}", n));
            }
            cmd
        }
        Intent::InvokeContract {
            contract_id,
            function,
        } => {
            let mut cmd = "starforge contract invoke".to_string();
            if let Some(id) = contract_id {
                cmd.push_str(&format!(" --contract {}", id));
            }
            if let Some(f) = function {
                cmd.push_str(&format!(" --fn {}", f));
            }
            cmd
        }
        Intent::InspectContract { contract_id } => {
            let mut cmd = "starforge inspect state".to_string();
            if let Some(id) = contract_id {
                cmd.push_str(&format!(" --contract {}", id));
            }
            cmd
        }
        Intent::ShowNetwork => "starforge network show".to_string(),
        Intent::SwitchNetwork { network } => format!("starforge network switch {}", network),
        Intent::AddNetwork { name, horizon } => {
            let mut cmd = format!("starforge network add {}", name);
            if let Some(h) = horizon {
                cmd.push_str(&format!(" --horizon {}", h));
            }
            cmd
        }
        Intent::StartNode => "starforge node start".to_string(),
        Intent::StopNode => "starforge node stop".to_string(),
        Intent::NodeStatus => "starforge node status".to_string(),
        Intent::ShowInfo => "starforge info".to_string(),
        Intent::ShowHelp { command } => {
            let cmd = command.as_deref().unwrap_or("");
            format!("starforge help {}", cmd)
        }
        Intent::RunDoctor => "starforge config doctor".to_string(),
        Intent::RunDiagnostics => "starforge diagnostics".to_string(),
        Intent::ShowTransaction { hash } => {
            let mut cmd = "starforge tx".to_string();
            if let Some(h) = hash {
                cmd.push_str(&format!(" {}", h));
            }
            cmd
        }
        Intent::Ambiguous { candidates } => {
            format!("(ambiguous — did you mean: {})", candidates.join(", "))
        }
        Intent::Unknown { input } => {
            format!("starforge --help (could not understand: '{}')", input)
        }
    }
}

// ── Educational Explanation ────────────────────────────────────────────────

/// Generates an educational explanation for the parsed command.
fn generate_explanation(intent: &Intent, command: &str) -> String {
    let mut explanation = String::new();

    match intent {
        Intent::CreateWallet { name, fund } => {
            explanation.push_str(&format!(
                "🔐 I'll create a new Stellar wallet{}",
                if let Some(n) = name {
                    format!(" named '{}'", n)
                } else {
                    String::new()
                }
            ));
            if *fund {
                explanation.push_str(" and fund it with testnet XLM");
            }
            explanation.push_str(".\n\n");
            explanation.push_str(
                "This generates a new Ed25519 keypair and saves it locally.\n\
                 The wallet can be used to sign and submit transactions.\n",
            );
        }
        Intent::ListWallets => {
            explanation.push_str("📋 I'll list all wallets saved locally.\n\n");
            explanation.push_str("This shows wallet names, public keys, and networks.\n");
        }
        Intent::ShowWallet { name } => {
            explanation.push_str(&format!(
                "🔍 I'll show details for wallet '{}'.\n\n",
                name.as_deref().unwrap_or("wallet"),
            ));
            explanation.push_str(
                "This fetches the live balance from the network and displays wallet info.\n",
            );
        }
        Intent::FundWallet { name } => {
            explanation.push_str(&format!(
                "💰 I'll fund wallet '{}' via the testnet faucet.\n\n",
                name.as_deref().unwrap_or("wallet"),
            ));
            explanation.push_str("Friendbot sends 10,000 XLM to testnet accounts.\n");
        }
        Intent::DeployContract { wallet, network } => {
            explanation.push_str("🚀 I'll deploy a compiled Soroban contract.\n\n");
            explanation
                .push_str("This submits a contract deployment transaction to the network.\n");
            if let Some(w) = wallet {
                explanation.push_str(&format!("Using wallet '{}' for signing.\n", w));
            }
            explanation.push_str(&format!(
                "Targeting {} network.\n",
                network.as_deref().unwrap_or("testnet"),
            ));
        }
        Intent::InvokeContract {
            contract_id,
            function,
        } => {
            explanation.push_str("⚡ I'll invoke a function on a deployed contract.\n\n");
            if let Some(id) = contract_id {
                explanation.push_str(&format!("Contract: {}\n", id));
            }
            if let Some(f) = function {
                explanation.push_str(&format!("Function: {}\n", f));
            }
        }
        Intent::ShowNetwork => {
            explanation.push_str("🌐 I'll show the current network configuration.\n\n");
            explanation.push_str(
                "This displays the active network (testnet/mainnet) and its Horizon URL.\n",
            );
        }
        Intent::SwitchNetwork { network } => {
            explanation.push_str(&format!("🔄 I'll switch to the {} network.\n\n", network));
            explanation.push_str("This changes the active network for all subsequent commands.\n");
        }
        Intent::StartNode => {
            explanation.push_str("🐳 I'll start a local Soroban devnet via Docker.\n\n");
            explanation.push_str("This launches a local Stellar node for testing.\n");
        }
        Intent::RunDoctor => {
            explanation.push_str("🩺 I'll run diagnostics on your StarForge installation.\n\n");
            explanation.push_str("This checks for missing dependencies and connectivity issues.\n");
        }
        _ => {
            explanation.push_str("I'll execute the following command:\n\n");
        }
    }

    explanation.push_str(&format!("\nCommand: {}\n", command.bright_white()));
    explanation
}

// ── Confidence Rating ──────────────────────────────────────────────────────

/// Returns a human-readable confidence label and its coloured display.
fn confidence_label(confidence: f64) -> &'static str {
    if confidence >= 0.9 {
        "High"
    } else if confidence >= 0.7 {
        "Medium"
    } else if confidence >= 0.5 {
        "Low"
    } else {
        "Very Low"
    }
}

fn confidence_color(confidence: f64) -> ColoredString {
    let label = confidence_label(confidence);
    match label {
        "High" => label.green().bold(),
        "Medium" => label.yellow().bold(),
        "Low" => label.yellow(),
        _ => label.red(),
    }
}

// ── Main Handler ───────────────────────────────────────────────────────────

/// Handle the natural language command interface.
pub async fn handle(args: NlArgs) -> Result<()> {
    let input = args.input.trim();

    if input.is_empty() {
        p::error("No input provided. Please provide a natural language command.");
        println!();
        p::info("Examples:");
        println!(
            "  {} {}",
            "→".cyan(),
            "\"create a new wallet named alice\"".bright_white()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "\"deploy my contract to testnet\"".bright_white()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "\"show me the balance of wallet bob\"".bright_white()
        );
        println!();
        p::info("Run `starforge nl --help` for usage information.");
        return Ok(());
    }

    // Step 1: Extract entities
    let entities = extract_entities(input);

    // Step 2: Recognize intent
    let (intent, confidence) = recognize_intent(&entities);

    // Step 3: Translate to command
    let command = translate_to_command(&intent, &entities);

    // Step 4: Generate explanation
    let explanation = generate_explanation(&intent, &command);

    // Build the parsed command result

    // ── Verbose / Dry-Run Output ──────────────────────────────────
    if args.verbose || args.dry_run {
        p::header("Natural Language Translation");
        println!();
        p::kv("Input", input);
        p::kv("Intent", &format!("{:?}", intent));
        p::kv("Confidence", &format!("{:.0}%", confidence * 100.0));
        println!();

        if !entities.keywords.is_empty() {
            p::kv("Keywords", &entities.keywords.join(", "));
        }
        if let Some(ref name) = entities.wallet_name {
            p::kv("Wallet", name);
        }
        if let Some(ref id) = entities.contract_id {
            p::kv("Contract ID", id);
        }
        if let Some(ref net) = entities.network {
            p::kv("Network", net);
        }
        println!();
    }

    if args.explain || args.dry_run {
        p::header("Explanation");
        println!();
        print!("{}", explanation);
        println!();
    }

    if args.dry_run {
        p::header("Translated Command");
        println!();
        println!("  $ {}", command.bright_white());
        println!();
        p::info("Dry run complete. No command was executed.");
        return Ok(());
    }

    // ── Main Output ───────────────────────────────────────────────
    p::header("Command Translation");
    println!();
    println!("  $ {}", command.bright_white());
    println!();

    // Show confidence
    p::kv(
        "Confidence",
        &format!(
            "{} ({:.0}%)",
            confidence_color(confidence),
            confidence * 100.0
        ),
    );
    println!();

    // Handle low confidence
    if confidence < 0.5 {
        p::warn("Low confidence translation. The command may not be what you intended.");
        println!();
        p::info(
            "Try rephrasing your command or use `starforge <command> --help` for direct syntax.",
        );
        println!();
        p::info("Available commands:");
        println!("  {} wallet    — Manage test wallets", "→".cyan());
        println!("  {} deploy    — Deploy contracts", "→".cyan());
        println!("  {} contract  — Contract operations", "→".cyan());
        println!("  {} network   — Network management", "→".cyan());
        println!("  {} node      — Local devnet", "→".cyan());
        println!("  {} help      — Get help", "→".cyan());
        return Ok(());
    }

    // Handle ambiguous intent
    if let Intent::Ambiguous { candidates } = &intent {
        p::warn("Multiple interpretations found:");
        for (i, candidate) in candidates.iter().enumerate() {
            println!("  {}. {}", i + 1, candidate.bright_white());
        }
        println!();
        p::info("Please be more specific or use `starforge <command> --help`.");
        return Ok(());
    }

    // Handle unknown intent
    if let Intent::Unknown { input: ref unknown } = &intent {
        p::error(&format!("I couldn't understand: '{}'", unknown));
        println!();
        p::info("Try rephrasing your command. Here are some examples:");
        println!();
        println!(
            "  {} {}",
            "→".cyan(),
            "\"create a wallet named alice\"".bright_white()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "\"deploy my contract\"".bright_white()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "\"show wallet balance\"".bright_white()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "\"switch to mainnet\"".bright_white()
        );
        println!();
        return Ok(());
    }

    // Interactive confirmation
    if args.interactive {
        p::header("Interactive Confirmation");
        println!();
        println!(
            "  {} I will execute: {}",
            "→".cyan(),
            command.bright_white()
        );
        println!();
        p::info("Press Enter to continue or Ctrl+C to cancel.");
        println!();

        print!("  {} ", "?".yellow().bold());
        use std::io::{self, Write};
        io::stdout().flush()?;
        let mut buffer = String::new();
        io::stdin().read_line(&mut buffer)?;
    }

    // Show explanation before execution
    if !args.explain && !args.dry_run {
        println!("{}", explanation);
    }

    // Execute the translated command via the existing CLI infrastructure.
    // We re-invoke `starforge <subcommand> ...` so that logging, correlation
    // IDs, telemetry, and all global flags are applied consistently.
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        p::error("No command to execute.");
        return Ok(());
    }

    let result = TokioCommand::new(parts[0])
        .args(&parts[1..])
        .output()
        .await
        .context("Failed to execute translated command")?;

    if result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        if !stdout.is_empty() {
            print!("{}", stdout);
        }
        p::success("Command executed successfully.");
    } else {
        let stderr = String::from_utf8_lossy(&result.stderr);
        if !stderr.is_empty() {
            eprint!("{}", stderr);
        }
        p::error(&format!(
            "Command failed with exit code: {}",
            result.status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Entity extraction ────────────────────────────────────────────

    #[test]
    fn extract_wallet_name_named() {
        let e = extract_entities("create a wallet named alice");
        assert_eq!(e.wallet_name, Some("alice".to_string()));
    }

    #[test]
    fn extract_wallet_name_called() {
        let e = extract_entities("create a wallet called bob");
        assert_eq!(e.wallet_name, Some("bob".to_string()));
    }

    #[test]
    fn extract_wallet_name_directly_after_keyword() {
        let e = extract_entities("show me the balance of wallet bob");
        assert_eq!(e.wallet_name, Some("bob".to_string()));
    }

    #[test]
    fn extract_network_testnet() {
        let e = extract_entities("deploy to testnet");
        assert_eq!(e.network, Some("testnet".to_string()));
    }

    #[test]
    fn extract_network_mainnet() {
        let e = extract_entities("switch to mainnet");
        assert_eq!(e.network, Some("mainnet".to_string()));
    }

    #[test]
    fn extract_contract_id() {
        // A Stellar contract ID is exactly 56 characters.
        let id = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM";
        assert_eq!(id.len(), 56, "fixture must be a real contract ID length");

        let e = extract_entities(&format!("invoke contract {}", id));
        assert_eq!(e.contract_id.as_deref(), Some(id));
    }

    #[test]
    fn extract_function_name() {
        let e = extract_entities("invoke contract call transfer");
        assert_eq!(e.function_name, Some("transfer".to_string()));
    }

    #[test]
    fn fund_flag_in_keywords() {
        let e = extract_entities("create a wallet named alice and fund it");
        assert!(e.keywords.contains(&"fund".to_string()));
    }

    #[test]
    fn empty_input_yields_no_keywords() {
        let e = extract_entities("");
        assert!(e.keywords.is_empty());
        assert!(e.wallet_name.is_none());
    }

    #[test]
    fn stop_words_filtered() {
        let e = extract_entities("please show me the wallet");
        assert!(!e.keywords.contains(&"please".to_string()));
        assert!(!e.keywords.contains(&"the".to_string()));
    }

    // ── Intent recognition ───────────────────────────────────────────

    #[test]
    fn recognize_create_wallet() {
        let e = extract_entities("create a wallet named alice");
        let (intent, confidence) = recognize_intent(&e);
        assert!(matches!(intent, Intent::CreateWallet { .. }));
        assert!(confidence > 0.5);
    }

    #[test]
    fn recognize_list_wallets() {
        let e = extract_entities("list all wallets");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::ListWallets));
    }

    #[test]
    fn recognize_show_wallet() {
        let e = extract_entities("show wallet balance");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::ShowWallet { .. }));
    }

    #[test]
    fn recognize_deploy_contract() {
        let e = extract_entities("deploy my contract to testnet");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::DeployContract { .. }));
    }

    #[test]
    fn recognize_switch_network() {
        let e = extract_entities("switch to mainnet");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::SwitchNetwork { .. }));
    }

    #[test]
    fn recognize_start_node() {
        let e = extract_entities("start the local node");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::StartNode));
    }

    #[test]
    fn recognize_stop_node() {
        let e = extract_entities("stop the devnet");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::StopNode));
    }

    #[test]
    fn recognize_run_doctor() {
        let e = extract_entities("run doctor diagnostics");
        let (intent, _) = recognize_intent(&e);
        assert!(matches!(intent, Intent::RunDoctor));
    }

    // ── Command translation ──────────────────────────────────────────

    #[test]
    fn translate_create_wallet_with_name_and_fund() {
        let intent = Intent::CreateWallet {
            name: Some("alice".to_string()),
            fund: true,
        };
        let e = ExtractedEntities {
            wallet_name: Some("alice".to_string()),
            ..Default::default()
        };
        let cmd = translate_to_command(&intent, &e);
        assert_eq!(cmd, "starforge wallet create alice --fund");
    }

    #[test]
    fn translate_list_wallets() {
        let intent = Intent::ListWallets;
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert_eq!(cmd, "starforge wallet list");
    }

    #[test]
    fn translate_show_wallet() {
        let intent = Intent::ShowWallet {
            name: Some("bob".to_string()),
        };
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert_eq!(cmd, "starforge wallet show bob");
    }

    #[test]
    fn translate_show_network() {
        let intent = Intent::ShowNetwork;
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert_eq!(cmd, "starforge network show");
    }

    #[test]
    fn translate_switch_network() {
        let intent = Intent::SwitchNetwork {
            network: "mainnet".to_string(),
        };
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert_eq!(cmd, "starforge network switch mainnet");
    }

    #[test]
    fn translate_deploy_contract() {
        let intent = Intent::DeployContract {
            wallet: Some("deployer".to_string()),
            network: Some("testnet".to_string()),
        };
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert_eq!(
            cmd,
            "starforge deploy --wasm contract.wasm --wallet deployer --network testnet"
        );
    }

    #[test]
    fn translate_invoke_contract() {
        let intent = Intent::InvokeContract {
            contract_id: Some("CABC123".to_string()),
            function: Some("transfer".to_string()),
        };
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert_eq!(
            cmd,
            "starforge contract invoke --contract CABC123 --fn transfer"
        );
    }

    #[test]
    fn translate_unknown() {
        let intent = Intent::Unknown {
            input: "foo bar".to_string(),
        };
        let cmd = translate_to_command(&intent, &ExtractedEntities::default());
        assert!(cmd.contains("could not understand"));
    }

    // ── Confidence ───────────────────────────────────────────────────

    #[test]
    fn confidence_label_high() {
        assert_eq!(confidence_label(0.95), "High");
    }

    #[test]
    fn confidence_label_medium() {
        assert_eq!(confidence_label(0.75), "Medium");
    }

    #[test]
    fn confidence_label_low() {
        assert_eq!(confidence_label(0.4), "Very Low");
    }

    // ── Round-trip integration ───────────────────────────────────────

    #[test]
    fn roundtrip_create_wallet_named_alice_fund() {
        let input = "create a new wallet named alice and fund it";
        let entities = extract_entities(input);
        let (intent, confidence) = recognize_intent(&entities);
        let cmd = translate_to_command(&intent, &entities);
        assert!(matches!(intent, Intent::CreateWallet { .. }));
        assert!(confidence > 0.5);
        assert!(cmd.contains("alice"));
        assert!(cmd.contains("--fund"));
    }

    #[test]
    fn roundtrip_deploy_to_testnet() {
        let input = "deploy my contract to testnet";
        let entities = extract_entities(input);
        let (intent, _) = recognize_intent(&entities);
        let cmd = translate_to_command(&intent, &entities);
        assert!(matches!(intent, Intent::DeployContract { .. }));
        assert!(cmd.contains("--network testnet"));
    }

    #[test]
    fn roundtrip_show_wallet_balance() {
        let input = "show me the balance of wallet bob";
        let entities = extract_entities(input);
        let (intent, _) = recognize_intent(&entities);
        let cmd = translate_to_command(&intent, &entities);
        assert!(matches!(intent, Intent::ShowWallet { .. }));
        assert!(cmd.contains("bob"));
    }
}
