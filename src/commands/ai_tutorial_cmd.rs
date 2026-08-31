//! AI Interactive Tutorial Commands
//!
//! Provides commands for interactive tutorials with skill assessment,
//! personalized learning paths, and progress tracking.

use crate::utils::{
    ai_tutorial::{SkillLevel, TutorialManager},
    print as p,
};
use anyhow::Result;
use clap::Subcommand;
use dialoguer::{Confirm, Input, Select};

#[derive(Subcommand)]
pub enum AiTutorialCommands {
    /// List all available tutorials
    List,

    /// Show recommended tutorials based on your skill level
    Recommended,

    /// Start a specific tutorial
    Start {
        /// Tutorial ID
        tutorial_id: String,
    },

    /// Continue where you left off
    Continue,

    /// Show your learning progress
    Progress,

    /// Assess your current skill level
    Assess,

    /// Reset your learning progress
    Reset,

    /// Show tutorial details
    Show {
        /// Tutorial ID
        tutorial_id: String,
    },
}

pub async fn handle(cmd: AiTutorialCommands) -> Result<()> {
    match cmd {
        AiTutorialCommands::List => handle_list().await,
        AiTutorialCommands::Recommended => handle_recommended().await,
        AiTutorialCommands::Start { tutorial_id } => handle_start(&tutorial_id).await,
        AiTutorialCommands::Continue => handle_continue().await,
        AiTutorialCommands::Progress => handle_progress().await,
        AiTutorialCommands::Assess => handle_assess().await,
        AiTutorialCommands::Reset => handle_reset().await,
        AiTutorialCommands::Show { tutorial_id } => handle_show(&tutorial_id).await,
    }
}

async fn handle_list() -> Result<()> {
    p::header("Available Tutorials");
    p::separator();

    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // Wait for initialization

    let tutorials = manager.list_tutorials().await;

    if tutorials.is_empty() {
        p::info("No tutorials available yet.");
    } else {
        let headers = &["ID", "Title", "Topic", "Difficulty", "Duration"];
        let rows: Vec<Vec<String>> = tutorials
            .iter()
            .map(|t| {
                vec![
                    t.id.clone(),
                    t.title.clone(),
                    t.topic.display_name().to_string(),
                    format!("{:?}", t.difficulty),
                    format!("{} min", t.estimated_total_minutes),
                ]
            })
            .collect();

        p::table(headers, &rows);
    }

    p::separator();
    p::info("Start a tutorial: starforge ai tutorial start <id>");
    Ok(())
}

async fn handle_recommended() -> Result<()> {
    p::header("Recommended Tutorials");
    p::separator();

    let user_id = "default"; // In a real app, this would be authenticated user
    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let skill_level = manager.assess_skill_level(user_id).await?;
    p::kv("Your Skill Level", &format!("{:?}", skill_level));
    println!();

    let tutorials = manager.get_recommended_tutorials(user_id).await?;

    if tutorials.is_empty() {
        p::info("You've completed all available tutorials!");
    } else {
        let headers = &["ID", "Title", "Difficulty", "Duration"];
        let rows: Vec<Vec<String>> = tutorials
            .iter()
            .map(|t| {
                vec![
                    t.id.clone(),
                    t.title.clone(),
                    format!("{:?}", t.difficulty),
                    format!("{} min", t.estimated_total_minutes),
                ]
            })
            .collect();

        p::table(headers, &rows);
    }

    p::separator();
    Ok(())
}

async fn handle_start(tutorial_id: &str) -> Result<()> {
    let user_id = "default";
    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let tutorial = manager.start_tutorial(user_id, tutorial_id).await?;

    p::header(&format!("Tutorial: {}", tutorial.title));
    p::separator();
    p::info(&tutorial.description);
    p::kv("Difficulty", &format!("{:?}", tutorial.difficulty));
    p::kv(
        "Estimated Time",
        &format!("{} min", tutorial.estimated_total_minutes),
    );
    p::separator();
    println!();

    run_tutorial_steps(&manager, user_id, tutorial_id, &tutorial).await?;

    Ok(())
}

