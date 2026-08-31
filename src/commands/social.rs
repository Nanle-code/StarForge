use crate::utils::{config, print as p, social};
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum SocialCommands {
    /// Manage teams for contract collaboration
    #[command(subcommand)]
    Team(TeamCommands),
    /// Manage code reviews
    #[command(subcommand)]
    Review(ReviewCommands),
    /// Share contracts with others
    #[command(subcommand)]
    Share(ShareCommands),
    /// Community discussions
    #[command(subcommand)]
    Discussion(DiscussionCommands),
    /// View contribution history and reputation
    #[command(subcommand)]
    Contribution(ContributionCommands),
    /// View reputation leaderboard
    Leaderboard(LeaderboardArgs),
}

#[derive(Subcommand)]
pub enum TeamCommands {
    /// Create a new team
    Create(CreateTeamArgs),
    /// Add a member to a team
    AddMember(AddTeamMemberArgs),
    /// List all teams
    List,
    /// Show team details
    Show(ShowTeamArgs),
}

#[derive(Args)]
pub struct CreateTeamArgs {
    /// Team name
    pub name: String,
    /// Team description
    pub description: String,
    /// Owner's wallet name
    #[arg(long)]
    pub wallet: String,
}

#[derive(Args)]
pub struct AddTeamMemberArgs {
    /// Team ID
    pub team_id: String,
    /// Member's public key
    pub public_key: String,
    /// Member's username
    pub username: String,
    /// Member role (owner, admin, developer, reviewer, viewer)
    #[arg(long, default_value = "developer")]
    pub role: String,
}

#[derive(Args)]
pub struct ShowTeamArgs {
    /// Team ID
    pub team_id: String,
}

#[derive(Subcommand)]
pub enum ReviewCommands {
    /// Create a new code review
    Create(CreateReviewArgs),
    /// Add a comment to a review
    Comment(ReviewCommentArgs),
    /// Approve a review
    Approve(ApproveReviewArgs),
    /// List reviews
    List(ListReviewsArgs),
    /// Show review details
    Show(ShowReviewArgs),
}

#[derive(Args)]
pub struct CreateReviewArgs {
    /// Repository ID
    pub repository_id: String,
    /// Contract ID
    pub contract_id: String,
    /// Review title
    pub title: String,
    /// Review description
    pub description: String,
    /// Author's wallet name
    #[arg(long)]
    pub wallet: String,
    /// Required approvals
    #[arg(long, default_value = "2")]
    pub required_approvals: u8,
}

#[derive(Args)]
pub struct ReviewCommentArgs {
    /// Review ID
    pub review_id: String,
    /// Comment content
    pub content: String,
    /// Author's wallet name
    #[arg(long)]
    pub wallet: String,
    /// File path (optional)
    #[arg(long)]
    pub file: Option<String>,
    /// Line number (optional)
    #[arg(long)]
    pub line: Option<u32>,
}

#[derive(Args)]
pub struct ApproveReviewArgs {
    /// Review ID
    pub review_id: String,
    /// Reviewer's wallet name
    #[arg(long)]
    pub wallet: String,
}

#[derive(Args)]
pub struct ListReviewsArgs {
    /// Filter by repository ID
    #[arg(long)]
    pub repository_id: Option<String>,
}

#[derive(Args)]
pub struct ShowReviewArgs {
    /// Review ID
    pub review_id: String,
}

#[derive(Subcommand)]
pub enum ShareCommands {
    /// Share a contract with someone
    Share(ShareContractArgs),
    /// List shared contracts
    List(ListSharedArgs),
}

#[derive(Args)]
pub struct ShareContractArgs {
    /// Contract ID
    pub contract_id: String,
    /// Public key to share with
    pub shared_with: String,
    /// Sharer's wallet name
    #[arg(long)]
    pub wallet: String,
    /// Permission level (read, write, admin)
    #[arg(long, default_value = "read")]
    pub permission: String,
    /// Expiration date (RFC3339 format, optional)
    #[arg(long)]
    pub expires_at: Option<String>,
}

#[derive(Args)]
pub struct ListSharedArgs {
    /// Public key to filter by
    #[arg(long)]
    pub public_key: Option<String>,
}

