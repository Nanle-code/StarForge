use crate::utils::approval_engine::{
    self, approve_request, build_default_workflows, cancel_request, create_request,
    create_workflow, deactivate_workflow, get_approval_summary, get_request, list_requests,
    list_workflows, reject_request, ApprovalLevel, ApprovalStatus,
};
use crate::utils::compliance::{build_default_policies, run_compliance_checks};
use crate::utils::notifications::{
    send_approval_completed_notification, send_approval_requested_notification,
};
use crate::utils::print as p;
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::collections::HashMap;

#[derive(Subcommand)]
pub enum ApprovalCommands {
    /// Initialize default workflows and policies
    Init,
    /// Manage approval workflows
    #[command(subcommand)]
    Workflow(WorkflowCommands),
    /// Manage approval requests
    #[command(subcommand)]
    Request(RequestCommands),
    /// Show approval dashboard
    Dashboard,
}

#[derive(Subcommand)]
pub enum WorkflowCommands {
    /// Create a new approval workflow
    Create(CreateWorkflowArgs),
    /// List all approval workflows
    List,
    /// Show workflow details
    Show(ShowWorkflowArgs),
    /// Deactivate a workflow
    Deactivate(DeactivateWorkflowArgs),
}

#[derive(Subcommand)]
pub enum RequestCommands {
    /// Create a new approval request for deployment
    Create(CreateRequestArgs),
    /// List approval requests
    List(ListRequestsArgs),
    /// Show request details
    Show(ShowRequestArgs),
    /// Approve a request at current level
    Approve(ApproveRequestArgs),
    /// Reject a request
    Reject(RejectRequestArgs),
    /// Cancel a request
    Cancel(CancelRequestArgs),
}

#[derive(Args)]
pub struct CreateWorkflowArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub description: String,
    #[arg(long, value_delimiter = ',')]
    pub levels: Vec<String>,
    #[arg(long, default_value = "1")]
    pub required_approvers: u8,
    #[arg(long, value_delimiter = ',', default_value = "developer")]
    pub approver_roles: Vec<String>,
    #[arg(long)]
    pub timeout_hours: Option<u64>,
}

#[derive(Args)]
pub struct ShowWorkflowArgs {
    #[arg(long)]
    pub id: String,
}

#[derive(Args)]
pub struct DeactivateWorkflowArgs {
    #[arg(long)]
    pub id: String,
}

#[derive(Args)]
pub struct CreateRequestArgs {
    #[arg(long)]
    pub workflow_id: String,
    #[arg(long)]
    pub contract_id: String,
    #[arg(long)]
    pub wasm_path: String,
    #[arg(long)]
    pub wasm_hash: String,
    #[arg(long, default_value = "testnet")]
    pub network: String,
    #[arg(long)]
    pub description: String,
    #[arg(long)]
    pub requested_by: String,
}

#[derive(Args)]
pub struct ListRequestsArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub network: Option<String>,
    #[arg(long, default_value = "false")]
    pub json: bool,
}

#[derive(Args)]
pub struct ShowRequestArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long, default_value = "false")]
    pub json: bool,
}

#[derive(Args)]
pub struct ApproveRequestArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub approver: String,
    #[arg(long)]
    pub comment: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub roles: Vec<String>,
}

#[derive(Args)]
pub struct RejectRequestArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub approver: String,
    #[arg(long)]
    pub reason: String,
    #[arg(long, value_delimiter = ',')]
    pub roles: Vec<String>,
}

#[derive(Args)]
pub struct CancelRequestArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub cancelled_by: String,
    #[arg(long)]
    pub reason: String,
}

pub async fn handle(cmd: ApprovalCommands) -> Result<()> {
    match cmd {
        ApprovalCommands::Init => handle_init(),
        ApprovalCommands::Workflow(sub) => match sub {
            WorkflowCommands::Create(args) => handle_create_workflow(args),
            WorkflowCommands::List => handle_list_workflows(),
            WorkflowCommands::Show(args) => handle_show_workflow(args),
            WorkflowCommands::Deactivate(args) => handle_deactivate_workflow(args),
        },
        ApprovalCommands::Request(sub) => match sub {
            RequestCommands::Create(args) => handle_create_request(args),
            RequestCommands::List(args) => handle_list_requests(args),
            RequestCommands::Show(args) => handle_show_request(args),
            RequestCommands::Approve(args) => handle_approve_request(args),
            RequestCommands::Reject(args) => handle_reject_request(args),
            RequestCommands::Cancel(args) => handle_cancel_request(args),
        },
        ApprovalCommands::Dashboard => handle_dashboard(),
    }
}

