//! Conversational AI Assistant
//!
//! Provides multi-turn conversation support with context retention,
//! workflow guidance, and personality customization.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Conversation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// Conversation context for retention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    pub session_id: String,
    pub messages: Vec<ConversationMessage>,
    pub workflow_state: Option<WorkflowState>,
    pub user_preferences: UserPreferences,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
}

/// Workflow state for guided conversations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub current_step: String,
    pub completed_steps: Vec<String>,
    pub workflow_type: WorkflowType,
    pub data: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowType {
    ContractDeployment,
    WalletSetup,
    ContractDevelopment,
    Debugging,
    GasOptimization,
    Custom(String),
}

/// User preferences for personalization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub personality: AssistantPersonality,
    pub verbosity: VerbosityLevel,
    pub expertise_level: ExpertiseLevel,
    pub enable_proactive_suggestions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssistantPersonality {
    Professional,
    Friendly,
    Technical,
    Concise,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerbosityLevel {
    Brief,
    Normal,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExpertiseLevel {
    Beginner,
    Intermediate,
    Advanced,
}

impl Default for UserPreferences {
    fn default() -> Self {
        UserPreferences {
            personality: AssistantPersonality::Professional,
            verbosity: VerbosityLevel::Normal,
            expertise_level: ExpertiseLevel::Intermediate,
            enable_proactive_suggestions: true,
        }
    }
}

/// Proactive suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub title: String,
    pub description: String,
    pub action_type: SuggestionAction,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SuggestionAction {
    Command(String),
    Explanation,
    NextStep,
    Documentation,
}

/// Conversation manager
pub struct ConversationManager {
    contexts: Arc<RwLock<HashMap<String, ConversationContext>>>,
    max_context_messages: usize,
}

impl ConversationManager {
    pub fn new() -> Self {
        ConversationManager {
            contexts: Arc::new(RwLock::new(HashMap::new())),
            max_context_messages: 50,
        }
    }

    pub fn with_max_messages(mut self, max: usize) -> Self {
        self.max_context_messages = max;
        self
    }

    /// Create a new conversation session
    pub async fn create_session(&self, preferences: UserPreferences) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let context = ConversationContext {
            session_id: session_id.clone(),
            messages: vec![],
            workflow_state: None,
            user_preferences: preferences,
            created_at: Utc::now(),
            last_updated: Utc::now(),
        };

        let mut contexts = self.contexts.write().await;
        contexts.insert(session_id.clone(), context);
        Ok(session_id)
    }

    /// Add a user message to the conversation
    pub async fn add_user_message(
        &self,
        session_id: &str,
        content: String,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<()> {
        let message = ConversationMessage {
            role: MessageRole::User,
            content,
            timestamp: Utc::now(),
            metadata,
        };

        self.add_message(session_id, message).await
    }

    /// Add an assistant message to the conversation
    pub async fn add_assistant_message(
        &self,
        session_id: &str,
        content: String,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<()> {
        let message = ConversationMessage {
            role: MessageRole::Assistant,
            content,
            timestamp: Utc::now(),
            metadata,
        };

        self.add_message(session_id, message).await
    }

    /// Add a system message to the conversation
    pub async fn add_system_message(&self, session_id: &str, content: String) -> Result<()> {
        let message = ConversationMessage {
            role: MessageRole::System,
            content,
            timestamp: Utc::now(),
            metadata: None,
        };

        self.add_message(session_id, message).await
    }

    async fn add_message(&self, session_id: &str, message: ConversationMessage) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(session_id).context("Session not found")?;

        context.messages.push(message);
        context.last_updated = Utc::now();

        // Trim context if too large
        if context.messages.len() > self.max_context_messages {
            let _remove_count = context.messages.len() - self.max_context_messages;
            // Keep system messages, remove oldest user/assistant messages
            context.messages.retain(|m| m.role == MessageRole::System);

            // Add back recent messages up to limit
            let recent_messages: Vec<_> = context
                .messages
                .iter()
                .filter(|m| m.role != MessageRole::System)
                .rev()
                .take(self.max_context_messages)
                .cloned()
                .collect();

            context.messages.extend(recent_messages.into_iter().rev());
        }

        Ok(())
    }

    /// Get conversation context
    pub async fn get_context(&self, session_id: &str) -> Result<ConversationContext> {
        let contexts = self.contexts.read().await;
        contexts
            .get(session_id)
            .cloned()
            .context("Session not found")
    }

    /// Get conversation history
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<ConversationMessage>> {
        let context = self.get_context(session_id).await?;
        Ok(context.messages)
    }

    /// Set workflow state
    pub async fn set_workflow_state(
        &self,
        session_id: &str,
        workflow_state: WorkflowState,
    ) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(session_id).context("Session not found")?;

        context.workflow_state = Some(workflow_state);
        context.last_updated = Utc::now();
        Ok(())
    }

    /// Update user preferences
    pub async fn update_preferences(
        &self,
        session_id: &str,
        preferences: UserPreferences,
    ) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(session_id).context("Session not found")?;

        context.user_preferences = preferences;
        context.last_updated = Utc::now();
        Ok(())
    }

    /// Clear conversation history
    pub async fn clear_history(&self, session_id: &str) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(session_id).context("Session not found")?;

        // Keep system messages
        context.messages.retain(|m| m.role == MessageRole::System);
        context.last_updated = Utc::now();
        Ok(())
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut contexts = self.contexts.write().await;
        contexts.remove(session_id).context("Session not found")?;
        Ok(())
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Vec<String> {
        let contexts = self.contexts.read().await;
        contexts.keys().cloned().collect()
    }

    /// Generate proactive suggestions based on context
    pub async fn generate_suggestions(&self, session_id: &str) -> Result<Vec<Suggestion>> {
        let context = self.get_context(session_id).await?;

        if !context.user_preferences.enable_proactive_suggestions {
            return Ok(vec![]);
        }

        let mut suggestions = Vec::new();

        // Analyze last few messages for context
        let recent_messages: Vec<_> = context.messages.iter().rev().take(5).collect();

        let last_user_message = recent_messages.iter().find(|m| m.role == MessageRole::User);

        if let Some(msg) = last_user_message {
            let content = msg.content.to_lowercase();

            // Deployment-related suggestions
            if content.contains("deploy") || content.contains("contract") {
                suggestions.push(Suggestion {
                    title: "Check deployment prerequisites".to_string(),
                    description: "Ensure your wallet is funded and contract is compiled"
                        .to_string(),
                    action_type: SuggestionAction::NextStep,
                    confidence: 0.8,
                });

                suggestions.push(Suggestion {
                    title: "Run gas analysis".to_string(),
                    description: "Analyze gas costs before deployment".to_string(),
                    action_type: SuggestionAction::Command(
                        "starforge gas analyze --wasm <file>".to_string(),
                    ),
                    confidence: 0.7,
                });
            }

            // Wallet-related suggestions
            if content.contains("wallet") {
                suggestions.push(Suggestion {
                    title: "List available wallets".to_string(),
                    description: "View all configured wallets".to_string(),
                    action_type: SuggestionAction::Command("starforge wallet list".to_string()),
                    confidence: 0.9,
                });
            }

            // Debugging-related suggestions
            if content.contains("error") || content.contains("bug") {
                suggestions.push(Suggestion {
                    title: "Run AI debugger".to_string(),
                    description: "Let AI analyze the error".to_string(),
                    action_type: SuggestionAction::Command("starforge ai debug".to_string()),
                    confidence: 0.85,
                });
            }

            // Testing-related suggestions
            if content.contains("test") {
                suggestions.push(Suggestion {
                    title: "Generate test suite".to_string(),
                    description: "AI can generate comprehensive tests".to_string(),
                    action_type: SuggestionAction::Command("starforge ai test <file>".to_string()),
                    confidence: 0.75,
                });
            }
        }

        // Workflow-based suggestions
        if let Some(workflow) = &context.workflow_state {
            if workflow.workflow_type == WorkflowType::ContractDeployment
                && !workflow.completed_steps.contains(&"compile".to_string())
            {
                suggestions.push(Suggestion {
                    title: "Compile contract".to_string(),
                    description: "Build the WASM file".to_string(),
                    action_type: SuggestionAction::Command(
                        "cargo build --target wasm32-unknown-unknown --release".to_string(),
                    ),
                    confidence: 0.95,
                });
            }
        }

        Ok(suggestions)
    }

    /// Format conversation for AI prompt
    pub async fn format_for_prompt(&self, session_id: &str) -> Result<String> {
        let context = self.get_context(session_id).await?;

        let mut prompt = String::new();

        // Add personality context
        prompt.push_str(&format!(
            "Personality: {:?}\n",
            context.user_preferences.personality
        ));
        prompt.push_str(&format!(
            "Expertise Level: {:?}\n",
            context.user_preferences.expertise_level
        ));
        prompt.push_str(&format!(
            "Verbosity: {:?}\n\n",
            context.user_preferences.verbosity
        ));

        // Add workflow context if active
        if let Some(workflow) = &context.workflow_state {
            prompt.push_str(&format!("Current Workflow: {:?}\n", workflow.workflow_type));
            prompt.push_str(&format!("Current Step: {}\n", workflow.current_step));
            prompt.push_str(&format!(
                "Completed Steps: {:?}\n\n",
                workflow.completed_steps
            ));
        }

        // Add conversation history
        prompt.push_str("Conversation History:\n");
        for message in &context.messages {
            prompt.push_str(&format!("{:?}: {}\n", message.role, message.content));
        }

        Ok(prompt)
    }
}