#[derive(Subcommand)]
pub enum DiscussionCommands {
    /// Create a new discussion
    Create(CreateDiscussionArgs),
    /// Reply to a discussion
    Reply(ReplyDiscussionArgs),
    /// Vote on a discussion
    Vote(VoteDiscussionArgs),
    /// List discussions
    List(ListDiscussionsArgs),
    /// Show discussion details
    Show(ShowDiscussionArgs),
}

#[derive(Args)]
pub struct CreateDiscussionArgs {
    /// Contract ID
    pub contract_id: String,
    /// Discussion title
    pub title: String,
    /// Discussion content
    pub content: String,
    /// Author's wallet name
    #[arg(long)]
    pub wallet: String,
    /// Tags (comma-separated)
    #[arg(long)]
    pub tags: Option<String>,
}

#[derive(Args)]
pub struct ReplyDiscussionArgs {
    /// Discussion ID
    pub discussion_id: String,
    /// Reply content
    pub content: String,
    /// Author's wallet name
    #[arg(long)]
    pub wallet: String,
}

#[derive(Args)]
pub struct VoteDiscussionArgs {
    /// Discussion ID
    pub discussion_id: String,
    /// Upvote (true) or downvote (false)
    #[arg(long)]
    pub upvote: bool,
}

#[derive(Args)]
pub struct ListDiscussionsArgs {
    /// Filter by contract ID
    #[arg(long)]
    pub contract_id: Option<String>,
}

#[derive(Args)]
pub struct ShowDiscussionArgs {
    /// Discussion ID
    pub discussion_id: String,
}

#[derive(Subcommand)]
pub enum ContributionCommands {
    /// Record a contribution
    Record(RecordContributionArgs),
    /// List contributions
    List(ListContributionsArgs),
    /// Show reputation
    Show(ShowReputationArgs),
}

#[derive(Args)]
pub struct RecordContributionArgs {
    /// Contributor's wallet name
    #[arg(long)]
    pub wallet: String,
    /// Contract ID
    pub contract_id: String,
    /// Contribution type (code_commit, code_review, bug_fix, feature_addition, documentation, test_coverage)
    #[arg(long)]
    pub contribution_type: String,
    /// Description
    pub description: String,
    /// Points awarded
    #[arg(long)]
    pub points: u32,
}

#[derive(Args)]
pub struct ListContributionsArgs {
    /// Filter by contributor
    #[arg(long)]
    pub contributor: Option<String>,
}

#[derive(Args)]
pub struct ShowReputationArgs {
    /// Public key or wallet name
    pub identifier: String,
}

#[derive(Args)]
pub struct LeaderboardArgs {
    /// Number of top contributors to show
    #[arg(long, default_value = "10")]
    pub limit: usize,
}

pub async fn handle(cmd: SocialCommands) -> Result<()> {
    match cmd {
        SocialCommands::Team(team_cmd) => handle_team(team_cmd),
        SocialCommands::Review(review_cmd) => handle_review(review_cmd),
        SocialCommands::Share(share_cmd) => handle_share(share_cmd),
        SocialCommands::Discussion(discussion_cmd) => handle_discussion(discussion_cmd),
        SocialCommands::Contribution(contribution_cmd) => handle_contribution(contribution_cmd),
        SocialCommands::Leaderboard(args) => handle_leaderboard(args),
    }
}

