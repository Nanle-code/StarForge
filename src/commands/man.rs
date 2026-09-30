use anyhow::{bail, Result};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Subcommands for man page generation
#[derive(clap::Subcommand)]
pub enum ManCommand {
    /// Print a man page to stdout for piping into man(1)
    Generate {
        /// Which man page: "starforge" (default) or a subcommand name like "wallet"
        #[arg(default_value = "starforge")]
        page: String,
    },

    /// Write all man pages to a directory
    Install {
        /// Target directory for .1 files (default: /usr/local/share/man/man1)
        #[arg(long)]
        directory: Option<PathBuf>,
    },

    /// List available man pages
    List,
}

/// Known subcommands that have man pages (must match build.rs SUBCOMMAND_INFO).
///
/// Paths follow the noun-verb tree (ADR 0007): `contract test` ->
/// `starforge-contract-test.1`.
const KNOWN_PAGES: &[&str] = &[
    "wallet",
    "wallet tx",
    "wallet auth",
    "wallet multisig",
    "wallet diagnostics",
    "contract",
    "contract storage",
    "contract debug",
    "contract repl",
    "contract test",
    "contract audit",
    "contract security",
    "contract governance",
    "contract upgrade",
    "contract verify",
    "contract migrate",
    "contract generate",
    "contract complete",
    "contract explain",
    "contract lint",
    "contract optimize",
    "contract gas",
    "contract metrics",
    "contract profile",
    "contract benchmark",
    "contract docs",
    "contract mutate",
    "contract monitor",
    "contract health",
    "deploy",
    "deploy run",
    "deploy history",
    "deploy env",
    "deploy schedule",
    "deploy orchestrate",
    "deploy pipeline",
    "deploy approval",
    "deploy cost",
    "deploy analytics",
    "deploy backup",
    "network",
    "network node",
    "network simulate",
    "network snapshot",
    "template",
    "template vcs",
    "template registry",
    "plugin",
    "ai",
    "ai local",
    "ai debug",
    "ai navigate",
    "ai gate",
    "ai profiling",
    "ai ide",
    "ai tests",
    "ai test-maintain",
    "ai deploy-test",
    "ai property-test",
    "ai feedback",
    "ai search",
    "ai recommend",
    "ai route",
    "ai plan",
    "ai accessibility",
    "ai suggest",
    "ai docs",
    "ai telemetry",
    "ai training",
    "ai security-audit",
    "ai prompts",
    "ai help",
    "config",
    "config info",
    "config telemetry",
    "config flags",
    "config privacy",
    "project",
    "project new",
    "project collab",
    "tool",
    "tool tutorial",
    "tool nl",
    "tool pr",
    "tool bug-report",
    "completions",
];

fn man_dir() -> Option<PathBuf> {
    // When installed via `starforge man install`, man pages live in
    // the same directory as the binary under ../share/starforge/man.
    // For development, they live in the workspace `man/` directory.
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;

    // Check for man pages next to the binary first (release install)
    let candidate = exe_dir.join("man");
    if candidate.join("starforge.1").exists() {
        return Some(candidate);
    }

    // Check one level up (e.g., /usr/local/bin -> /usr/local/share/starforge/man)
    let candidate = exe_dir
        .join("..")
        .join("share")
        .join("starforge")
        .join("man");
    if candidate.join("starforge.1").exists() {
        return Some(candidate);
    }

    // Fallback: workspace man/ directory (development builds)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let dev_man = Path::new(manifest_dir).join("man");
    if dev_man.join("starforge.1").exists() {
        return Some(dev_man);
    }

    None
}

pub async fn handle(cmd: ManCommand) -> Result<()> {
    match cmd {
        ManCommand::Generate { page } => handle_generate(&page),
        ManCommand::Install { directory } => handle_install(directory.as_deref()),
        ManCommand::List => handle_list(),
    }
}

