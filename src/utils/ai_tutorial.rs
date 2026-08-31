//! AI Interactive Tutorial System
//!
//! Provides adaptive tutorials with skill assessment, personalized learning paths,
//! interactive exercises, real-time feedback, and progress tracking.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// User skill level assessment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillLevel {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

/// Tutorial topic
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TutorialTopic {
    GettingStarted,
    WalletManagement,
    ContractDevelopment,
    Deployment,
    AdvancedFeatures,
    GasOptimization,
    Security,
    Testing,
}

impl TutorialTopic {
    pub fn display_name(&self) -> &'static str {
        match self {
            TutorialTopic::GettingStarted => "Getting Started with StarForge",
            TutorialTopic::WalletManagement => "Wallet Management",
            TutorialTopic::ContractDevelopment => "Contract Development",
            TutorialTopic::Deployment => "Deployment",
            TutorialTopic::AdvancedFeatures => "Advanced Features",
            TutorialTopic::GasOptimization => "Gas Optimization",
            TutorialTopic::Security => "Security Best Practices",
            TutorialTopic::Testing => "Testing",
        }
    }

    pub fn required_skill_level(&self) -> SkillLevel {
        match self {
            TutorialTopic::GettingStarted => SkillLevel::Beginner,
            TutorialTopic::WalletManagement => SkillLevel::Beginner,
            TutorialTopic::ContractDevelopment => SkillLevel::Intermediate,
            TutorialTopic::Deployment => SkillLevel::Intermediate,
            TutorialTopic::AdvancedFeatures => SkillLevel::Advanced,
            TutorialTopic::GasOptimization => SkillLevel::Advanced,
            TutorialTopic::Security => SkillLevel::Advanced,
            TutorialTopic::Testing => SkillLevel::Intermediate,
        }
    }
}

/// Tutorial step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TutorialStep {
    pub id: String,
    pub title: String,
    pub description: String,
    pub content: String,
    pub exercise: Option<Exercise>,
    pub order: u32,
    pub estimated_minutes: u32,
}

/// Interactive exercise
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exercise {
    pub id: String,
    pub question: String,
    pub exercise_type: ExerciseType,
    pub expected_answer: Option<String>,
    pub hints: Vec<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExerciseType {
    MultipleChoice { options: Vec<String> },
    CommandExecution,
    CodeCompletion,
    FreeText,
}

/// User progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProgress {
    pub user_id: String,
    pub skill_level: SkillLevel,
    pub completed_tutorials: Vec<String>,
    pub completed_steps: HashMap<String, Vec<String>>, // tutorial_id -> step_ids
    pub exercise_scores: HashMap<String, f64>,         // exercise_id -> score
    pub total_time_spent_minutes: u64,
    pub last_activity: DateTime<Utc>,
    pub learning_path: Vec<TutorialTopic>,
}

/// Tutorial
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tutorial {
    pub id: String,
    pub topic: TutorialTopic,
    pub title: String,
    pub description: String,
    pub steps: Vec<TutorialStep>,
    pub difficulty: SkillLevel,
    pub estimated_total_minutes: u32,
}

/// Tutorial system manager
#[derive(Clone)]
pub struct TutorialManager {
    tutorials: Arc<RwLock<HashMap<String, Tutorial>>>,
    user_progress: Arc<RwLock<HashMap<String, UserProgress>>>,
}

impl TutorialManager {
    pub fn new() -> Self {
        let manager = TutorialManager {
            tutorials: Arc::new(RwLock::new(HashMap::new())),
            user_progress: Arc::new(RwLock::new(HashMap::new())),
        };

        // Initialize with default tutorials
        let manager_clone = manager.clone();
        tokio::spawn(async move {
            if let Err(e) = manager_clone.initialize_default_tutorials().await {
                eprintln!("Failed to initialize tutorials: {}", e);
            }
        });

        manager
    }

