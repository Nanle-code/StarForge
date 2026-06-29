use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub description: String,
    pub members: Vec<TeamMember>,
    pub created_at: String,
    pub repositories: Vec<ContractRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub public_key: String,
    pub username: String,
    pub role: TeamRole,
    pub joined_at: String,
    pub contribution_points: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TeamRole {
    Owner,
    Admin,
    Developer,
    Reviewer,
    Viewer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractRepository {
    pub id: String,
    pub name: String,
    pub contract_id: String,
    pub description: String,
    pub visibility: RepositoryVisibility,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepositoryVisibility {
    Private,
    Team,
    Public,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeReview {
    pub id: String,
    pub repository_id: String,
    pub contract_id: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub reviewers: Vec<String>,
    pub status: ReviewStatus,
    pub comments: Vec<ReviewComment>,
    pub approvals: u8,
    pub required_approvals: u8,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReviewStatus {
    Open,
    InReview,
    Approved,
    Rejected,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub id: String,
    pub author: String,
    pub content: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub created_at: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedContract {
    pub id: String,
    pub contract_id: String,
    pub shared_by: String,
    pub shared_with: String,
    pub permission: SharePermission,
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SharePermission {
    Read,
    Write,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discussion {
    pub id: String,
    pub contract_id: String,
    pub title: String,
    pub content: String,
    pub author: String,
    pub tags: Vec<String>,
    pub replies: Vec<DiscussionReply>,
    pub upvotes: u32,
    pub downvotes: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionReply {
    pub id: String,
    pub author: String,
    pub content: String,
    pub upvotes: u32,
    pub downvotes: u32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub id: String,
    pub contributor: String,
    pub contract_id: String,
    pub contribution_type: ContributionType,
    pub description: String,
    pub points: u32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContributionType {
    CodeCommit,
    CodeReview,
    BugFix,
    FeatureAddition,
    Documentation,
    TestCoverage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reputation {
    pub public_key: String,
    pub username: String,
    pub total_points: u32,
    pub rank: ReputationRank,
    pub badges: Vec<Badge>,
    pub contribution_history: Vec<Contribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReputationRank {
    Novice,
    Contributor,
    Expert,
    Master,
    Legend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Badge {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub earned_at: String,
}

pub struct SocialManager {
    config_dir: PathBuf,
}

impl SocialManager {
    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir().context("Failed to get home directory")?;
        let config_dir = home_dir.join(".starforge").join("social");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }

        Ok(Self { config_dir })
    }

    // Team Collaboration
    pub fn create_team(
        &self,
        name: &str,
        description: &str,
        owner_public_key: &str,
    ) -> Result<Team> {
        let team = Team {
            id: format!("team_{}", uuid::Uuid::new_v4()),
            name: name.to_string(),
            description: description.to_string(),
            members: vec![TeamMember {
                public_key: owner_public_key.to_string(),
                username: "owner".to_string(),
                role: TeamRole::Owner,
                joined_at: chrono::Utc::now().to_rfc3339(),
                contribution_points: 0,
            }],
            created_at: chrono::Utc::now().to_rfc3339(),
            repositories: vec![],
        };

        self.save_team(&team)?;
        Ok(team)
    }

    pub fn add_team_member(
        &self,
        team_id: &str,
        public_key: &str,
        username: &str,
        role: TeamRole,
    ) -> Result<()> {
        let mut team = self.load_team(team_id)?;

        if team.members.iter().any(|m| m.public_key == public_key) {
            anyhow::bail!("Member already exists in team");
        }

        team.members.push(TeamMember {
            public_key: public_key.to_string(),
            username: username.to_string(),
            role,
            joined_at: chrono::Utc::now().to_rfc3339(),
            contribution_points: 0,
        });

        self.save_team(&team)
    }

    pub fn list_teams(&self) -> Result<Vec<Team>> {
        let mut teams = Vec::new();

        for entry in fs::read_dir(&self.config_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let team: Team = serde_json::from_str(&content)?;
                teams.push(team);
            }
        }

        Ok(teams)
    }

    // Code Review Workflows
    pub fn create_review(
        &self,
        repository_id: &str,
        contract_id: &str,
        title: &str,
        description: &str,
        author: &str,
        required_approvals: u8,
    ) -> Result<CodeReview> {
        let review = CodeReview {
            id: format!("review_{}", uuid::Uuid::new_v4()),
            repository_id: repository_id.to_string(),
            contract_id: contract_id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            author: author.to_string(),
            reviewers: vec![],
            status: ReviewStatus::Open,
            comments: vec![],
            approvals: 0,
            required_approvals,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        self.save_review(&review)?;
        Ok(review)
    }

    pub fn add_review_comment(
        &self,
        review_id: &str,
        author: &str,
        content: &str,
        file_path: Option<String>,
        line_number: Option<u32>,
    ) -> Result<()> {
        let mut review = self.load_review(review_id)?;

        review.comments.push(ReviewComment {
            id: format!("comment_{}", uuid::Uuid::new_v4()),
            author: author.to_string(),
            content: content.to_string(),
            file_path,
            line_number,
            created_at: chrono::Utc::now().to_rfc3339(),
            resolved: false,
        });

        review.updated_at = chrono::Utc::now().to_rfc3339();
        self.save_review(&review)
    }

    pub fn approve_review(&self, review_id: &str, reviewer: &str) -> Result<()> {
        let mut review = self.load_review(review_id)?;

        if !review.reviewers.contains(&reviewer.to_string()) {
            review.reviewers.push(reviewer.to_string());
        }

        review.approvals += 1;
        review.updated_at = chrono::Utc::now().to_rfc3339();

        if review.approvals >= review.required_approvals {
            review.status = ReviewStatus::Approved;
        }

        self.save_review(&review)
    }

    pub fn list_reviews(&self, repository_id: Option<&str>) -> Result<Vec<CodeReview>> {
        let reviews_dir = self.config_dir.join("reviews");

        if !reviews_dir.exists() {
            return Ok(vec![]);
        }

        let mut reviews = Vec::new();

        for entry in fs::read_dir(&reviews_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(repo_id) = repository_id {
                let content = fs::read_to_string(&path)?;
                let review: CodeReview = serde_json::from_str(&content)?;

                if review.repository_id == repo_id {
                    reviews.push(review);
                }
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let review: CodeReview = serde_json::from_str(&content)?;
                reviews.push(review);
            }
        }

        Ok(reviews)
    }

    // Contract Sharing
    pub fn share_contract(
        &self,
        contract_id: &str,
        shared_by: &str,
        shared_with: &str,
        permission: SharePermission,
        expires_at: Option<String>,
    ) -> Result<SharedContract> {
        let shared = SharedContract {
            id: format!("share_{}", uuid::Uuid::new_v4()),
            contract_id: contract_id.to_string(),
            shared_by: shared_by.to_string(),
            shared_with: shared_with.to_string(),
            permission,
            expires_at,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.save_shared_contract(&shared)?;
        Ok(shared)
    }

    pub fn list_shared_contracts(&self, public_key: &str) -> Result<Vec<SharedContract>> {
        let shares_dir = self.config_dir.join("shares");

        if !shares_dir.exists() {
            return Ok(vec![]);
        }

        let mut shared = Vec::new();

        for entry in fs::read_dir(&shares_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let share: SharedContract = serde_json::from_str(&content)?;

                if share.shared_with == public_key || share.shared_by == public_key {
                    shared.push(share);
                }
            }
        }

        Ok(shared)
    }

    // Community Discussion
    pub fn create_discussion(
        &self,
        contract_id: &str,
        title: &str,
        content: &str,
        author: &str,
        tags: Vec<String>,
    ) -> Result<Discussion> {
        let discussion = Discussion {
            id: format!("discussion_{}", uuid::Uuid::new_v4()),
            contract_id: contract_id.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            author: author.to_string(),
            tags,
            replies: vec![],
            upvotes: 0,
            downvotes: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        self.save_discussion(&discussion)?;
        Ok(discussion)
    }

    pub fn add_discussion_reply(
        &self,
        discussion_id: &str,
        author: &str,
        content: &str,
    ) -> Result<()> {
        let mut discussion = self.load_discussion(discussion_id)?;

        discussion.replies.push(DiscussionReply {
            id: format!("reply_{}", uuid::Uuid::new_v4()),
            author: author.to_string(),
            content: content.to_string(),
            upvotes: 0,
            downvotes: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
        });

        discussion.updated_at = chrono::Utc::now().to_rfc3339();
        self.save_discussion(&discussion)
    }

    pub fn vote_discussion(&self, discussion_id: &str, upvote: bool) -> Result<()> {
        let mut discussion = self.load_discussion(discussion_id)?;

        if upvote {
            discussion.upvotes += 1;
        } else {
            discussion.downvotes += 1;
        }

        discussion.updated_at = chrono::Utc::now().to_rfc3339();
        self.save_discussion(&discussion)
    }

    pub fn list_discussions(&self, contract_id: Option<&str>) -> Result<Vec<Discussion>> {
        let discussions_dir = self.config_dir.join("discussions");

        if !discussions_dir.exists() {
            return Ok(vec![]);
        }

        let mut discussions = Vec::new();

        for entry in fs::read_dir(&discussions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(contract_id) = contract_id {
                let content = fs::read_to_string(&path)?;
                let discussion: Discussion = serde_json::from_str(&content)?;

                if discussion.contract_id == contract_id {
                    discussions.push(discussion);
                }
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let discussion: Discussion = serde_json::from_str(&content)?;
                discussions.push(discussion);
            }
        }

        Ok(discussions)
    }

    // Contribution Tracking
    pub fn record_contribution(
        &self,
        contributor: &str,
        contract_id: &str,
        contribution_type: ContributionType,
        description: &str,
        points: u32,
    ) -> Result<Contribution> {
        let contribution = Contribution {
            id: format!("contribution_{}", uuid::Uuid::new_v4()),
            contributor: contributor.to_string(),
            contract_id: contract_id.to_string(),
            contribution_type,
            description: description.to_string(),
            points,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.save_contribution(&contribution)?;

        // Update reputation
        self.update_reputation(contributor, points)?;

        Ok(contribution)
    }

    pub fn get_contributions(&self, contributor: Option<&str>) -> Result<Vec<Contribution>> {
        let contributions_dir = self.config_dir.join("contributions");

        if !contributions_dir.exists() {
            return Ok(vec![]);
        }

        let mut contributions = Vec::new();

        for entry in fs::read_dir(&contributions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(contributor) = contributor {
                let content = fs::read_to_string(&path)?;
                let contribution: Contribution = serde_json::from_str(&content)?;

                if contribution.contributor == contributor {
                    contributions.push(contribution);
                }
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let contribution: Contribution = serde_json::from_str(&content)?;
                contributions.push(contribution);
            }
        }

        Ok(contributions)
    }

    // Reputation System
    pub fn get_reputation(&self, public_key: &str) -> Result<Reputation> {
        let reputation_path = self
            .config_dir
            .join("reputation")
            .join(format!("{}.json", public_key));

        if reputation_path.exists() {
            let content = fs::read_to_string(&reputation_path)?;
            let reputation: Reputation = serde_json::from_str(&content)?;
            return Ok(reputation);
        }

        // Create new reputation entry
        let reputation = Reputation {
            public_key: public_key.to_string(),
            username: public_key[..8].to_string(),
            total_points: 0,
            rank: ReputationRank::Novice,
            badges: vec![],
            contribution_history: vec![],
        };

        self.save_reputation(&reputation)?;
        Ok(reputation)
    }

    pub fn update_reputation(&self, public_key: &str, points: u32) -> Result<()> {
        let mut reputation = self.get_reputation(public_key)?;
        reputation.total_points += points;

        // Update rank based on points
        reputation.rank = match reputation.total_points {
            0..=99 => ReputationRank::Novice,
            100..=499 => ReputationRank::Contributor,
            500..=999 => ReputationRank::Expert,
            1000..=4999 => ReputationRank::Master,
            _ => ReputationRank::Legend,
        };

        // Check for badges
        self.check_and_award_badges(&mut reputation)?;

        self.save_reputation(&reputation)
    }

    fn check_and_award_badges(&self, reputation: &mut Reputation) -> Result<()> {
        let mut new_badges = Vec::new();

        // First contribution badge
        if reputation.total_points >= 10
            && !reputation
                .badges
                .iter()
                .any(|b| b.id == "first_contribution")
        {
            new_badges.push(Badge {
                id: "first_contribution".to_string(),
                name: "First Contribution".to_string(),
                description: "Made your first contribution".to_string(),
                icon: "🌟".to_string(),
                earned_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // Code reviewer badge
        let review_count = reputation
            .contribution_history
            .iter()
            .filter(|c| matches!(c.contribution_type, ContributionType::CodeReview))
            .count();

        if review_count >= 10 && !reputation.badges.iter().any(|b| b.id == "code_reviewer") {
            new_badges.push(Badge {
                id: "code_reviewer".to_string(),
                name: "Code Reviewer".to_string(),
                description: "Completed 10 code reviews".to_string(),
                icon: "👁️".to_string(),
                earned_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        reputation.badges.extend(new_badges);
        Ok(())
    }

    pub fn get_leaderboard(&self, limit: usize) -> Result<Vec<Reputation>> {
        let reputation_dir = self.config_dir.join("reputation");

        if !reputation_dir.exists() {
            return Ok(vec![]);
        }

        let mut reputations = Vec::new();

        for entry in fs::read_dir(&reputation_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let reputation: Reputation = serde_json::from_str(&content)?;
                reputations.push(reputation);
            }
        }

        // Sort by total points descending
        reputations.sort_by_key(|b| std::cmp::Reverse(b.total_points));

        // Limit results
        reputations.truncate(limit);

        Ok(reputations)
    }

    // Helper functions for saving/loading
    fn save_team(&self, team: &Team) -> Result<()> {
        let team_path = self.config_dir.join(format!("{}.json", team.id));
        let json = serde_json::to_string_pretty(team)?;
        fs::write(team_path, json)?;
        Ok(())
    }

    pub fn load_team(&self, team_id: &str) -> Result<Team> {
        let team_path = self.config_dir.join(format!("{}.json", team_id));
        let content = fs::read_to_string(&team_path)?;
        let team: Team = serde_json::from_str(&content)?;
        Ok(team)
    }

    fn save_review(&self, review: &CodeReview) -> Result<()> {
        let reviews_dir = self.config_dir.join("reviews");
        fs::create_dir_all(&reviews_dir)?;

        let review_path = reviews_dir.join(format!("{}.json", review.id));
        let json = serde_json::to_string_pretty(review)?;
        fs::write(review_path, json)?;
        Ok(())
    }

    pub fn load_review(&self, review_id: &str) -> Result<CodeReview> {
        let review_path = self
            .config_dir
            .join("reviews")
            .join(format!("{}.json", review_id));
        let content = fs::read_to_string(&review_path)?;
        let review: CodeReview = serde_json::from_str(&content)?;
        Ok(review)
    }

    fn save_shared_contract(&self, shared: &SharedContract) -> Result<()> {
        let shares_dir = self.config_dir.join("shares");
        fs::create_dir_all(&shares_dir)?;

        let share_path = shares_dir.join(format!("{}.json", shared.id));
        let json = serde_json::to_string_pretty(shared)?;
        fs::write(share_path, json)?;
        Ok(())
    }

    fn save_discussion(&self, discussion: &Discussion) -> Result<()> {
        let discussions_dir = self.config_dir.join("discussions");
        fs::create_dir_all(&discussions_dir)?;

        let discussion_path = discussions_dir.join(format!("{}.json", discussion.id));
        let json = serde_json::to_string_pretty(discussion)?;
        fs::write(discussion_path, json)?;
        Ok(())
    }

    pub fn load_discussion(&self, discussion_id: &str) -> Result<Discussion> {
        let discussion_path = self
            .config_dir
            .join("discussions")
            .join(format!("{}.json", discussion_id));
        let content = fs::read_to_string(&discussion_path)?;
        let discussion: Discussion = serde_json::from_str(&content)?;
        Ok(discussion)
    }

    fn save_contribution(&self, contribution: &Contribution) -> Result<()> {
        let contributions_dir = self.config_dir.join("contributions");
        fs::create_dir_all(&contributions_dir)?;

        let contribution_path = contributions_dir.join(format!("{}.json", contribution.id));
        let json = serde_json::to_string_pretty(contribution)?;
        fs::write(contribution_path, json)?;
        Ok(())
    }

    fn save_reputation(&self, reputation: &Reputation) -> Result<()> {
        let reputation_dir = self.config_dir.join("reputation");
        fs::create_dir_all(&reputation_dir)?;

        let reputation_path = reputation_dir.join(format!("{}.json", reputation.public_key));
        let json = serde_json::to_string_pretty(reputation)?;
        fs::write(reputation_path, json)?;
        Ok(())
    }
}