fn handle_init() -> Result<()> {
    p::header("Approval Workflow Initialization");

    let workflows = build_default_workflows()?;
    let policies = build_default_policies()?;

    println!();
    p::success(&format!(
        "Created {} default approval workflows",
        workflows.len()
    ));
    for wf in &workflows {
        println!(
            "  {} {} ({})",
            "•".cyan(),
            wf.name.white(),
            wf.id[..12].dimmed()
        );
        for level in &wf.levels {
            println!(
                "    {} {} — {} approver(s) needed, roles: {:?}",
                "└".dimmed(),
                level.name,
                level.required_approvers,
                level.approver_roles
            );
        }
    }

    println!();
    p::success(&format!(
        "Created {} default compliance policies",
        policies.len()
    ));
    for pol in &policies {
        println!(
            "  {} {} ({})",
            "•".cyan(),
            pol.name.white(),
            pol.severity.to_string().dimmed()
        );
    }

    println!();
    p::info("Run `starforge approval workflow list` to see all workflows.");
    p::info("Run `starforge approval request create --help` to create an approval request.");
    Ok(())
}

fn handle_create_workflow(args: CreateWorkflowArgs) -> Result<()> {
    p::header("Create Approval Workflow");

    let levels = if args.levels.len() == 1 && args.levels[0].contains(',') {
        args.levels[0]
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        args.levels.clone()
    };

    if levels.is_empty() {
        anyhow::bail!("At least one level name is required. Use --levels level1,level2,...");
    }

    let approval_levels: Vec<ApprovalLevel> = levels
        .iter()
        .map(|name| ApprovalLevel {
            name: name.to_string(),
            description: format!("Approval by {}", name),
            required_approvers: args.required_approvers,
            approver_roles: args.approver_roles.clone(),
            timeout_hours: args.timeout_hours,
        })
        .collect();

    let workflow = create_workflow(&args.name, &args.description, approval_levels)?;

    println!();
    p::kv_accent("Workflow ID", &workflow.id);
    p::kv("Name", &workflow.name);
    p::kv("Description", &workflow.description);
    p::kv("Levels", &workflow.levels.len().to_string());
    p::success("Workflow created successfully");
    Ok(())
}