async fn run_tutorial_steps(
    manager: &TutorialManager,
    user_id: &str,
    tutorial_id: &str,
    tutorial: &crate::utils::ai_tutorial::Tutorial,
) -> Result<()> {
    let progress = manager.get_user_progress(user_id).await?;
    let completed_steps = progress
        .completed_steps
        .get(tutorial_id)
        .cloned()
        .unwrap_or_default();

    for step in &tutorial.steps {
        if completed_steps.contains(&step.id) {
            p::info(&format!("✓ Step: {} (already completed)", step.title));
            println!();
            continue;
        }

        p::header(&format!("Step: {}", step.title));
        p::separator();
        p::info(&step.description);
        println!();
        println!("{}", step.content);
        println!();

        // Handle exercise if present
        if let Some(exercise) = &step.exercise {
            p::info(&format!("Exercise: {}", exercise.question));
            println!();

            match &exercise.exercise_type {
                crate::utils::ai_tutorial::ExerciseType::MultipleChoice { options } => {
                    let selection = Select::new()
                        .with_prompt("Select your answer")
                        .items(options)
                        .interact()?;

                    let answer = selection.to_string();
                    let result = manager
                        .complete_step(user_id, tutorial_id, &step.id, Some(answer))
                        .await?;

                    println!();
                    if result.is_correct {
                        p::success(&result.feedback);
                    } else {
                        p::error(&result.feedback);
                        if !exercise.hints.is_empty() {
                            println!();
                            p::info("Hint:");
                            for hint in &exercise.hints {
                                println!("  - {}", hint);
                            }
                        }
                    }
                }
                crate::utils::ai_tutorial::ExerciseType::CommandExecution => {
                    if let Some(cmd) = &exercise.command {
                        p::info(&format!("Run this command: {}", cmd));

                        let executed = Confirm::new()
                            .with_prompt("Have you executed the command?")
                            .interact()?;

                        if executed {
                            let result = manager
                                .complete_step(
                                    user_id,
                                    tutorial_id,
                                    &step.id,
                                    Some("executed".to_string()),
                                )
                                .await?;
                            p::success(&result.feedback);
                        }
                    }
                }
                crate::utils::ai_tutorial::ExerciseType::CodeCompletion => {
                    let answer = Input::new().with_prompt("Enter your answer").interact()?;

                    let result = manager
                        .complete_step(user_id, tutorial_id, &step.id, Some(answer))
                        .await?;

                    println!();
                    if result.is_correct {
                        p::success(&result.feedback);
                    } else {
                        p::error(&result.feedback);
                        if !exercise.hints.is_empty() {
                            println!();
                            p::info("Hint:");
                            for hint in &exercise.hints {
                                println!("  - {}", hint);
                            }
                        }
                    }
                }
                crate::utils::ai_tutorial::ExerciseType::FreeText => {
                    let answer = Input::new().with_prompt("Enter your answer").interact()?;

                    let result = manager
                        .complete_step(user_id, tutorial_id, &step.id, Some(answer))
                        .await?;

                    println!();
                    p::success(&result.feedback);
                }
            }

            println!();
        }

        p::separator();
        println!();

        if Confirm::new()
            .with_prompt("Continue to next step?")
            .default(true)
            .interact()?
        {
            continue;
        } else {
            p::info("Tutorial paused. Use 'starforge ai tutorial continue' to resume.");
            return Ok(());
        }
    }

    p::success("🎉 Tutorial completed!");
    p::separator();
    Ok(())
}

