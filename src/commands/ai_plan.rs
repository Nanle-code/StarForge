//! CLI for AI Project Planning Assistant (issue #517).

use crate::utils::ai_project_planner as planner;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiPlanCommands {
    /// Analyze project requirements from a description
    Analyze(AnalyzeArgs),

    /// Suggest contract architectures
    Architecture(ArchitectureArgs),

    /// Generate detailed task breakdown
    Breakdown(BreakdownArgs),

    /// Estimate project timeline with milestones
    Timeline(TimelineArgs),

    /// Plan team resources and allocation
    Resources(ResourcesArgs),

    /// Identify project risks and mitigations
    Risks(RisksArgs),

    /// Generate a complete project plan
    Generate(GenerateArgs),

    /// List saved project plans
    List,

    /// Show a saved plan
    Show(ShowArgs),
}

#[derive(Args)]
pub struct AnalyzeArgs {
    /// Project description or requirements
    pub description: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ArchitectureArgs {
    pub description: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct BreakdownArgs {
    pub description: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct TimelineArgs {
    pub description: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ResourcesArgs {
    pub description: String,

    /// Team size override
    #[arg(long)]
    pub team_size: Option<u32>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct RisksArgs {
    pub description: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Project name
    pub name: String,

    /// Project description / requirements
    pub description: String,

    /// Enhance plan with AI (requires Ollama)
    #[arg(long, default_value_t = false)]
    pub use_ai: bool,

    /// Ollama model for AI enhancement
    #[arg(long, default_value = ollama::DEFAULT_MODEL)]
    pub model: String,

    /// Save plan to disk
    #[arg(long, default_value_t = true)]
    pub save: bool,

    #[arg(long)]
    pub json: bool,

    /// Write plan to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct ShowArgs {
    /// Path to saved plan JSON
    pub path: PathBuf,

    #[arg(long)]
    pub json: bool,
}

pub async fn handle(cmd: AiPlanCommands) -> Result<()> {
    match cmd {
        AiPlanCommands::Analyze(args) => handle_analyze(args),
        AiPlanCommands::Architecture(args) => handle_architecture(args),
        AiPlanCommands::Breakdown(args) => handle_breakdown(args),
        AiPlanCommands::Timeline(args) => handle_timeline(args),
        AiPlanCommands::Resources(args) => handle_resources(args),
        AiPlanCommands::Risks(args) => handle_risks(args),
        AiPlanCommands::Generate(args) => handle_generate(args).await,
        AiPlanCommands::List => handle_list(),
        AiPlanCommands::Show(args) => handle_show(args),
    }
}

fn handle_analyze(args: AnalyzeArgs) -> Result<()> {
    let reqs = planner::analyze_requirements(&args.description);
    output_or_print(&reqs, args.json, "Requirement Analysis", |r| {
        p::kv("Summary", &r.summary);
        println!();
        p::info("Functional requirements:");
        for req in &r.functional_requirements {
            println!("  • {}", req);
        }
        println!();
        p::info("Non-functional requirements:");
        for req in &r.non_functional_requirements {
            println!("  • {}", req);
        }
        println!();
        p::info("Constraints:");
        for c in &r.constraints {
            println!("  • {}", c);
        }
        println!();
        p::info("Success criteria:");
        for s in &r.success_criteria {
            println!("  • {}", s);
        }
    })
}

fn handle_architecture(args: ArchitectureArgs) -> Result<()> {
    let archs = planner::suggest_architectures(&args.description);
    output_or_print(&archs, args.json, "Architecture Suggestions", |archs| {
        for arch in archs {
            let marker = if arch.recommended {
                " ★ recommended"
            } else {
                ""
            };
            println!();
            println!("  {}{}", arch.name, marker);
            println!("  {}", arch.description);
            println!("  Storage: {}", arch.storage_strategy);
            println!("  Auth: {}", arch.auth_model);
            println!("  Upgrade: {}", arch.upgrade_strategy);
            if !arch.pros.is_empty() {
                println!("  Pros:");
                for p_item in &arch.pros {
                    println!("    + {}", p_item);
                }
            }
            if !arch.cons.is_empty() {
                println!("  Cons:");
                for c in &arch.cons {
                    println!("    - {}", c);
                }
            }
        }
    })
}

fn handle_breakdown(args: BreakdownArgs) -> Result<()> {
    let phases = planner::default_phases(&args.description);
    let tasks = planner::breakdown_tasks(&args.description, &phases);
    output_or_print(&tasks, args.json, "Task Breakdown", |tasks| {
        let headers = &["ID", "Title", "Phase", "Priority", "Effort", "Role"];
        let rows: Vec<Vec<String>> = tasks
            .iter()
            .map(|t| {
                vec![
                    t.id.clone(),
                    t.title.clone(),
                    t.phase.clone(),
                    format!("{:?}", t.priority),
                    t.effort_points.to_string(),
                    t.assignee_role.clone(),
                ]
            })
            .collect();
        p::table(headers, &rows);
    })
}

fn handle_timeline(args: TimelineArgs) -> Result<()> {
    let phases = planner::default_phases(&args.description);
    let tasks = planner::breakdown_tasks(&args.description, &phases);
    let timeline = planner::estimate_timeline(&tasks, &phases);
    output_or_print(&timeline, args.json, "Timeline Estimate", |t| {
        p::kv("Total days", &t.total_days.to_string());
        p::kv("Buffer days", &t.buffer_days.to_string());
        p::kv("Start", &t.start_date.format("%Y-%m-%d").to_string());
        p::kv(
            "Target completion",
            &t.target_completion.format("%Y-%m-%d").to_string(),
        );
        println!();
        p::info("Milestones:");
        for m in &t.milestones {
            println!(
                "  • {} — {} ({})",
                m.name,
                m.date.format("%Y-%m-%d"),
                m.deliverables.join(", ")
            );
        }
        if !t.critical_path.is_empty() {
            println!();
            p::info("Critical path:");
            println!("  {}", t.critical_path.join(" → "));
        }
    })
}

fn handle_resources(args: ResourcesArgs) -> Result<()> {
    let phases = planner::default_phases(&args.description);
    let tasks = planner::breakdown_tasks(&args.description, &phases);
    let resources = planner::plan_resources(&tasks, args.team_size);
    output_or_print(&resources, args.json, "Resource Plan", |r| {
        p::kv("Team size", &r.team_size.to_string());
        println!();
        p::info("Roles:");
        for role in &r.roles {
            println!("  • {} (×{})", role.role, role.count);
            for resp in &role.responsibilities {
                println!("      - {}", resp);
            }
        }
        println!();
        p::info("Skills required:");
        for s in &r.skills_required {
            println!("  • {}", s);
        }
        println!();
        p::info("Tooling:");
        for t in &r.tooling {
            println!("  • {}", t);
        }
    })
}

fn handle_risks(args: RisksArgs) -> Result<()> {
    let risks = planner::identify_risks(&args.description);
    output_or_print(&risks, args.json, "Risk Assessment", |risks| {
        let headers = &["ID", "Title", "Category", "Severity", "Likelihood"];
        let rows: Vec<Vec<String>> = risks
            .iter()
            .map(|r| {
                vec![
                    r.id.clone(),
                    r.title.clone(),
                    format!("{:?}", r.category),
                    format!("{:?}", r.severity),
                    format!("{:?}", r.likelihood),
                ]
            })
            .collect();
        p::table(headers, &rows);
        println!();
        for r in risks {
            println!("  {} — {}", r.id, r.title);
            println!("    Mitigation: {}", r.mitigation);
            println!("    Contingency: {}", r.contingency);
            println!();
        }
    })
}

async fn handle_generate(args: GenerateArgs) -> Result<()> {
    p::header(&format!("Project Plan — {}", args.name));
    p::separator();

    let mut plan = planner::generate_plan(&args.name, &args.description);

    if args.use_ai {
        if ollama::is_ollama_running().await {
            let spinner = p::spinner("Enhancing plan with AI…");
            planner::enhance_plan_with_ai(&mut plan, &args.model)
                .await
                .context("AI enhancement failed")?;
            spinner.finish_and_clear();
            p::success("AI enhancement complete.");
        } else {
            p::warn("Ollama not running — skipping AI enhancement.");
        }
    }

    if args.save {
        let path = planner::save_plan(&plan)?;
        p::success(&format!("Plan saved → {}", path.display()));
    }

    if let Some(out) = &args.out {
        std::fs::write(out, serde_json::to_string_pretty(&plan)?)?;
        p::success(&format!("Plan written → {}", out.display()));
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    print_plan_summary(&plan);
    p::separator();
    Ok(())
}

fn handle_list() -> Result<()> {
    let plans = planner::list_saved_plans()?;
    p::header("Saved Project Plans");
    p::separator();

    if plans.is_empty() {
        p::info("No saved plans found.");
        p::info("Generate one with: starforge ai-plan generate <name> \"<description>\"");
    } else {
        for path in &plans {
            println!("  • {}", path.display());
        }
        p::success(&format!("{} plan(s) found.", plans.len()));
    }

    p::separator();
    Ok(())
}

fn handle_show(args: ShowArgs) -> Result<()> {
    let plan = planner::load_plan(&args.path)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    p::header(&format!("Project Plan — {}", plan.project_name));
    p::separator();
    print_plan_summary(&plan);

    if let Some(summary) = &plan.ai_summary {
        println!();
        p::info("AI Recommendations");
        println!("{}", summary);
    }

    p::separator();
    Ok(())
}

fn print_plan_summary(plan: &planner::ProjectPlan) {
    p::kv(
        "Generated",
        &plan.generated_at.format("%Y-%m-%d %H:%M UTC").to_string(),
    );
    p::kv("Tasks", &plan.tasks.len().to_string());
    p::kv("Phases", &plan.phases.len().to_string());
    p::kv("Risks", &plan.risks.len().to_string());
    p::kv("Timeline", &format!("{} days", plan.timeline.total_days));
    p::kv("Team size", &plan.resources.team_size.to_string());

    let recommended = plan.architectures.iter().find(|a| a.recommended);
    if let Some(arch) = recommended {
        p::kv("Recommended architecture", &arch.name);
    }

    println!();
    p::info("Phases:");
    for phase in &plan.phases {
        println!(
            "  {}. {} ({} days) — {}",
            phase.order, phase.name, phase.estimated_days, phase.description
        );
    }

    println!();
    p::info("Top risks:");
    for risk in plan.risks.iter().take(3) {
        println!("  • [{}] {} — {}", risk.id, risk.title, risk.mitigation);
    }
}

fn output_or_print<T: serde::Serialize>(
    data: &T,
    json: bool,
    title: &str,
    print_fn: impl FnOnce(&T),
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(data)?);
    } else {
        p::header(title);
        p::separator();
        print_fn(data);
        p::separator();
    }
    Ok(())
}