fn handle_list_workflows() -> Result<()> {
    p::header("Approval Workflows");

    let workflows = list_workflows(false)?;

    if workflows.is_empty() {
        p::info("No workflows found. Create one with `starforge approval workflow create`");
        p::info("Or run `starforge approval init` to create default workflows.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<18} {:<30} {:<10} {:<10} {}",
        "ID".dimmed(),
        "Name".dimmed(),
        "Levels".dimmed(),
        "Active".dimmed(),
        "Created".dimmed(),
    );
    println!("  {}", "─".repeat(80).dimmed());

    for wf in &workflows {
        let active_mark = if wf.active {
            "✓".green()
        } else {
            "✗".red()
        };
        let created = wf.created_at.get(..10).unwrap_or(&wf.created_at);
        println!(
            "  {:<18} {:<30} {:<10} {:<10} {}",
            wf.id[..12].cyan(),
            wf.name.white(),
            wf.levels.len(),
            active_mark,
            created.dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_show_workflow(args: ShowWorkflowArgs) -> Result<()> {
    p::header("Workflow Details");

    let workflow = approval_engine::get_workflow(&args.id)?
        .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found", args.id))?;

    println!();
    p::kv_accent("ID", &workflow.id);
    p::kv("Name", &workflow.name);
    p::kv("Description", &workflow.description);
    p::kv("Active", if workflow.active { "yes" } else { "no" });
    p::kv(
        "Created",
        workflow
            .created_at
            .get(..19)
            .unwrap_or(&workflow.created_at),
    );
    println!();

    p::separator();
    println!(
        "  {} {}",
        "Approval Levels".bright_white(),
        format!("({})", workflow.levels.len()).dimmed()
    );
    p::separator();

    for (i, level) in workflow.levels.iter().enumerate() {
        println!();
        println!(
            "  {} {}",
            "Level".dimmed(),
            (i + 1).to_string().white().bold()
        );
        println!("    {} {}", "Name:        ".dimmed(), level.name.white());
        println!("    {} {}", "Description: ".dimmed(), level.description);
        println!(
            "    {} {} approver(s)",
            "Required:    ".dimmed(),
            level.required_approvers
        );
        println!(
            "    {} {:?}",
            "Roles:       ".dimmed(),
            level.approver_roles
        );
        if let Some(timeout) = level.timeout_hours {
            println!("    {} {} hours", "Timeout:     ".dimmed(), timeout);
        }
    }
    println!();
    p::separator();
    Ok(())
}

fn handle_deactivate_workflow(args: DeactivateWorkflowArgs) -> Result<()> {
    deactivate_workflow(&args.id)?;
    p::success(&format!("Workflow '{}' deactivated", args.id));
    Ok(())
}

fn handle_create_request(args: CreateRequestArgs) -> Result<()> {
    p::header("Create Approval Request");

    let workflow = approval_engine::get_workflow(&args.workflow_id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Workflow '{}' not found. Use `starforge approval workflow list`",
            args.workflow_id
        )
    })?;

    let metadata = HashMap::new();

    let request = create_request(
        &args.workflow_id,
        &args.contract_id,
        &args.wasm_path,
        &args.wasm_hash,
        &args.network,
        &args.description,
        &args.requested_by,
        metadata,
    )?;

    send_approval_requested_notification(
        &request.id,
        &request.contract_id,
        &request.network,
        &request.requested_by,
        &workflow.levels[0].name,
    )?;

    let compliance = run_compliance_checks(
        &request.id,
        &request.contract_id,
        &request.network,
        &request.requested_by,
    )?;

    println!();
    p::kv_accent("Request ID", &request.id);
    p::kv("Workflow", &request.workflow_name);
    p::kv("Contract", &request.contract_id);
    p::kv("Network", &request.network);
    p::kv("Description", &request.description);
    p::kv("Requested by", &request.requested_by);
    p::kv("Status", &request.status.to_string());
    p::kv("Current level", &workflow.levels[0].name);
    println!();

    if compliance.all_passed {
        p::success("Compliance checks passed");
    } else {
        p::warn(&format!(
            "Compliance checks: {} blocking, {} warning(s)",
            compliance.blocking_count, compliance.warning_count
        ));
    }

    println!();
    p::info(&format!(
        "To approve: starforge approval request approve --id {} --approver <name>",
        &request.id[..12]
    ));
    p::success("Approval request created");
    Ok(())
}

fn handle_list_requests(args: ListRequestsArgs) -> Result<()> {
    p::header("Approval Requests");

    let status_filter = args
        .status
        .as_ref()
        .map(|s| match s.to_lowercase().as_str() {
            "pending" => ApprovalStatus::Pending,
            "in_progress" | "inprogress" => ApprovalStatus::InProgress,
            "approved" => ApprovalStatus::Approved,
            "rejected" => ApprovalStatus::Rejected,
            "expired" => ApprovalStatus::Expired,
            "cancelled" => ApprovalStatus::Cancelled,
            _ => ApprovalStatus::Pending,
        });

    let requests = list_requests(status_filter, args.network.as_deref())?;

    if requests.is_empty() {
        p::info("No approval requests found.");
        return Ok(());
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&requests)?);
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<14} {:<18} {:<12} {:<10} {:<12} {}",
        "ID".dimmed(),
        "Contract".dimmed(),
        "Network".dimmed(),
        "Status".dimmed(),
        "Level".dimmed(),
        "Requested".dimmed(),
    );
    println!("  {}", "─".repeat(90).dimmed());

    for req in &requests {
        let status_colored = match req.status {
            ApprovalStatus::Pending => req.status.to_string().yellow().to_string(),
            ApprovalStatus::InProgress => req.status.to_string().cyan().to_string(),
            ApprovalStatus::Approved => req.status.to_string().green().to_string(),
            ApprovalStatus::Rejected => req.status.to_string().red().to_string(),
            ApprovalStatus::Expired => req.status.to_string().dimmed().to_string(),
            ApprovalStatus::Cancelled => req.status.to_string().dimmed().to_string(),
        };
        let created = req.created_at.get(..16).unwrap_or(&req.created_at);
        println!(
            "  {:<14} {:<18} {:<12} {:<10} {:<12} {}",
            req.id[..12].cyan(),
            req.contract_id.chars().take(16).collect::<String>(),
            req.network,
            status_colored,
            req.level_progress(),
            created.dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_show_request(args: ShowRequestArgs) -> Result<()> {
    p::header("Approval Request Details");

    let request = get_request(&args.id)?
        .ok_or_else(|| anyhow::anyhow!("Approval request '{}' not found", args.id))?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&request)?);
        return Ok(());
    }

    let workflow = approval_engine::get_workflow(&request.workflow_id)?;

    println!();
    p::kv_accent("Request ID", &request.id);
    p::kv("Workflow", &request.workflow_name);
    p::kv("Contract ID", &request.contract_id);
    p::kv("WASM Path", &request.wasm_path);
    p::kv("WASM Hash", &request.wasm_hash);
    p::kv("Network", &request.network);
    p::kv("Description", &request.description);
    p::kv("Requested by", &request.requested_by);
    p::kv("Status", &request.status.to_string());
    p::kv("Level progress", &request.level_progress());
    p::kv(
        "Created",
        request.created_at.get(..19).unwrap_or(&request.created_at),
    );
    p::kv(
        "Updated",
        request.updated_at.get(..19).unwrap_or(&request.updated_at),
    );
    if let Some(ref expiry) = request.expires_at {
        p::kv("Expires", expiry.get(..19).unwrap_or(expiry));
    }

    if let Some(ref wf) = workflow {
        println!();
        p::separator();
        println!(
            "  {} {}",
            "Approval Levels".bright_white(),
            format!("({})", wf.levels.len()).dimmed()
        );
        p::separator();

        for (i, level) in wf.levels.iter().enumerate() {
            let actions_at_level: Vec<_> = request
                .actions
                .iter()
                .filter(|a| a.level_index == i)
                .collect();

            let approved_count = actions_at_level
                .iter()
                .filter(|a| a.action == crate::utils::approval_engine::ActionType::Approve)
                .count();
            let rejected = actions_at_level
                .iter()
                .any(|a| a.action == crate::utils::approval_engine::ActionType::Reject);

            let status_char = if i < request.current_level {
                "✓".green()
            } else if i == request.current_level {
                "▶".cyan()
            } else {
                "○".dimmed()
            };

            println!();
            println!(
                "  {} Level {}: {}",
                status_char,
                i + 1,
                level.name.white().bold()
            );

            if rejected {
                println!("    {} {}", "⛔".red(), "Rejected".red());
            } else {
                println!(
                    "    {} {}/{}",
                    "Approvals:".dimmed(),
                    approved_count.to_string().white(),
                    level.required_approvers
                );
            }

            for action in &actions_at_level {
                let action_str = match action.action {
                    crate::utils::approval_engine::ActionType::Approve => "✓ Approved".green(),
                    crate::utils::approval_engine::ActionType::Reject => "✗ Rejected".red(),
                    crate::utils::approval_engine::ActionType::Escalate => "↑ Escalated".yellow(),
                };
                println!(
                    "    {} by {} at {}",
                    action_str,
                    action.approver.white(),
                    action
                        .timestamp
                        .get(..19)
                        .unwrap_or(&action.timestamp)
                        .dimmed()
                );
                if let Some(ref comment) = action.comment {
                    println!("      \"{}\"", comment.italic().dimmed());
                }
            }
        }
    }

    println!();
    p::separator();
    Ok(())
}

