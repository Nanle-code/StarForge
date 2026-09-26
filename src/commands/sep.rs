use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::sep::sep10::{self, Sep10Client};
use crate::utils::{config, output, print as p, wallet_signer};

/// SEP-10 web authentication commands.
///
/// `sep10` is the SEP-10 entry point: it reads an anchor's stellar.toml, runs
/// the challenge/response handshake with a local wallet, and prints the JWT the
/// anchor issues. The module is laid out as a command group so SEP-24 and
/// SEP-31 testing tools can follow the same shape.
#[derive(Args)]
pub struct Sep10Args {
    #[command(subcommand)]
    pub command: Sep10Commands,
}

#[derive(Subcommand)]
pub enum Sep10Commands {
    /// Authenticate against a SEP-10 server and print the JWT it issues
    Auth(Sep10AuthArgs),
}

#[derive(Args)]
pub struct Sep10AuthArgs {
    /// Anchor home domain; its https://<domain>/.well-known/stellar.toml is read
    #[arg(long)]
    pub domain: String,

    /// Local wallet to authenticate with
    #[arg(long)]
    pub wallet: String,

    /// Network the anchor operates on
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,

    /// Override the stellar.toml URL, e.g. for a local reference server
    #[arg(long)]
    pub toml_url: Option<String>,

    /// Emit a machine-readable JSON object instead of the human-readable output
    #[arg(long)]
    pub json: bool,

    /// Show every step and value of the handshake
    #[arg(long, default_value = "false")]
    pub verbose: bool,

    /// Also write the JWT to this file
    #[arg(long)]
    pub output: Option<PathBuf>,
}

pub async fn handle(args: Sep10Args) -> Result<()> {
    match args.command {
        Sep10Commands::Auth(args) => handle_auth(args).await,
    }
}

#[derive(serde::Serialize)]
struct AuthReport {
    home_domain: String,
    web_auth_endpoint: String,
    account: String,
    signing_key: String,
    network_passphrase: String,
    challenge_data_name: String,
    challenge_seconds_remaining: u64,
    signatures_after_signing: usize,
    jwt: String,
    jwt_claims: Option<sep10::JwtClaims>,
    jwt_file: Option<String>,
}

/// Prints the handshake as it progresses, unless the caller asked for JSON, in
/// which case stdout must carry nothing but the JSON document.
struct Reporter {
    human: bool,
    verbose: bool,
}

impl Reporter {
    fn new(human: bool, verbose: bool) -> Self {
        Self { human, verbose }
    }

    fn header(&self, message: &str) {
        if self.human {
            p::header(message);
        }
    }

    fn separator(&self) {
        if self.human {
            p::separator();
        }
    }

    fn blank_line(&self) {
        if self.human {
            println!();
        }
    }

    fn kv(&self, key: &str, value: &str) {
        if self.human {
            p::kv(key, value);
        }
    }

    fn step(&self, index: usize, total: usize, message: &str) {
        if self.human {
            p::step(index, total, message);
        }
    }

    fn success(&self, message: &str) {
        if self.human {
            p::success(message);
        }
    }

    fn info(&self, message: &str) {
        if self.human {
            p::info(message);
        }
    }

    fn detail(&self, key: &str, value: &str) {
        if self.human && self.verbose {
            p::kv(key, value);
        }
    }
}