fn handle_team(cmd: TeamCommands) -> Result<()> {
    let social_manager = social::SocialManager::new()?;

    match cmd {
        TeamCommands::Create(args) => {
            let cfg = config::load()?;
            let wallet = cfg
                .wallets
                .iter()
                .find(|w| w.name == args.wallet)
                .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

            p::header("Create Team");
            p::kv("Team name", &args.name);
            p::kv("Description", &args.description);
            p::kv("Owner", &wallet.public_key);

            let team =
                social_manager.create_team(&args.name, &args.description, &wallet.public_key)?;

            p::success("Team created successfully");
            p::kv("Team ID", &team.id);
        }
        TeamCommands::AddMember(args) => {
            let role = match args.role.to_lowercase().as_str() {
                "owner" => social::TeamRole::Owner,
                "admin" => social::TeamRole::Admin,
                "developer" => social::TeamRole::Developer,
                "reviewer" => social::TeamRole::Reviewer,
                "viewer" => social::TeamRole::Viewer,
                _ => anyhow::bail!("Invalid role. Use: owner, admin, developer, reviewer, viewer"),
            };

            p::header("Add Team Member");
            p::kv("Team ID", &args.team_id);
            p::kv("Public key", &args.public_key);
            p::kv("Username", &args.username);
            p::kv("Role", &args.role);

            social_manager.add_team_member(
                &args.team_id,
                &args.public_key,
                &args.username,
                role,
            )?;

            p::success("Member added successfully");
        }
        TeamCommands::List => {
            p::header("Teams");

            let teams = social_manager.list_teams()?;

            if teams.is_empty() {
                p::info("No teams found");
            } else {
                for team in teams {
                    println!();
                    p::kv_accent("Team", &team.name);
                    p::kv("ID", &team.id);
                    p::kv("Description", &team.description);
                    p::kv("Members", &team.members.len().to_string());
                    p::kv("Repositories", &team.repositories.len().to_string());
                }
            }
        }
        TeamCommands::Show(args) => {
            p::header("Team Details");
            p::kv("Team ID", &args.team_id);

            let team = social_manager.load_team(&args.team_id)?;

            println!();
            p::kv_accent("Name", &team.name);
            p::kv("Description", &team.description);
            p::kv("Created at", &team.created_at);

            println!();
            p::header("Members");
            for member in &team.members {
                println!();
                p::kv("Username", &member.username);
                p::kv("Public key", &member.public_key);
                p::kv("Role", &format!("{:?}", member.role));
                p::kv(
                    "Contribution points",
                    &member.contribution_points.to_string(),
                );
            }
        }
    }

    Ok(())
}