fn handle_approve_request(args: ApproveRequestArgs) -> Result<()> {
    p::header("Approve Request");

    let request = get_request(&args.id)?;
    if let Some(ref req) = request {
        if req.status == ApprovalStatus::Approved {
            anyhow::bail!("Request is already approved");
        }
        if req.status == ApprovalStatus::Rejected {
            anyhow::bail!("Request has been rejected");
        }
        if req.status == ApprovalStatus::Cancelled {
            anyhow::bail!("Request has been cancelled");
        }
        if req.status == ApprovalStatus::Expired {
            anyhow::bail!("Request has expired");
        }
    }

    let result = approve_request(
        &args.id,
        &args.approver,
        args.comment.as_deref(),
        &args.roles,
    )?;

    let status_str = result.status.to_string();
    send_approval_completed_notification(
        &result.id,
        &result.contract_id,
        &result.network,
        &args.approver,
        &status_str,
    )?;

    println!();
    p::kv_accent("Request ID", &result.id);
    p::kv("Approved by", &args.approver);
    p::kv("Status", &result.status.to_string());
    p::kv("Level progress", &result.level_progress());

    if result.status == ApprovalStatus::Approved {
        println!();
        p::success("Request fully approved! Ready for deployment.");
        p::info(&format!(
            "Deploy with: starforge deploy --wasm {} --network {}",
            result.wasm_path, result.network
        ));
    } else {
        println!();
        p::info("Request advanced to next approval level.");
    }

    Ok(())
}

