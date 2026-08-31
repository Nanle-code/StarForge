use crate::utils::prompt_manager::PromptManager;
use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use dialoguer::{theme::ColorfulTheme, Input};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum ExplainCommands {
    /// Explain a Soroban smart contract using AI
    Contract {
        /// The path to the Soroban contract file to explain
        file: PathBuf,

        /// Explanation depth (beginner, intermediate, advanced, expert)
        #[arg(long, default_value = "intermediate")]
        level: String,

        /// Language for the explanation
        #[arg(long, default_value = "English")]
        lang: String,
    },
}

#[derive(Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

pub async fn handle(cmd: &ExplainCommands) -> Result<()> {
    // Offline-first guard: `explain` is cloud-only and must fail clearly
    // before any network call when offline mode is active.
    crate::utils::ai_offline::require_offline_compatible(
        "explain",
        crate::utils::ai_offline::resolve_configured_mode_sync(),
    )?;

    match cmd {
        ExplainCommands::Contract { file, level, lang } => {
            let api_key = env::var("OPENAI_API_KEY").context(
                "OPENAI_API_KEY environment variable is not set. Please set it to use the AI explainer.",
            )?;

            let code = fs::read_to_string(file).context("Failed to read the target file")?;

            let manager = PromptManager::new()?;
            let context = serde_json::json!({
                "code": code,
                "level": level,
                "language": lang,
            });
            let (version_id, rendered_prompt) =
                manager.get_rendered_prompt("code_explainer", context)?;

            let mut conversation_history = vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: rendered_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: "Please explain the code now.".to_string(),
                },
            ];

            println!("🤖 Analyzing code... (this may take a moment)");

            let response = call_openai_api(&api_key, &conversation_history).await?;

            println!("\n{}\n", response);

            // Record initial positive feedback for rendering successfully
            manager.record_feedback(version_id, true, None)?;

            // Interactive loop
            conversation_history.push(ChatMessage {
                role: "assistant".to_string(),
                content: response,
            });

            loop {
                let follow_up: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Do you have questions about specific lines or concepts? (Type your question, or 'exit' to quit)")
                    .allow_empty(true)
                    .interact_text()?;

                let follow_up = follow_up.trim();
                if follow_up.is_empty() || follow_up.eq_ignore_ascii_case("exit") {
                    println!("Goodbye!");
                    break;
                }

                conversation_history.push(ChatMessage {
                    role: "user".to_string(),
                    content: follow_up.to_string(),
                });

                println!("🤖 Thinking...");
                let answer = call_openai_api(&api_key, &conversation_history).await?;
                println!("\n{}\n", answer);

                conversation_history.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: answer,
                });
            }

            Ok(())
        }
    }
}

async fn call_openai_api(api_key: &str, messages: &[ChatMessage]) -> Result<String> {
    let client = reqwest::Client::new();

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))?,
    );

    let request_body = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: messages.to_vec(),
        temperature: 0.3,
    };

    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .headers(headers)
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to OpenAI API")?;

    if !res.status().is_success() {
        let err_text = res.text().await?;
        return Err(anyhow!("OpenAI API error: {}", err_text));
    }

    let response_data: ChatResponse = res
        .json()
        .await
        .context("Failed to parse OpenAI API response")?;

    let content = response_data
        .choices
        .first()
        .ok_or_else(|| anyhow!("No choices returned from OpenAI"))?
        .message
        .content
        .clone();

    Ok(content)
}
