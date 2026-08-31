//! Conversational AI Assistant Commands
//!
//! Provides interactive chat interface with multi-turn conversation support,
//! context retention, and workflow guidance.

use crate::utils::ai_cache;
use crate::utils::{
    ai_conversation::{
        AssistantPersonality, ConversationManager, ExpertiseLevel, UserPreferences, VerbosityLevel,
        WorkflowState, WorkflowType,
    },
    ollama, print as p,
};
use anyhow::{Context, Result};
use clap::Subcommand;
use rustyline::DefaultEditor;
use std::collections::HashMap;

#[derive(Subcommand)]
pub enum AiChatCommands {
    /// Start an interactive chat session
    Chat {
        /// Session ID to resume (optional)
        #[arg(long)]
        session: Option<String>,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Assistant personality
        #[arg(long, default_value = "professional")]
        personality: String,

        /// Expertise level (beginner, intermediate, advanced)
        #[arg(long, default_value = "intermediate")]
        expertise: String,

        /// Verbosity level (brief, normal, detailed)
        #[arg(long, default_value = "normal")]
        verbosity: String,
    },

    /// List active chat sessions
    ListSessions,

    /// Show conversation history for a session
    History {
        /// Session ID
        session: String,
    },

    /// Delete a chat session
    DeleteSession {
        /// Session ID
        session: String,
    },

    /// Start a guided workflow
    Workflow {
        /// Workflow type (deployment, wallet, development, debugging, optimization)
        workflow_type: String,

        /// Session ID to resume (optional)
        #[arg(long)]
        session: Option<String>,
    },
}

pub async fn handle(cmd: AiChatCommands) -> Result<()> {
    match cmd {
        AiChatCommands::Chat {
            session,
            model,
            personality,
            expertise,
            verbosity,
        } => handle_chat(session, &model, &personality, &expertise, &verbosity).await,
        AiChatCommands::ListSessions => handle_list_sessions().await,
        AiChatCommands::History { session } => handle_history(&session).await,
        AiChatCommands::DeleteSession { session } => handle_delete_session(&session).await,
        AiChatCommands::Workflow {
            workflow_type,
            session,
        } => handle_workflow(&workflow_type, session).await,
    }
}

async fn handle_chat(
    session_id: Option<String>,
    model: &str,
    personality: &str,
    expertise: &str,
    verbosity: &str,
) -> Result<()> {
    let manager = ConversationManager::new();

    let session_id = if let Some(sid) = session_id {
        // Resume existing session
        manager
            .get_context(&sid)
            .await
            .context("Session not found")?;
        sid
    } else {
        // Create new session
        let preferences = UserPreferences {
            personality: parse_personality(personality),
            verbosity: parse_verbosity(verbosity),
            expertise_level: parse_expertise(expertise),
            enable_proactive_suggestions: true,
        };
        manager.create_session(preferences).await?
    };

    p::header("Conversational AI Assistant");
    p::separator();
    p::kv("Session", &session_id);
    p::kv("Model", model);
    p::info("Type 'exit' or 'quit' to end the conversation.");
    p::separator();
    println!();

    // Add system message with context
    let system_msg = format!(
        "You are a helpful StarForge AI assistant. You help developers with Stellar and Soroban development. \
        Be concise and actionable. Current expertise level: {:?}, Personality: {:?}.",
        parse_expertise(expertise),
        parse_personality(personality)
    );
    manager.add_system_message(&session_id, system_msg).await?;

    let mut rl = DefaultEditor::new()?;

    loop {
        let readline = rl.readline("You: ");
        match readline {
            Ok(line) => {
                let line = line.trim();

                if line.is_empty() {
                    continue;
                }

                if line == "exit" || line == "quit" {
                    p::info("Ending conversation session.");
                    break;
                }

                if line == "/help" {
                    print_help();
                    continue;
                }

                if line == "/suggestions" {
                    let suggestions = manager.generate_suggestions(&session_id).await?;
                    print_suggestions(&suggestions);
                    continue;
                }

                if line == "/clear" {
                    manager.clear_history(&session_id).await?;
                    p::success("Conversation history cleared.");
                    continue;
                }

                // Add user message
                manager
                    .add_user_message(&session_id, line.to_string(), None)
                    .await?;

                // Generate AI response
                let prompt = manager.format_for_prompt(&session_id).await?;

                p::info("AI: Thinking...");

                let response = generate_ai_response(&prompt, model).await?;

                println!("AI: {}", response);

                // Add assistant response
                manager
                    .add_assistant_message(&session_id, response, None)
                    .await?;

                // Show proactive suggestions if available
                let suggestions = manager.generate_suggestions(&session_id).await?;
                if !suggestions.is_empty() {
                    println!();
                    p::info("💡 Suggestions:");
                    print_suggestions(&suggestions);
                }

                println!();
            }
            Err(_) => {
                break;
            }
        }
    }

    p::separator();
    p::info(&format!("Session saved: {}", session_id));
    p::info("Resume with: starforge ai chat --session <id>");
    Ok(())
}

