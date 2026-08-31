use crate::utils::prompt_manager::PromptManager;
use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use dialoguer::Confirm;
use dialoguer::{theme::ColorfulTheme, Input, Select};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum GenerateCommands {
    /// Generate a Soroban smart contract from a natural language prompt
    Contract {
        /// The description of the smart contract you want to build
        prompt: String,

        /// Output file to save the generated Rust code
        #[arg(short, long, default_value = "contract.rs")]
        out: PathBuf,
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

pub async fn handle(cmd: &GenerateCommands) -> Result<()> {
    match cmd {
        GenerateCommands::Contract { prompt, out } => {
            // Offline-first guard: `generate` is cloud-only and must fail
            // clearly before any network call when offline mode is active.
            crate::utils::ai_offline::require_offline_compatible(
                "generate",
                crate::utils::ai_offline::resolve_configured_mode_sync(),
            )?;

            let api_key = env::var("OPENAI_API_KEY").context(
                "OPENAI_API_KEY environment variable is not set. Please set it to use the AI generator.",
            )?;

            let manager = PromptManager::new()?;
            let context = serde_json::json!({
                "need_tests": true,
                "user_prompt": prompt,
            });
            let (version_id, rendered_prompt) =
                manager.get_rendered_prompt("contract_generator", context)?;

            let mut conversation_history = vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: rendered_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: prompt.clone(),
                },
            ];

            loop {
                println!("🤖 Generating Soroban contract... (this may take a moment)");

                let code = call_openai_api(&api_key, &conversation_history).await?;

                // Show a preview of the code
                println!("\n--- Generated Code Preview ---\n");
                let lines: Vec<&str> = code.lines().take(20).collect();
                for line in lines {
                    println!("{}", line);
                }
                if code.lines().count() > 20 {
                    println!("... ({} more lines)", code.lines().count() - 20);
                }
                println!("\n------------------------------\n");

                let selections = &[
                    "Save to file and exit",
                    "Refine (provide additional instructions)",
                ];
                let selection = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("What would you like to do with the generated contract?")
                    .default(0)
                    .items(&selections[..])
                    .interact()?;

                if selection == 0 {
                    // Clean up potential markdown blocks if the LLM ignored instructions
                    let mut cleaned_code = code.trim();
                    if cleaned_code.starts_with("```rust") {
                        cleaned_code = cleaned_code
                            .strip_prefix("```rust\n")
                            .unwrap_or(cleaned_code);
                    } else if cleaned_code.starts_with("```") {
                        cleaned_code = cleaned_code.strip_prefix("```\n").unwrap_or(cleaned_code);
                    }
                    if cleaned_code.ends_with("```") {
                        cleaned_code = cleaned_code.strip_suffix("```").unwrap_or(cleaned_code);
                    }
                    cleaned_code = cleaned_code.trim();

                    fs::write(out, cleaned_code).context("Failed to write the generated code")?;
                    println!("✅ Contract successfully saved to {}", out.display());

                    if Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("Did this generated contract meet your expectations?")
                        .interact()?
                    {
                        manager.record_feedback(version_id, true, Some(5))?;
                    } else {
                        manager.record_feedback(version_id, false, Some(1))?;
                    }

                    break;
                } else {
                    // Refine
                    conversation_history.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: code,
                    });

                    let refinement_prompt: String = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("Enter your refinement instructions")
                        .interact_text()?;

                    conversation_history.push(ChatMessage {
                        role: "user".to_string(),
                        content: refinement_prompt,
                    });
                }
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
        temperature: 0.2,
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
