use crate::utils::{print as p, tutorial_engine};
use anyhow::Result;
use clap::Subcommand;
use colored::*;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum TutorialCommands {
    /// List available tutorials
    List,
    /// Start a tutorial by slug (e.g. hello-world)
    Start {
        slug: String,
        /// Run tutorial in offline demo mode using local network stubs (non-production)
        #[arg(long)]
        demo: bool,
    },
    /// Advance to the next tutorial step
    Next,
    /// Show current tutorial status
    Status,
}

pub async fn handle(cmd: TutorialCommands) -> Result<()> {
    match cmd {
        TutorialCommands::List => list(),
        TutorialCommands::Start { slug, demo } => start(slug, demo),
        TutorialCommands::Next => next(),
        TutorialCommands::Status => status(),
    }
}

fn repo_root() -> Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

fn list() -> Result<()> {
    let root = repo_root()?;
    p::header("Tutorials");

    let slugs = tutorial_engine::list_tutorial_slugs(&root)?;
    if slugs.is_empty() {
        p::info("No tutorials installed yet.");
        return Ok(());
    }

    p::separator();
    for (i, slug) in slugs.iter().enumerate() {
        let title = tutorial_engine::load_tutorial(&root, slug)
            .map(|t| t.title)
            .unwrap_or_else(|_| slug.clone());
        println!(
            "  {:>2}. {} — {}",
            i + 1,
            slug.cyan().bold(),
            title.dimmed()
        );
    }
    p::separator();
    p::info(
        "Start with: starforge tool tutorial start hello-world (or add --demo for offline mode)",
    );
    Ok(())
}

fn start(slug: String, demo: bool) -> Result<()> {
    let root = repo_root()?;
    let tutorial = tutorial_engine::load_tutorial(&root, &slug)?;

    let mut status = tutorial_engine::load_status()?;
    status.active = Some(slug.clone());
    status.started_at = Some(chrono::Utc::now().to_rfc3339());
    status.current_step = 0;
    status.completed_steps.clear();
    status.demo_mode = demo;
    tutorial_engine::save_status(&status)?;

    if demo {
        println!("{}", tutorial_engine::DEMO_MODE_BANNER.yellow().bold());
    }

    p::header(&format!("Tutorial: {}", tutorial.title));
    if let Some(desc) = &tutorial.description {
        println!("  {}", desc.dimmed());
    }
    p::separator();
    print_current_step(&tutorial, &status);
    p::info("Advance with: starforge tool tutorial next");
    p::info("Track progress with: starforge tool tutorial status");
    Ok(())
}

fn next() -> Result<()> {
    let root = repo_root()?;
    let mut status = tutorial_engine::load_status()?;
    let slug = status.active.clone().ok_or_else(|| {
        anyhow::anyhow!("No active tutorial. Run starforge tool tutorial start <slug>")
    })?;
    let tutorial = tutorial_engine::load_tutorial(&root, &slug)?;
    let step = tutorial.steps.get(status.current_step).ok_or_else(|| {
        anyhow::anyhow!(
            "Tutorial progress is invalid; restart with starforge tool tutorial start {}",
            slug
        )
    })?;

    if let Some(checkpoint) = &step.checkpoint {
        if let Err(error) = tutorial_engine::verify_checkpoint(checkpoint, &root) {
            let repair_hint = step
                .repair_hint
                .as_deref()
                .unwrap_or("Complete the command shown for this step, then retry.");
            anyhow::bail!(
                "Checkpoint '{}' failed: {}\nRepair hint: {}",
                step.title,
                error,
                repair_hint
            );
        }
    }

    if !status.completed_steps.contains(&status.current_step) {
        status.completed_steps.push(status.current_step);
    }

    if status.current_step + 1 >= tutorial.steps.len() {
        if status.demo_mode {
            println!("{}", tutorial_engine::DEMO_MODE_BANNER.yellow().bold());
        }
        p::success("Tutorial complete! You reached the final milestone.");
        if let Some(elapsed) = elapsed_seconds(&status) {
            p::kv("Elapsed", &format_elapsed(elapsed));
        }
        status.active = None;
        status.current_step = 0;
        tutorial_engine::save_status(&status)?;
        return Ok(());
    }

    status.current_step += 1;
    tutorial_engine::save_status(&status)?;

    if status.demo_mode {
        println!("{}", tutorial_engine::DEMO_MODE_BANNER.yellow().bold());
    }

    p::header(&format!("Tutorial: {}", tutorial.title));
    print_current_step(&tutorial, &status);
    p::info(
        "Run the suggested command in your terminal, then `starforge tool tutorial next` again.",
    );
    Ok(())
}

fn status() -> Result<()> {
    let root = repo_root()?;
    let status = tutorial_engine::load_status()?;
    p::header("Tutorial Status");

    match status.active {
        Some(ref active) => {
            let tutorial = tutorial_engine::load_tutorial(&root, active)?;
            if status.demo_mode {
                println!("{}", tutorial_engine::DEMO_MODE_BANNER.yellow().bold());
            }
            p::kv_accent("Active", active);
            p::kv(
                "Mode",
                if status.demo_mode {
                    "Demo (offline stubs)"
                } else {
                    "Standard (live network)"
                },
            );
            if let Some(ts) = &status.started_at {
                p::kv("Started", ts);
            }
            if let Some(elapsed) = elapsed_seconds(&status) {
                p::kv("Elapsed", &format_elapsed(elapsed));
            }
            p::kv(
                "Progress",
                &format!(
                    "step {} of {} ({} completed)",
                    status.current_step + 1,
                    tutorial.steps.len(),
                    status.completed_steps.len()
                ),
            );
            p::separator();
            print_current_step(&tutorial, &status);
        }
        None => {
            p::info(&format!(
                "No active tutorial. Start one with: {}",
                "starforge tool tutorial start hello-world".cyan()
            ));
        }
    }
    Ok(())
}

fn elapsed_seconds(status: &tutorial_engine::TutorialStatus) -> Option<i64> {
    let started_at = chrono::DateTime::parse_from_rfc3339(status.started_at.as_deref()?).ok()?;
    Some(
        chrono::Utc::now()
            .signed_duration_since(started_at.with_timezone(&chrono::Utc))
            .num_seconds()
            .max(0),
    )
}

fn format_elapsed(seconds: i64) -> String {
    format!("{}m {:02}s", seconds / 60, seconds % 60)
}

fn print_current_step(
    tutorial: &tutorial_engine::TutorialDefinition,
    status: &tutorial_engine::TutorialStatus,
) {
    if status.demo_mode {
        println!(
            "  {}",
            "[DEMO STUBS ACTIVE — NO LIVE FUNDS REQUIRED]".yellow()
        );
    }
    let step_index = status
        .current_step
        .min(tutorial.steps.len().saturating_sub(1));
    let step = &tutorial.steps[step_index];
    let body = tutorial_engine::render_step(step, step_index, tutorial.steps.len());
    for line in body.lines() {
        println!("  {}", line.white());
    }
}