async fn handle_auth(args: Sep10AuthArgs) -> Result<()> {
    let emit_json = args.json || output::is_json_mode_enabled();
    let reporter = Reporter::new(!emit_json, args.verbose);

    reporter.header("SEP-10 Web Authentication");

    config::validate_wallet_name(&args.wallet)?;
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|wallet| wallet.name == args.wallet)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet '{}' not found. Run `starforge wallet list`",
                args.wallet
            )
        })?;

    let passphrase = config::get_network_passphrase(&args.network);
    let mut client = Sep10Client::new(&args.domain, &passphrase)
        .with_context(|| format!("could not build a SEP-10 client for '{}'", args.domain))?;
    if let Some(toml_url) = args.toml_url.as_deref() {
        client = client.with_toml_url(toml_url);
    }

    reporter.separator();
    reporter.kv("Home Domain", &args.domain);
    reporter.kv("Wallet", &wallet.name);
    reporter.kv("Account", &wallet.public_key);
    reporter.kv("Network", &args.network);
    reporter.separator();
    reporter.blank_line();

    // 1/5 — the anchor's stellar.toml tells us where to authenticate and which
    // key must have signed the challenge.
    reporter.step(1, 5, "Reading stellar.toml…");
    reporter.detail("stellar.toml", &client.toml_url());
    let toml = client.load_stellar_toml().await?;
    let web_auth_endpoint = toml.web_auth_endpoint(client.home_domain())?.to_string();
    reporter.kv("Web Auth Endpoint", &web_auth_endpoint);
    let signing_key = toml.signing_key(client.home_domain())?.to_string();
    reporter.detail("SIGNING_KEY", &signing_key);
    reporter.blank_line();

    // 2/5 — the anchor builds a challenge for our account.
    reporter.step(2, 5, "Requesting challenge…");
    let challenge = client.fetch_challenge(&toml, &wallet.public_key).await?;
    reporter.detail("Challenge XDR", &summarize(&challenge.transaction));
    reporter.blank_line();

    // 3/5 — every rule SEP-10 states is checked locally before we sign, so a
    // hostile or replayed challenge never earns our signature.
    reporter.step(3, 5, "Validating challenge…");
    let (envelope, validated) = client.validate_challenge(
        &toml,
        &challenge.transaction,
        &wallet.public_key,
        sep10::unix_now(),
    )?;
    reporter.kv(
        "Challenge",
        &format!(
            "{} auth, {}s remaining",
            validated.home_domain, validated.seconds_remaining
        ),
    );
    reporter.detail("Challenge source", &validated.source_account);
    reporter.detail("Nonce (hex)", &validated.nonce);
    reporter.detail(
        "Time Bounds",
        &format!("{}..{}", validated.min_time, validated.max_time),
    );
    reporter.detail("Anchor Signature", "valid");
    reporter.blank_line();

    // 4/5 — sign the validated challenge with the wallet's key.
    reporter.step(4, 5, "Signing challenge…");
    let secret = wallet_signer::resolve_local_secret(wallet, &wallet.name)?;
    let signed_xdr = client.sign_challenge(&toml, &envelope, secret.as_str())?;
    let signatures_after_signing = sep10::signature_count(&signed_xdr)?;
    reporter.detail(
        "Signatures",
        &format!("{} (anchor + wallet)", signatures_after_signing),
    );
    reporter.blank_line();

    // 5/5 — exchange the signed challenge for a JWT.
    reporter.step(5, 5, "Submitting signed challenge…");
    let token = client.submit_challenge(&toml, &signed_xdr).await?;
    let claims = sep10::decode_jwt_claims(&token);

    let mut jwt_file = None;
    if let Some(path) = args.output.as_ref() {
        std::fs::write(path, format!("{}\n", token))
            .with_context(|| format!("could not write {}", path.display()))?;
        jwt_file = Some(path.display().to_string());
    }

    let report = AuthReport {
        home_domain: args.domain.clone(),
        web_auth_endpoint,
        account: wallet.public_key.clone(),
        signing_key,
        network_passphrase: client.network_passphrase_for(&toml),
        challenge_data_name: validated.data_name.clone(),
        challenge_seconds_remaining: validated.seconds_remaining,
        signatures_after_signing,
        jwt: token.clone(),
        jwt_claims: claims.clone(),
        jwt_file,
    };

    if emit_json {
        return output::print_json(&report);
    }

    reporter.success("Authenticated");
    if let Some(claims) = claims.as_ref() {
        if let Some(subject) = claims.sub.as_deref() {
            reporter.kv("Subject", subject);
        }
        if let Some(expires_at) = claims.exp {
            reporter.kv(
                "Expires",
                &format!(
                    "{} (in {}s)",
                    expires_at,
                    expires_at.saturating_sub(sep10::unix_now())
                ),
            );
        }
    }
    if let Some(path) = report.jwt_file.as_deref() {
        reporter.info(&format!("JWT written to {}", path));
    }
    reporter.separator();
    reporter.blank_line();
    println!("{}", token);

    Ok(())
}

fn summarize(value: &str) -> String {
    if value.len() <= 32 {
        value.to_string()
    } else {
        format!("{}… ({} chars)", &value[..32], value.len())
    }
}