async fn handle_continue() -> Result<()> {
    let user_id = "default";
    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let progress = manager.get_user_progress(user_id).await?;

    // Find the first incomplete tutorial
    let tutorials = manager.list_tutorials().await;
    for tutorial in tutorials {
        if !progress.completed_tutorials.contains(&tutorial.id) {
            p::info(&format!("Resuming tutorial: {}", tutorial.title));
            return handle_start(&tutorial.id).await;
        }
    }

    p::info("No incomplete tutorials found. Use 'starforge ai tutorial list' to see available tutorials.");
    Ok(())
}

async fn handle_progress() -> Result<()> {
    p::header("Learning Progress");
    p::separator();

    let user_id = "default";
    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let progress = manager.get_user_progress(user_id).await?;

    p::kv("Skill Level", &format!("{:?}", progress.skill_level));
    p::kv(
        "Completed Tutorials",
        &progress.completed_tutorials.len().to_string(),
    );
    p::kv(
        "Total Time Spent",
        &format!("{} min", progress.total_time_spent_minutes),
    );
    p::kv(
        "Last Activity",
        &progress.last_activity.format("%Y-%m-%d %H:%M").to_string(),
    );
    println!();

    p::info("Learning Path:");
    for (i, topic) in progress.learning_path.iter().enumerate() {
        println!("  {}. {}", i + 1, topic.display_name());
    }

    println!();
    p::info("Completed Tutorials:");
    if progress.completed_tutorials.is_empty() {
        println!("  None yet");
    } else {
        for tutorial_id in &progress.completed_tutorials {
            if let Ok(tutorial) = manager.get_tutorial(tutorial_id).await {
                println!("  ✓ {}", tutorial.title);
            }
        }
    }

    p::separator();
    Ok(())
}

async fn handle_assess() -> Result<()> {
    p::header("Skill Assessment");
    p::separator();

    let user_id = "default";
    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    p::info(
        "Assessing your current skill level based on completed tutorials and exercise scores...",
    );
    println!();

    let skill_level = manager.assess_skill_level(user_id).await?;

    p::success(&format!("Your skill level: {:?}", skill_level));
    println!();

    match skill_level {
        SkillLevel::Beginner => {
            p::info("Recommended: Start with 'Getting Started' and 'Wallet Management' tutorials.");
        }
        SkillLevel::Intermediate => {
            p::info("Recommended: Focus on 'Contract Development' and 'Deployment' tutorials.");
        }
        SkillLevel::Advanced => {
            p::info("Recommended: Explore 'Gas Optimization' and 'Security' tutorials.");
        }
        SkillLevel::Expert => {
            p::info("You've mastered the basics! Try advanced features and contribute to the community.");
        }
    }

    p::separator();
    Ok(())
}

async fn handle_reset() -> Result<()> {
    p::header("Reset Learning Progress");
    p::separator();

    let user_id = "default";
    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    if Confirm::new()
        .with_prompt("Are you sure you want to reset all learning progress?")
        .default(false)
        .interact()?
    {
        manager.reset_progress(user_id).await?;
        p::success("Learning progress has been reset.");
    } else {
        p::info("Reset cancelled.");
    }

    p::separator();
    Ok(())
}

async fn handle_show(tutorial_id: &str) -> Result<()> {
    p::header(&format!("Tutorial: {}", tutorial_id));
    p::separator();

    let manager = TutorialManager::new();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let tutorial = manager.get_tutorial(tutorial_id).await?;

    p::kv("Title", &tutorial.title);
    p::kv("Topic", tutorial.topic.display_name());
    p::kv("Difficulty", &format!("{:?}", tutorial.difficulty));
    p::kv(
        "Estimated Time",
        &format!("{} min", tutorial.estimated_total_minutes),
    );
    println!();
    p::info(&tutorial.description);
    println!();

    p::info("Steps:");
    for (i, step) in tutorial.steps.iter().enumerate() {
        println!(
            "  {}. {} ({} min)",
            i + 1,
            step.title,
            step.estimated_minutes
        );
    }

    p::separator();
    Ok(())
}