async fn handle_list_sessions() -> Result<()> {
    p::header("Active Chat Sessions");
    p::separator();

    let manager = ConversationManager::new();
    let sessions = manager.list_sessions().await;

    if sessions.is_empty() {
        p::info("No active sessions.");
    } else {
        let headers = &["Session ID", "Created", "Last Updated"];
        let mut rows = Vec::new();

        for session_id in sessions {
            if let Ok(context) = manager.get_context(&session_id).await {
                rows.push(vec![
                    session_id,
                    context.created_at.format("%Y-%m-%d %H:%M").to_string(),
                    context.last_updated.format("%Y-%m-%d %H:%M").to_string(),
                ]);
            }
        }

        p::table(headers, &rows);
    }

    p::separator();
    Ok(())
}

async fn handle_history(session_id: &str) -> Result<()> {
    p::header(&format!("Conversation History: {}", session_id));
    p::separator();

    let manager = ConversationManager::new();
    let history = manager.get_history(session_id).await?;

    if history.is_empty() {
        p::info("No messages in this session.");
    } else {
        for message in history {
            let role = match message.role {
                crate::utils::ai_conversation::MessageRole::User => "You",
                crate::utils::ai_conversation::MessageRole::Assistant => "AI",
                crate::utils::ai_conversation::MessageRole::System => "System",
            };
            println!("{}: {}", role, message.content);
        }
    }

    p::separator();
    Ok(())
}

async fn handle_delete_session(session_id: &str) -> Result<()> {
    p::header(&format!("Delete Session: {}", session_id));
    p::separator();

    let manager = ConversationManager::new();
    manager.delete_session(session_id).await?;

    p::success("Session deleted.");
    p::separator();
    Ok(())
}

async fn handle_workflow(workflow_type: &str, session_id: Option<String>) -> Result<()> {
    let manager = ConversationManager::new();

    let workflow = match workflow_type {
        "deployment" => WorkflowType::ContractDeployment,
        "wallet" => WorkflowType::WalletSetup,
        "development" => WorkflowType::ContractDevelopment,
        "debugging" => WorkflowType::Debugging,
        "optimization" => WorkflowType::GasOptimization,
        _ => WorkflowType::Custom(workflow_type.to_string()),
    };

    let session_id = if let Some(sid) = session_id {
        manager
            .get_context(&sid)
            .await
            .context("Session not found")?;
        sid
    } else {
        let preferences = UserPreferences::default();
        manager.create_session(preferences).await?
    };

    let workflow_state = WorkflowState {
        current_step: "start".to_string(),
        completed_steps: vec![],
        workflow_type: workflow.clone(),
        data: HashMap::new(),
    };

    manager
        .set_workflow_state(&session_id, workflow_state)
        .await?;

    p::header(&format!("Workflow: {:?}", workflow));
    p::separator();
    p::info("Starting guided workflow...");
    p::info("The AI will guide you through each step.");
    p::separator();

    // Start chat with workflow context
    handle_chat(
        Some(session_id),
        ollama::DEFAULT_MODEL,
        "professional",
        "intermediate",
        "normal",
    )
    .await
}

async fn generate_ai_response(prompt: &str, model: &str) -> Result<String> {
    let opts = crate::utils::ollama::GenerateOptions {
        temperature: Some(0.7),
        num_predict: Some(512),
        num_ctx: Some(4096),
    };

    let response = ollama::generate_cached(
        model,
        prompt,
        Some(opts),
        Some(ai_cache::DEFAULT_CACHE_TTL_SECONDS),
        "ask",
    )
    .await
    .context("LLM generation failed")?;

    Ok(response.response.trim().to_string())
}

fn print_help() {
    println!();
    p::info("Available commands:");
    println!("  /help        - Show this help");
    println!("  /suggestions - Show proactive suggestions");
    println!("  /clear       - Clear conversation history");
    println!("  /exit        - End conversation");
    println!();
}

fn print_suggestions(suggestions: &[crate::utils::ai_conversation::Suggestion]) {
    for (i, suggestion) in suggestions.iter().enumerate() {
        println!("  {}. {}", i + 1, suggestion.title);
        println!("     {}", suggestion.description);
        if let crate::utils::ai_conversation::SuggestionAction::Command(cmd) =
            &suggestion.action_type
        {
            println!("     Command: {}", cmd);
        }
    }
    println!();
}

fn parse_personality(s: &str) -> AssistantPersonality {
    match s.to_lowercase().as_str() {
        "friendly" => AssistantPersonality::Friendly,
        "technical" => AssistantPersonality::Technical,
        "concise" => AssistantPersonality::Concise,
        _ => AssistantPersonality::Professional,
    }
}

fn parse_verbosity(s: &str) -> VerbosityLevel {
    match s.to_lowercase().as_str() {
        "brief" => VerbosityLevel::Brief,
        "detailed" => VerbosityLevel::Detailed,
        _ => VerbosityLevel::Normal,
    }
}

fn parse_expertise(s: &str) -> ExpertiseLevel {
    match s.to_lowercase().as_str() {
        "beginner" => ExpertiseLevel::Beginner,
        "advanced" => ExpertiseLevel::Advanced,
        _ => ExpertiseLevel::Intermediate,
    }
}
