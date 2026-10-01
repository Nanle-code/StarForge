//! starforge dev — a local Soroban contract development watch loop.

use crate::utils::progress::ProgressReporter;
use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Deserialize)]
pub struct DevManifest {
    #[serde(default = "default_sources")]
    pub sources: Vec<PathBuf>,
    #[serde(default = "default_build")]
    pub build: Vec<String>,
    #[serde(default)]
    pub deploy: Option<Vec<String>>,
    #[serde(default)]
    pub smoke: Vec<Vec<String>>,
}

fn default_sources() -> Vec<PathBuf> {
    vec![PathBuf::from(".")]
}

fn default_build() -> Vec<String> {
    vec!["stellar".into(), "contract".into(), "build".into()]
}

#[derive(Args, Debug)]
pub struct DevArgs {
    #[arg(long, default_value = "starforge-dev.toml")]
    pub manifest: PathBuf,
    #[arg(long)]
    pub once: bool,
    #[arg(long, default_value_t = 300)]
    pub debounce_ms: u64,
    #[arg(long, default_value_t = 200)]
    pub interval_ms: u64,
}

pub async fn handle(args: DevArgs) -> Result<()> {
    let manifest_path = fs::canonicalize(&args.manifest)
        .with_context(|| format!("Dev manifest not found: {}", args.manifest.display()))?;
    let root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let manifest: DevManifest = toml::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("Invalid dev manifest: {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;

    let sources = resolve_sources(&root, &manifest.sources);
    let mut snapshot = snapshot_sources(&sources)?;
    run_cycle(&root, &manifest)?;
    if args.once {
        return Ok(());
    }

    let (stop_tx, stop_rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })
    .context("failed to install Ctrl-C handler")?;

    println!(
        "Watching {} source path(s). Press Ctrl-C to stop.",
        sources.len()
    );
    loop {
        if stop_rx.try_recv().is_ok() {
            println!("\nStopped.");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(args.interval_ms.max(25)));
        let current = snapshot_sources(&sources)?;
        if current == snapshot {
            continue;
        }

        std::thread::sleep(Duration::from_millis(args.debounce_ms));
        let settled = snapshot_sources(&sources)?;
        snapshot = settled.clone();
        if settled != current {
            continue;
        }
        clear_terminal();
        if let Err(error) = run_cycle(&root, &manifest) {
            eprintln!("[ERROR] {error:#}");
        }
    }
}

fn validate_manifest(manifest: &DevManifest) -> Result<()> {
    if manifest.build.is_empty() || manifest.build[0].trim().is_empty() {
        anyhow::bail!("dev manifest build command cannot be empty");
    }
    if manifest
        .deploy
        .as_ref()
        .is_some_and(|command| command.is_empty() || command[0].trim().is_empty())
    {
        anyhow::bail!("dev manifest deploy command cannot be empty");
    }
    for (index, command) in manifest.smoke.iter().enumerate() {
        if command.is_empty() || command[0].trim().is_empty() {
            anyhow::bail!("smoke command {} cannot be empty", index + 1);
        }
    }
    Ok(())
}

fn resolve_sources(root: &Path, sources: &[PathBuf]) -> Vec<PathBuf> {
    sources
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .collect()
}

fn snapshot_sources(sources: &[PathBuf]) -> Result<BTreeMap<String, (u128, u64)>> {
    let mut files = BTreeMap::new();
    for source in sources {
        if source.is_file() {
            record_file(source, &mut files)?;
        } else if source.is_dir() {
            collect_files(source, &mut files)?;
        } else {
            anyhow::bail!("Watched source does not exist: {}", source.display());
        }
    }
    Ok(files)
}

fn collect_files(dir: &Path, files: &mut BTreeMap<String, (u128, u64)>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() {
            record_file(&path, files)?;
        }
    }
    Ok(())
}

fn record_file(path: &Path, files: &mut BTreeMap<String, (u128, u64)>) -> Result<()> {
    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    files.insert(
        path.to_string_lossy().into_owned(),
        (modified, metadata.len()),
    );
    Ok(())
}

fn run_cycle(root: &Path, manifest: &DevManifest) -> Result<()> {
    let command_count = 1 + if manifest.deploy.is_some() { 1 } else { 0 } + manifest.smoke.len();
    let reporter = ProgressReporter::new(command_count);
    let mut index = 1;

    run_step(&reporter, index, root, &manifest.build, "build")?;
    index += 1;
    if let Some(deploy) = &manifest.deploy {
        run_step(&reporter, index, root, deploy, "deploy")?;
        index += 1;
        for (smoke_index, smoke) in manifest.smoke.iter().enumerate() {
            run_step(
                &reporter,
                index,
                root,
                smoke,
                &format!("smoke-{}", smoke_index + 1),
            )?;
            index += 1;
        }
    }
    Ok(())
}

fn run_step(
    reporter: &ProgressReporter,
    index: usize,
    root: &Path,
    argv: &[String],
    label: &str,
) -> Result<()> {
    reporter.started(index, label);
    let started = Instant::now();
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to start {} command", label))?;
    let elapsed = started.elapsed();
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if detail.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            detail
        };
        reporter.failed(index, label, detail.clone());
        anyhow::bail!("{} failed after {:.2?}: {}", label, elapsed, detail);
    }
    reporter.completed(index, label, format!("completed in {:.2?}", elapsed));
    Ok(())
}

fn clear_terminal() {
    if std::io::stdout().is_terminal() {
        print!("\x1b[2J\x1b[H");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validates_commands_and_defaults_sources() {
        let manifest: DevManifest = toml::from_str("build = [\"echo\", \"ok\"]").unwrap();
        assert_eq!(manifest.sources, vec![PathBuf::from(".")]);
        assert_eq!(manifest.build, vec!["echo", "ok"]);
        validate_manifest(&manifest).unwrap();
    }

    #[test]
    fn snapshot_detects_file_size_changes() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("contract.rs");
        fs::write(&file, "one").unwrap();
        let first = snapshot_sources(std::slice::from_ref(&file)).unwrap();
        fs::write(&file, "two-two").unwrap();
        let second = snapshot_sources(std::slice::from_ref(&file)).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn runs_argv_without_a_shell() {
        let dir = TempDir::new().unwrap();
        let manifest = DevManifest {
            sources: vec![PathBuf::from(".")],
            build: vec!["true".into()],
            deploy: None,
            smoke: vec![],
        };
        run_cycle(dir.path(), &manifest).unwrap();
    }
}