fn handle_reject_request(args: RejectRequestArgs) -> Result<()> {
    p::header("Reject Request");

    let result = reject_request(&args.id, &args.approver, &args.reason, &args.roles)?;

    println!();
    p::kv_accent("Request ID", &result.id);
    p::kv("Rejected by", &args.approver);
    p::kv("Reason", &args.reason);
    p::kv("Status", &result.status.to_string());
    p::success("Request rejected");
    Ok(())
}

fn handle_cancel_request(args: CancelRequestArgs) -> Result<()> {
    p::header("Cancel Request");

    let result = cancel_request(&args.id, &args.cancelled_by, &args.reason)?;

    println!();
    p::kv_accent("Request ID", &result.id);
    p::kv("Cancelled by", &args.cancelled_by);
    p::kv("Reason", &args.reason);
    p::kv("Status", &result.status.to_string());
    p::success("Request cancelled");
    Ok(())
}

fn handle_dashboard() -> Result<()> {
    p::header("Approval Dashboard");

    let summary = get_approval_summary()?;
    let requests = list_requests(None, None)?;
    let _workflows = list_workflows(true)?;

    p::separator();
    p::kv("Total requests", &summary.total_requests.to_string());
    p::kv("Workflows", &summary.total_workflows.to_string());
    println!();
    p::kv("Pending", &summary.pending.to_string());
    p::kv("In progress", &summary.in_progress.to_string());
    p::kv("Approved", &summary.approved.to_string());
    p::kv("Rejected", &summary.rejected.to_string());
    p::kv("Expired", &summary.expired.to_string());
    p::kv("Cancelled", &summary.cancelled.to_string());

    if !summary.by_network.is_empty() {
        println!();
        p::separator();
        println!(
            "  {} {}",
            "By Network".bright_white(),
            format!("({} networks)", summary.by_network.len()).dimmed()
        );
        p::separator();
        for (net, count) in &summary.by_network {
            println!(
                "  {} {:>5}",
                format!("{}:", net).dimmed(),
                count.to_string().white()
            );
        }
    }

    if !summary.by_workflow.is_empty() {
        println!();
        p::separator();
        println!(
            "  {} {}",
            "By Workflow".bright_white(),
            format!("({} workflows)", summary.by_workflow.len()).dimmed()
        );
        p::separator();
        for (wf, count) in &summary.by_workflow {
            println!(
                "  {} {:>5}",
                format!("{}:", wf).dimmed(),
                count.to_string().white()
            );
        }
    }

    let has_action_items = requests
        .iter()
        .any(|r| r.status == ApprovalStatus::Pending || r.status == ApprovalStatus::InProgress);

    if has_action_items {
        println!();
        p::separator();
        println!(
            "  {} {}",
            "Pending Actions".bright_white().yellow(),
            "⏳".yellow()
        );
        p::separator();
        for req in &requests {
            if req.status == ApprovalStatus::Pending || req.status == ApprovalStatus::InProgress {
                println!(
                    "  {} {} | {} | {} | {}",
                    "▶".cyan(),
                    req.id[..12].cyan(),
                    req.contract_id.chars().take(20).collect::<String>(),
                    req.network,
                    req.status.to_string().yellow(),
                );
            }
        }
    }

    println!();
    p::separator();
    Ok(())
}
