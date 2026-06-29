use crate::utils::print as p;
use crate::utils::{config, fuzzing};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum FuzzCommands {
    /// Fuzz a contract WASM binary against the validator (byte mutation)
    Wasm(WasmArgs),
    /// Run the built-in property-based test suite (token model invariants)
    Property(PropertyArgs),
    /// Mutation-test a contract source file
    Mutate(MutateArgs),
    /// Manage saved fuzzing corpora
    Corpus(CorpusArgs),
}

#[derive(clap::Args)]
pub struct WasmArgs {
    /// Path to the compiled .wasm to use as the fuzzing seed
    #[arg(long)]
    pub wasm: PathBuf,
    /// Number of mutated inputs to try
    #[arg(long, default_value = "2000")]
    pub iterations: usize,
    /// RNG seed for reproducible runs
    #[arg(long, default_value = "3735928559")]
    pub seed: u64,
}

#[derive(clap::Args)]
pub struct PropertyArgs {
    /// Number of generated inputs to test
    #[arg(long, default_value = "1000")]
    pub iterations: usize,
    /// RNG seed for reproducible runs
    #[arg(long, default_value = "3735928559")]
    pub seed: u64,
}

#[derive(clap::Args)]
pub struct MutateArgs {
    /// Path to the contract source file to mutate
    #[arg(long)]
    pub source: PathBuf,
    /// Emit JSON instead of formatted output
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct CorpusArgs {
    /// List stored corpora
    #[arg(long)]
    pub list: bool,
    /// Show the contents of a named corpus
    #[arg(long)]
    pub show: Option<String>,
}

pub fn handle(cmd: FuzzCommands) -> Result<()> {
    match cmd {
        FuzzCommands::Wasm(args) => handle_wasm(args),
        FuzzCommands::Property(args) => handle_property(args),
        FuzzCommands::Mutate(args) => handle_mutate(args),
        FuzzCommands::Corpus(args) => handle_corpus(args),
    }
}

fn handle_wasm(args: WasmArgs) -> Result<()> {
    config::validate_file_path(&args.wasm, Some("wasm"))?;
    let bytes = std::fs::read(&args.wasm)?;

    p::header("WASM Validator Fuzzing");
    p::kv("Seed binary", &args.wasm.display().to_string());
    p::kv("Iterations", &args.iterations.to_string());
    p::kv("RNG seed", &args.seed.to_string());

    let cfg = fuzzing::FuzzConfig {
        iterations: args.iterations,
        seed: args.seed,
        ..fuzzing::FuzzConfig::default()
    };
    let report = fuzzing::fuzz_wasm_validator(&bytes, &cfg);

    p::separator();
    p::kv("Rejected (graceful)", &report.rejected.to_string());
    p::kv("Accepted", &report.accepted.to_string());
    p::kv_accent("Crashes (panics)", &report.crashes.to_string());
    p::separator();

    if report.crashes > 0 {
        for c in &report.crash_inputs {
            p::warn(&format!("crash input prefix: {}", c));
        }
        anyhow::bail!("Validator panicked on {} mutated input(s)", report.crashes);
    }
    p::success("Validator handled every mutated input gracefully");
    Ok(())
}

fn handle_property(args: PropertyArgs) -> Result<()> {
    p::header("Property-Based Testing");
    p::kv("Suite", "token transfer model (supply conservation)");
    p::kv("Iterations", &args.iterations.to_string());
    p::kv("RNG seed", &args.seed.to_string());

    let cfg = fuzzing::FuzzConfig {
        iterations: args.iterations,
        seed: args.seed,
        ..fuzzing::FuzzConfig::default()
    };
    let report = fuzzing::run_token_property_suite(&cfg);
    p::separator();
    p::kv("Cases run", &report.iterations_run.to_string());

    if report.passed {
        p::success("All properties held across every generated input");
        return Ok(());
    }

    p::error(&format!(
        "Property violated: {}",
        report.failure_message.unwrap_or_default()
    ));
    if let Some(shrunk) = &report.shrunk_counterexample {
        let rendered: Vec<String> = shrunk.iter().map(|v| v.to_string()).collect();
        p::kv("Minimal counterexample", &rendered.join(", "));
        p::kv("Shrink steps", &report.shrink_steps.to_string());
    }
    anyhow::bail!("Property-based test failed");
}

fn handle_mutate(args: MutateArgs) -> Result<()> {
    config::validate_file_path(&args.source, None)?;
    let source = std::fs::read_to_string(&args.source)?;
    let mutants = fuzzing::generate_mutants(&source);
    let score = fuzzing::score_mutants(&mutants, fuzzing::heuristic_oracle);

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mutants": mutants,
                "score": score,
            }))?
        );
        return Ok(());
    }

    p::header("Mutation Testing");
    p::kv("Source", &args.source.display().to_string());
    p::kv("Mutants generated", &score.total.to_string());
    p::kv_accent("Mutation score", &format!("{:.1}%", score.score_pct));
    p::kv("Killed", &score.killed.to_string());
    p::kv("Survived", &score.survived.to_string());
    p::separator();

    if score.survivors.is_empty() {
        p::success("No surviving mutants — strong coverage of critical logic");
        return Ok(());
    }

    p::warn("Surviving mutants indicate untested logic:");
    for m in score.survivors.iter().take(25) {
        println!("  L{} [{}] {}", m.line, m.category, m.description);
    }
    if score.survivors.len() > 25 {
        p::info(&format!("… and {} more", score.survivors.len() - 25));
    }
    Ok(())
}

fn handle_corpus(args: CorpusArgs) -> Result<()> {
    if let Some(name) = args.show {
        let corpus = fuzzing::load_corpus(&name)?;
        p::header(&format!("Corpus: {}", name));
        if corpus.is_empty() {
            p::info("Corpus is empty or does not exist");
            return Ok(());
        }
        for (i, input) in corpus.iter().enumerate() {
            let rendered: Vec<String> = input.iter().map(|v| v.to_string()).collect();
            println!("  {:>3}: {}", i, rendered.join(", "));
        }
        return Ok(());
    }

    // Default + --list both list available corpora.
    let _ = args.list;
    p::header("Fuzzing Corpora");
    let names = fuzzing::list_corpora()?;
    if names.is_empty() {
        p::info("No corpora stored yet.");
        return Ok(());
    }
    for n in names {
        p::kv("•", &n);
    }
    Ok(())
}