    async fn initialize_default_tutorials(&self) -> Result<()> {
        let mut tutorials = self.tutorials.write().await;

        // Getting Started Tutorial
        tutorials.insert(
            "getting-started".to_string(),
            Tutorial {
                id: "getting-started".to_string(),
                topic: TutorialTopic::GettingStarted,
                title: "Getting Started with StarForge".to_string(),
                description: "Learn the basics of StarForge CLI and set up your development environment.".to_string(),
                steps: vec![
                    TutorialStep {
                        id: "gs-1".to_string(),
                        title: "Introduction to StarForge".to_string(),
                        description: "Learn what StarForge is and how it helps with Stellar development.".to_string(),
                        content: "StarForge is a developer productivity CLI for Stellar and Soroban workflows. It helps you manage wallets, deploy contracts, and scaffold projects from your terminal.".to_string(),
                        exercise: Some(Exercise {
                            id: "gs-1-ex".to_string(),
                            question: "What is StarForge?".to_string(),
                            exercise_type: ExerciseType::MultipleChoice {
                                options: vec![
                                    "A blockchain".to_string(),
                                    "A developer productivity CLI".to_string(),
                                    "A wallet".to_string(),
                                    "A programming language".to_string(),
                                ],
                            },
                            expected_answer: Some("1".to_string()),
                            hints: vec!["Think about what CLI stands for".to_string()],
                            command: None,
                        }),
                        order: 1,
                        estimated_minutes: 5,
                    },
                    TutorialStep {
                        id: "gs-2".to_string(),
                        title: "Installation".to_string(),
                        description: "How to install StarForge on your system.".to_string(),
                        content: "Install StarForge using cargo: cargo install starforge".to_string(),
                        exercise: Some(Exercise {
                            id: "gs-2-ex".to_string(),
                            question: "Run the installation command".to_string(),
                            exercise_type: ExerciseType::CommandExecution,
                            expected_answer: None,
                            hints: vec!["Use cargo install".to_string()],
                            command: Some("cargo install starforge".to_string()),
                        }),
                        order: 2,
                        estimated_minutes: 10,
                    },
                ],
                difficulty: SkillLevel::Beginner,
                estimated_total_minutes: 15,
            },
        );

        // Wallet Management Tutorial
        tutorials.insert(
            "wallet-management".to_string(),
            Tutorial {
                id: "wallet-management".to_string(),
                topic: TutorialTopic::WalletManagement,
                title: "Wallet Management".to_string(),
                description: "Learn to create, manage, and secure your Stellar wallets.".to_string(),
                steps: vec![
                    TutorialStep {
                        id: "wm-1".to_string(),
                        title: "Creating a Wallet".to_string(),
                        description: "How to create a new Stellar wallet.".to_string(),
                        content: "Use 'starforge wallet create <name>' to create a new wallet. The wallet will be saved in your config directory.".to_string(),
                        exercise: Some(Exercise {
                            id: "wm-1-ex".to_string(),
                            question: "What command creates a new wallet?".to_string(),
                            exercise_type: ExerciseType::CodeCompletion,
                            expected_answer: Some("starforge wallet create".to_string()),
                            hints: vec!["It starts with 'starforge wallet'".to_string()],
                            command: None,
                        }),
                        order: 1,
                        estimated_minutes: 8,
                    },
                    TutorialStep {
                        id: "wm-2".to_string(),
                        title: "Listing Wallets".to_string(),
                        description: "View all your saved wallets.".to_string(),
                        content: "Use 'starforge wallet list' to see all your configured wallets.".to_string(),
                        exercise: Some(Exercise {
                            id: "wm-2-ex".to_string(),
                            question: "Run the wallet list command".to_string(),
                            exercise_type: ExerciseType::CommandExecution,
                            expected_answer: None,
                            hints: vec!["The command is 'starforge wallet list'".to_string()],
                            command: Some("starforge wallet list".to_string()),
                        }),
                        order: 2,
                        estimated_minutes: 5,
                    },
                ],
                difficulty: SkillLevel::Beginner,
                estimated_total_minutes: 13,
            },
        );

        Ok(())
    }

    /// Assess user skill level
    pub async fn assess_skill_level(&self, user_id: &str) -> Result<SkillLevel> {
        let progress = self.get_or_create_progress(user_id).await;

        // Calculate skill level based on completed tutorials and scores
        let completed_count = progress.completed_tutorials.len();
        let avg_score: f64 = progress.exercise_scores.values().cloned().sum::<f64>()
            / progress.exercise_scores.len().max(1) as f64;

        let skill_level = match (completed_count, avg_score) {
            (0..=1, _) => SkillLevel::Beginner,
            (2..=4, 0.0..=0.6) => SkillLevel::Beginner,
            (2..=4, 0.6..) => SkillLevel::Intermediate,
            (5..=7, 0.0..=0.6) => SkillLevel::Intermediate,
            (5..=7, 0.6..) => SkillLevel::Advanced,
            (8.., _) => SkillLevel::Expert,
            _ => SkillLevel::Beginner,
        };

        // Update progress
        let mut progress_map = self.user_progress.write().await;
        if let Some(p) = progress_map.get_mut(user_id) {
            p.skill_level = skill_level.clone();
        }

        Ok(skill_level)
    }

    /// Get or create user progress
    async fn get_or_create_progress(&self, user_id: &str) -> UserProgress {
        let progress_map = self.user_progress.read().await;

        if let Some(progress) = progress_map.get(user_id) {
            progress.clone()
        } else {
            drop(progress_map);
            let new_progress = UserProgress {
                user_id: user_id.to_string(),
                skill_level: SkillLevel::Beginner,
                completed_tutorials: vec![],
                completed_steps: HashMap::new(),
                exercise_scores: HashMap::new(),
                total_time_spent_minutes: 0,
                last_activity: Utc::now(),
                learning_path: TutorialManager::generate_learning_path(SkillLevel::Beginner),
            };

            let mut progress_map = self.user_progress.write().await;
            progress_map.insert(user_id.to_string(), new_progress.clone());
            new_progress
        }
    }