/// Read a pre-generated man page by name. Used by the `starforge man generate` subcommand.
///
/// This function is public so integration tests can exercise man page content
/// without spawning the full binary (which has a pre-existing stack overflow
/// issue on Windows due to the 60+ deep subcommand tree).
pub fn read_man_page(page: &str) -> Result<String> {
    let man_dir = man_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "Man pages not found. Build the project first (cargo build) \
             or install man pages with: starforge man install"
        )
    })?;

    // Page names are CLI paths (`contract test`); page files use dashes.
    let filename = if page == "starforge" {
        "starforge.1".to_string()
    } else {
        format!("starforge-{}.1", page.replace(' ', "-"))
    };

    let path = man_dir.join(&filename);
    if !path.exists() {
        let mut available = vec!["starforge".to_string()];
        available.extend(KNOWN_PAGES.iter().map(|s| s.to_string()));
        bail!(
            "Unknown man page '{}'. Available pages: {}",
            page,
            available.join(", ")
        );
    }

    let mut contents = String::new();
    std::fs::File::open(&path)
        .and_then(|mut f| f.read_to_string(&mut contents))
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;

    Ok(contents)
}

fn handle_generate(page: &str) -> Result<()> {
    let contents = read_man_page(page)?;
    io::stdout().write_all(contents.as_bytes())?;
    Ok(())
}

fn handle_list() -> Result<()> {
    let mut pages = vec!["starforge".to_string()];
    pages.extend(KNOWN_PAGES.iter().map(|s| s.to_string()));
    pages.sort();

    println!("Available man pages:");
    for page in &pages {
        println!("  starforge-{}(1)", page.replace(' ', "-"));
    }
    println!("\nUsage: starforge man generate <page> | man -l -");
    println!("       starforge man generate {} | man -l -", pages[0]);
    Ok(())
}

fn handle_install(dir: Option<&std::path::Path>) -> Result<()> {
    let out_dir = match dir {
        Some(d) => d.to_path_buf(),
        None => {
            if cfg!(target_os = "windows") {
                bail!(
                    "No default man page directory on Windows. \
                     Use --directory to specify a target path."
                );
            }
            PathBuf::from("/usr/local/share/man/man1")
        }
    };

    let source_dir = man_dir().ok_or_else(|| {
        anyhow::anyhow!("Man pages not found. Build the project first (cargo build).")
    })?;

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {}", out_dir.display(), e))?;

    let mut installed = Vec::new();

    // Copy all .1 files from the source man directory
    for entry in std::fs::read_dir(&source_dir)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", source_dir.display(), e))?
    {
        let entry = entry.map_err(|e| anyhow::anyhow!("Dir entry error: {}", e))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("1") {
            let dest = out_dir.join(path.file_name().unwrap());
            std::fs::copy(&path, &dest).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to copy {} to {}: {}",
                    path.display(),
                    dest.display(),
                    e
                )
            })?;
            installed.push(dest);
        }
    }

    if installed.is_empty() {
        bail!(
            "No man pages found in {}. Build the project first.",
            source_dir.display()
        );
    }

    println!(
        "Installed {} man page(s) to {}:",
        installed.len(),
        out_dir.display()
    );
    for p in &installed {
        println!("  {}", p.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_main_man_page_works() {
        let result = read_man_page("starforge");
        assert!(
            result.is_ok(),
            "should read main man page: {:?}",
            result.err()
        );
        let contents = result.unwrap();
        assert!(contents.contains(".SH NAME"), "must contain NAME section");
    }

    #[test]
    fn read_wallet_man_page_works() {
        let result = read_man_page("wallet");
        assert!(
            result.is_ok(),
            "should read wallet man page: {:?}",
            result.err()
        );
        let contents = result.unwrap();
        assert!(
            contents.contains("starforge-wallet"),
            "must reference starforge-wallet"
        );
    }

    #[test]
    fn read_invalid_page_errors() {
        let result = read_man_page("nonexistent-cmd-xyz");
        assert!(result.is_err(), "should fail for unknown page");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Unknown man page"),
            "error must mention unknown page, got: {}",
            err_msg
        );
    }
}