impl Default for ConversationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let manager = ConversationManager::new();
        let session_id = manager
            .create_session(UserPreferences::default())
            .await
            .unwrap();
        assert!(!session_id.is_empty());
    }

    #[tokio::test]
    async fn test_add_message() {
        let manager = ConversationManager::new();
        let session_id = manager
            .create_session(UserPreferences::default())
            .await
            .unwrap();

        manager
            .add_user_message(&session_id, "Hello".to_string(), None)
            .await
            .unwrap();

        let history = manager.get_history(&session_id).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, MessageRole::User);
    }

    #[tokio::test]
    async fn test_workflow_state() {
        let manager = ConversationManager::new();
        let session_id = manager
            .create_session(UserPreferences::default())
            .await
            .unwrap();

        let workflow = WorkflowState {
            current_step: "compile".to_string(),
            completed_steps: vec![],
            workflow_type: WorkflowType::ContractDeployment,
            data: HashMap::new(),
        };

        manager
            .set_workflow_state(&session_id, workflow)
            .await
            .unwrap();

        let context = manager.get_context(&session_id).await.unwrap();
        assert!(context.workflow_state.is_some());
    }

    #[tokio::test]
    async fn test_proactive_suggestions() {
        let manager = ConversationManager::new();
        let session_id = manager
            .create_session(UserPreferences::default())
            .await
            .unwrap();

        manager
            .add_user_message(
                &session_id,
                "I want to deploy my contract".to_string(),
                None,
            )
            .await
            .unwrap();

        let suggestions = manager.generate_suggestions(&session_id).await.unwrap();
        assert!(!suggestions.is_empty());
    }
}