    /// Generate personalized learning path
    fn generate_learning_path(skill_level: SkillLevel) -> Vec<TutorialTopic> {
        match skill_level {
            SkillLevel::Beginner => vec![
                TutorialTopic::GettingStarted,
                TutorialTopic::WalletManagement,
                TutorialTopic::ContractDevelopment,
            ],
            SkillLevel::Intermediate => vec![
                TutorialTopic::ContractDevelopment,
                TutorialTopic::Deployment,
                TutorialTopic::Testing,
            ],
            SkillLevel::Advanced => vec![
                TutorialTopic::AdvancedFeatures,
                TutorialTopic::GasOptimization,
                TutorialTopic::Security,
            ],
            SkillLevel::Expert => vec![
                TutorialTopic::GasOptimization,
                TutorialTopic::Security,
                TutorialTopic::AdvancedFeatures,
            ],
        }
    }

    /// Get recommended tutorials for user
    pub async fn get_recommended_tutorials(&self, user_id: &str) -> Result<Vec<Tutorial>> {
        let progress = self.get_or_create_progress(user_id).await;
        let skill_level = progress.skill_level.clone();
        let learning_path = progress.learning_path.clone();

        let tutorials = self.tutorials.read().await;
        let mut recommended = Vec::new();

        for topic in learning_path {
            for tutorial in tutorials.values() {
                if tutorial.topic == topic
                    && !progress.completed_tutorials.contains(&tutorial.id)
                    && tutorial.difficulty.clone() as i32 <= skill_level.clone() as i32 + 1
                {
                    recommended.push(tutorial.clone());
                }
            }
        }

        // Sort by difficulty
        recommended.sort_by_key(|t| t.difficulty.clone() as i32);
        Ok(recommended)
    }

    /// Get all available tutorials
    pub async fn list_tutorials(&self) -> Vec<Tutorial> {
        let tutorials = self.tutorials.read().await;
        tutorials.values().cloned().collect()
    }

    /// Get a specific tutorial
    pub async fn get_tutorial(&self, tutorial_id: &str) -> Result<Tutorial> {
        let tutorials = self.tutorials.read().await;
        tutorials
            .get(tutorial_id)
            .cloned()
            .context("Tutorial not found")
    }

    /// Start a tutorial
    pub async fn start_tutorial(&self, user_id: &str, tutorial_id: &str) -> Result<Tutorial> {
        let tutorial = self.get_tutorial(tutorial_id).await?;

        // Update progress
        let mut progress_map = self.user_progress.write().await;
        if let Some(progress) = progress_map.get_mut(user_id) {
            progress.last_activity = Utc::now();
        }

        Ok(tutorial)
    }

    /// Complete a tutorial step
    pub async fn complete_step(
        &self,
        user_id: &str,
        tutorial_id: &str,
        step_id: &str,
        exercise_answer: Option<String>,
    ) -> Result<StepResult> {
        let tutorial = self.get_tutorial(tutorial_id).await?;
        let step = tutorial
            .steps
            .iter()
            .find(|s| s.id == step_id)
            .context("Step not found")?;

        let mut is_correct = false;
        let mut feedback = String::new();

        // Check exercise answer if provided
        if let Some(answer) = exercise_answer {
            if let Some(exercise) = &step.exercise {
                is_correct = self.check_exercise_answer(exercise, &answer);

                if is_correct {
                    feedback = "Correct! Well done.".to_string();

                    // Update exercise score
                    let mut progress_map = self.user_progress.write().await;
                    if let Some(progress) = progress_map.get_mut(user_id) {
                        progress.exercise_scores.insert(exercise.id.clone(), 1.0);
                    }
                } else {
                    feedback = "Not quite right. Try again!".to_string();

                    let mut progress_map = self.user_progress.write().await;
                    if let Some(progress) = progress_map.get_mut(user_id) {
                        progress.exercise_scores.insert(exercise.id.clone(), 0.0);
                    }
                }
            }
        }

        // Mark step as completed
        let mut progress_map = self.user_progress.write().await;
        if let Some(progress) = progress_map.get_mut(user_id) {
            progress
                .completed_steps
                .entry(tutorial_id.to_string())
                .or_insert_with(Vec::new)
                .push(step_id.to_string());
            progress.total_time_spent_minutes += step.estimated_minutes as u64;
            progress.last_activity = Utc::now();
        }

        // Check if tutorial is complete
        let all_steps = tutorial
            .steps
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>();
        let completed_steps = {
            let progress_map = self.user_progress.read().await;
            progress_map
                .get(user_id)
                .and_then(|p| p.completed_steps.get(tutorial_id))
                .cloned()
                .unwrap_or_default()
        };

        let tutorial_complete = all_steps.iter().all(|s| completed_steps.contains(s));

        if tutorial_complete {
            let mut progress_map = self.user_progress.write().await;
            if let Some(progress) = progress_map.get_mut(user_id) {
                if !progress
                    .completed_tutorials
                    .contains(&tutorial_id.to_string())
                {
                    progress.completed_tutorials.push(tutorial_id.to_string());

                    // Update skill level
                    progress.skill_level = self.assess_skill_level_internal(progress).await;

                    // Update learning path
                    progress.learning_path =
                        TutorialManager::generate_learning_path(progress.skill_level.clone());
                }
            }
        }

        Ok(StepResult {
            is_correct,
            feedback,
            tutorial_complete,
            next_step: if !tutorial_complete {
                tutorial
                    .steps
                    .iter()
                    .find(|s| !completed_steps.contains(&s.id))
                    .map(|s| s.id.clone())
            } else {
                None
            },
        })
    }