fn handle_review(cmd: ReviewCommands) -> Result<()> {
    let social_manager = social::SocialManager::new()?;

    match cmd {
        ReviewCommands::Create(args) => {
            let cfg = config::load()?;
            let wallet = cfg
                .wallets
                .iter()
                .find(|w| w.name == args.wallet)
                .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

            p::header("Create Code Review");
            p::kv("Repository ID", &args.repository_id);
            p::kv("Contract ID", &args.contract_id);
            p::kv("Title", &args.title);
            p::kv("Required approvals", &args.required_approvals.to_string());

            let review = social_manager.create_review(
                &args.repository_id,
                &args.contract_id,
                &args.title,
                &args.description,
                &wallet.public_key,
                args.required_approvals,
            )?;

            p::success("Review created successfully");
            p::kv("Review ID", &review.id);
        }
        ReviewCommands::Comment(args) => {
            p::header("Add Review Comment");
            p::kv("Review ID", &args.review_id);
            p::kv("Content", &args.content);

            social_manager.add_review_comment(
                &args.review_id,
                &args.wallet,
                &args.content,
                args.file,
                args.line,
            )?;

            p::success("Comment added successfully");
        }
        ReviewCommands::Approve(args) => {
            p::header("Approve Review");
            p::kv("Review ID", &args.review_id);
            p::kv("Reviewer", &args.wallet);

            social_manager.approve_review(&args.review_id, &args.wallet)?;

            p::success("Review approved successfully");
        }
        ReviewCommands::List(args) => {
            p::header("Code Reviews");

            let reviews = social_manager.list_reviews(args.repository_id.as_deref())?;

            if reviews.is_empty() {
                p::info("No reviews found");
            } else {
                for review in reviews {
                    println!();
                    p::kv_accent("Title", &review.title);
                    p::kv("ID", &review.id);
                    p::kv("Status", &format!("{:?}", review.status));
                    p::kv(
                        "Approvals",
                        &format!("{}/{}", review.approvals, review.required_approvals),
                    );
                    p::kv("Comments", &review.comments.len().to_string());
                }
            }
        }
        ReviewCommands::Show(args) => {
            p::header("Review Details");
            p::kv("Review ID", &args.review_id);

            let review = social_manager.load_review(&args.review_id)?;

            println!();
            p::kv_accent("Title", &review.title);
            p::kv("Description", &review.description);
            p::kv("Status", &format!("{:?}", review.status));
            p::kv("Author", &review.author);
            p::kv(
                "Approvals",
                &format!("{}/{}", review.approvals, review.required_approvals),
            );

            println!();
            p::header("Comments");
            for comment in &review.comments {
                println!();
                p::kv("Author", &comment.author);
                p::kv("Content", &comment.content);
                if let Some(file) = &comment.file_path {
                    p::kv("File", file);
                    if let Some(line) = comment.line_number {
                        p::kv("Line", &line.to_string());
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_share(cmd: ShareCommands) -> Result<()> {
    let social_manager = social::SocialManager::new()?;

    match cmd {
        ShareCommands::Share(args) => {
            let cfg = config::load()?;
            let wallet = cfg
                .wallets
                .iter()
                .find(|w| w.name == args.wallet)
                .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

            let permission = match args.permission.to_lowercase().as_str() {
                "read" => social::SharePermission::Read,
                "write" => social::SharePermission::Write,
                "admin" => social::SharePermission::Admin,
                _ => anyhow::bail!("Invalid permission. Use: read, write, admin"),
            };

            p::header("Share Contract");
            p::kv("Contract ID", &args.contract_id);
            p::kv("Shared with", &args.shared_with);
            p::kv("Permission", &args.permission);

            let shared = social_manager.share_contract(
                &args.contract_id,
                &wallet.public_key,
                &args.shared_with,
                permission,
                args.expires_at,
            )?;

            p::success("Contract shared successfully");
            p::kv("Share ID", &shared.id);
        }
        ShareCommands::List(args) => {
            p::header("Shared Contracts");

            let public_key = if let Some(pk) = args.public_key {
                pk
            } else {
                let cfg = config::load()?;
                if !cfg.wallets.is_empty() {
                    cfg.wallets[0].public_key.clone()
                } else {
                    anyhow::bail!("No wallet found. Specify --public-key");
                }
            };

            let shared = social_manager.list_shared_contracts(&public_key)?;

            if shared.is_empty() {
                p::info("No shared contracts found");
            } else {
                for share in shared {
                    println!();
                    p::kv_accent("Contract ID", &share.contract_id);
                    p::kv("Shared by", &share.shared_by);
                    p::kv("Shared with", &share.shared_with);
                    p::kv("Permission", &format!("{:?}", share.permission));
                    p::kv("Shared at", &share.created_at);
                }
            }
        }
    }

    Ok(())
}

fn handle_discussion(cmd: DiscussionCommands) -> Result<()> {
    let social_manager = social::SocialManager::new()?;

    match cmd {
        DiscussionCommands::Create(args) => {
            let cfg = config::load()?;
            let wallet = cfg
                .wallets
                .iter()
                .find(|w| w.name == args.wallet)
                .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

            let tags: Vec<String> = args
                .tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();

            p::header("Create Discussion");
            p::kv("Contract ID", &args.contract_id);
            p::kv("Title", &args.title);
            p::kv("Tags", &tags.join(", "));

            let discussion = social_manager.create_discussion(
                &args.contract_id,
                &args.title,
                &args.content,
                &wallet.public_key,
                tags,
            )?;

            p::success("Discussion created successfully");
            p::kv("Discussion ID", &discussion.id);
        }
        DiscussionCommands::Reply(args) => {
            p::header("Reply to Discussion");
            p::kv("Discussion ID", &args.discussion_id);
            p::kv("Content", &args.content);

            social_manager.add_discussion_reply(
                &args.discussion_id,
                &args.wallet,
                &args.content,
            )?;

            p::success("Reply added successfully");
        }
        DiscussionCommands::Vote(args) => {
            p::header("Vote on Discussion");
            p::kv("Discussion ID", &args.discussion_id);
            p::kv("Vote", if args.upvote { "Upvote" } else { "Downvote" });

            social_manager.vote_discussion(&args.discussion_id, args.upvote)?;

            p::success("Vote recorded successfully");
        }
        DiscussionCommands::List(args) => {
            p::header("Discussions");

            let discussions = social_manager.list_discussions(args.contract_id.as_deref())?;

            if discussions.is_empty() {
                p::info("No discussions found");
            } else {
                for discussion in discussions {
                    println!();
                    p::kv_accent("Title", &discussion.title);
                    p::kv("ID", &discussion.id);
                    p::kv("Author", &discussion.author);
                    p::kv("Replies", &discussion.replies.len().to_string());
                    p::kv(
                        "Votes",
                        &format!("+{}/-{}", discussion.upvotes, discussion.downvotes),
                    );
                }
            }
        }
        DiscussionCommands::Show(args) => {
            p::header("Discussion Details");
            p::kv("Discussion ID", &args.discussion_id);

            let discussion = social_manager.load_discussion(&args.discussion_id)?;

            println!();
            p::kv_accent("Title", &discussion.title);
            p::kv("Content", &discussion.content);
            p::kv("Author", &discussion.author);
            p::kv("Tags", &discussion.tags.join(", "));
            p::kv(
                "Votes",
                &format!("+{}/-{}", discussion.upvotes, discussion.downvotes),
            );

            println!();
            p::header("Replies");
            for reply in &discussion.replies {
                println!();
                p::kv("Author", &reply.author);
                p::kv("Content", &reply.content);
                p::kv("Votes", &format!("+{}/-{}", reply.upvotes, reply.downvotes));
            }
        }
    }

    Ok(())
}

fn handle_contribution(cmd: ContributionCommands) -> Result<()> {
    let social_manager = social::SocialManager::new()?;

    match cmd {
        ContributionCommands::Record(args) => {
            let cfg = config::load()?;
            let wallet = cfg
                .wallets
                .iter()
                .find(|w| w.name == args.wallet)
                .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

            let contribution_type = match args.contribution_type.to_lowercase().as_str() {
                "code_commit" => social::ContributionType::CodeCommit,
                "code_review" => social::ContributionType::CodeReview,
                "bug_fix" => social::ContributionType::BugFix,
                "feature_addition" => social::ContributionType::FeatureAddition,
                "documentation" => social::ContributionType::Documentation,
                "test_coverage" => social::ContributionType::TestCoverage,
                _ => anyhow::bail!("Invalid contribution type. Use: code_commit, code_review, bug_fix, feature_addition, documentation, test_coverage"),
            };

            p::header("Record Contribution");
            p::kv("Contributor", &wallet.public_key);
            p::kv("Contract ID", &args.contract_id);
            p::kv("Type", &args.contribution_type);
            p::kv("Points", &args.points.to_string());

            social_manager.record_contribution(
                &wallet.public_key,
                &args.contract_id,
                contribution_type,
                &args.description,
                args.points,
            )?;

            p::success("Contribution recorded successfully");
        }
        ContributionCommands::List(args) => {
            p::header("Contributions");

            let contributions = social_manager.get_contributions(args.contributor.as_deref())?;

            if contributions.is_empty() {
                p::info("No contributions found");
            } else {
                for contribution in contributions {
                    println!();
                    p::kv_accent("Contributor", &contribution.contributor);
                    p::kv("Contract ID", &contribution.contract_id);
                    p::kv("Type", &format!("{:?}", contribution.contribution_type));
                    p::kv("Description", &contribution.description);
                    p::kv("Points", &contribution.points.to_string());
                }
            }
        }
        ContributionCommands::Show(args) => {
            p::header("Reputation");
            p::kv("Identifier", &args.identifier);

            let reputation = social_manager.get_reputation(&args.identifier)?;

            println!();
            p::kv_accent("Username", &reputation.username);
            p::kv("Total points", &reputation.total_points.to_string());
            p::kv("Rank", &format!("{:?}", reputation.rank));

            println!();
            p::header("Badges");
            for badge in &reputation.badges {
                println!();
                p::kv("Name", &badge.name);
                p::kv("Description", &badge.description);
                p::kv("Earned at", &badge.earned_at);
            }
        }
    }

    Ok(())
}

fn handle_leaderboard(args: LeaderboardArgs) -> Result<()> {
    let social_manager = social::SocialManager::new()?;

    p::header("Reputation Leaderboard");
    p::kv("Top", &args.limit.to_string());

    let leaderboard = social_manager.get_leaderboard(args.limit)?;

    if leaderboard.is_empty() {
        p::info("No reputation data found");
    } else {
        println!();
        for (i, reputation) in leaderboard.iter().enumerate() {
            p::kv_accent(&format!("#{}", i + 1), &reputation.username);
            p::kv("Points", &reputation.total_points.to_string());
            p::kv("Rank", &format!("{:?}", reputation.rank));
            p::kv("Badges", &reputation.badges.len().to_string());
            println!();
        }
    }

    Ok(())
}