    fn check_exercise_answer(&self, exercise: &Exercise, answer: &str) -> bool {
        match &exercise.exercise_type {
            ExerciseType::MultipleChoice { options } => {
                if let Some(expected) = &exercise.expected_answer {
                    answer.trim() == expected.trim()
                } else {
                    // Check if answer is a valid option index
                    answer
                        .parse::<usize>()
                        .ok()
                        .is_some_and(|idx| idx < options.len())
                }
            }
            ExerciseType::CommandExecution => {
                // For command execution, we assume success if they attempted it
                true
            }
            ExerciseType::CodeCompletion => {
                if let Some(expected) = &exercise.expected_answer {
                    answer
                        .trim()
                        .to_lowercase()
                        .contains(&expected.trim().to_lowercase())
                } else {
                    false
                }
            }
            ExerciseType::FreeText => {
                // For free text, we'd need AI evaluation - for now, accept any non-empty answer
                !answer.trim().is_empty()
            }
        }
    }

    async fn assess_skill_level_internal(&self, progress: &UserProgress) -> SkillLevel {
        let completed_count = progress.completed_tutorials.len();
        let avg_score: f64 = progress.exercise_scores.values().cloned().sum::<f64>()
            / progress.exercise_scores.len().max(1) as f64;

        match (completed_count, avg_score) {
            (0..=1, _) => SkillLevel::Beginner,
            (2..=4, 0.0..=0.6) => SkillLevel::Beginner,
            (2..=4, 0.6..) => SkillLevel::Intermediate,
            (5..=7, 0.0..=0.6) => SkillLevel::Intermediate,
            (5..=7, 0.6..) => SkillLevel::Advanced,
            (8.., _) => SkillLevel::Expert,
            _ => SkillLevel::Beginner,
        }
    }

    /// Get user progress
    pub async fn get_user_progress(&self, user_id: &str) -> Result<UserProgress> {
        let progress_map = self.user_progress.read().await;
        progress_map
            .get(user_id)
            .cloned()
            .context("User progress not found")
    }

    /// Reset user progress
    pub async fn reset_progress(&self, user_id: &str) -> Result<()> {
        let mut progress_map = self.user_progress.write().await;
        progress_map.remove(user_id);
        Ok(())
    }
}

impl Default for TutorialManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of completing a step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub is_correct: bool,
    pub feedback: String,
    pub tutorial_complete: bool,
    pub next_step: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_skill_assessment() {
        let manager = TutorialManager::new();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // Wait for initialization

        let skill = manager.assess_skill_level("test-user").await.unwrap();
        assert_eq!(skill, SkillLevel::Beginner);
    }

    #[tokio::test]
    async fn test_learning_path_generation() {
        let beginner_path = TutorialManager::generate_learning_path(SkillLevel::Beginner);
        assert!(beginner_path.contains(&TutorialTopic::GettingStarted));

        let advanced_path = TutorialManager::generate_learning_path(SkillLevel::Advanced);
        assert!(advanced_path.contains(&TutorialTopic::GasOptimization));
    }

    #[tokio::test]
    async fn test_exercise_checking() {
        let manager = TutorialManager::new();
        let exercise = Exercise {
            id: "test".to_string(),
            question: "Test".to_string(),
            exercise_type: ExerciseType::MultipleChoice {
                options: vec!["A".to_string(), "B".to_string()],
            },
            expected_answer: Some("0".to_string()),
            hints: vec![],
            command: None,
        };

        assert!(manager.check_exercise_answer(&exercise, "0"));
        assert!(!manager.check_exercise_answer(&exercise, "1"));
    }
}
