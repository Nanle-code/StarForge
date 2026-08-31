use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub max_context_tokens: u32,
    pub include_project_structure: bool,
    pub include_recent_edits: bool,
    pub include_contract_code: bool,
    pub include_config_files: bool,
    pub include_stellar_docs: bool,
    pub max_file_size_bytes: u64,
    pub context_priorities: ContextPriorities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPriorities {
    pub contract_code: u8,
    pub config_files: u8,
    pub recent_edits: u8,
    pub project_structure: u8,
    pub documentation: u8,
}

impl Default for ContextPriorities {
    fn default() -> Self {
        Self {
            contract_code: 10,
            config_files: 7,
            recent_edits: 8,
            project_structure: 5,
            documentation: 3,
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 8000,
            include_project_structure: true,
            include_recent_edits: true,
            include_contract_code: true,
            include_config_files: true,
            include_stellar_docs: false,
            max_file_size_bytes: 100_000,
            context_priorities: ContextPriorities::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub source: ContextSource,
    pub content: String,
    pub priority: u8,
    pub estimated_tokens: u32,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContextSource {
    ProjectStructure,
    ContractCode,
    ConfigFile,
    RecentEdit,
    Documentation,
    UserHistory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub messages: Vec<ConversationMessage>,
    pub project_path: PathBuf,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Session {
    pub fn new(project_path: PathBuf) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            messages: Vec::new(),
            project_path,
            created_at: chrono::Utc::now(),
        }
    }

    pub fn add_message(&mut self, role: MessageRole, content: String) {
        self.messages.push(ConversationMessage {
            role,
            content,
            timestamp: chrono::Utc::now(),
        });
    }

    pub fn recent_messages(&self, count: usize) -> &[ConversationMessage] {
        let start = self.messages.len().saturating_sub(count);
        &self.messages[start..]
    }
}

pub struct AIContextManager {
    config: ContextConfig,
    sessions: RwLock<HashMap<String, Session>>,
    file_cache: RwLock<HashMap<PathBuf, (String, chrono::DateTime<chrono::Utc>)>>,
}

impl AIContextManager {
    pub fn new(config: ContextConfig) -> Self {
        Self {
            config,
            sessions: RwLock::new(HashMap::new()),
            file_cache: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_session(&self, project_path: PathBuf) -> String {
        let session = Session::new(project_path);
        let id = session.id.clone();
        let mut sessions = self.sessions.write().await;
        sessions.insert(id.clone(), session);
        id
    }

    pub async fn add_message(
        &self,
        session_id: &str,
        role: MessageRole,
        content: String,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id).context("Session not found")?;
        session.add_message(role, content);
        Ok(())
    }

    pub async fn get_conversation_history(
        &self,
        session_id: &str,
        max_messages: usize,
    ) -> Result<Vec<ConversationMessage>> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id).context("Session not found")?;
        Ok(session.recent_messages(max_messages).to_vec())
    }

    pub async fn collect_context(&self, project_path: &Path) -> Result<Vec<ContextItem>> {
        let mut items = Vec::new();

        if self.config.include_project_structure {
            let structure = self.collect_project_structure(project_path).await?;
            items.push(structure);
        }

        if self.config.include_contract_code {
            let contracts = self.collect_contract_code(project_path).await?;
            items.extend(contracts);
        }

        if self.config.include_config_files {
            let configs = self.collect_config_files(project_path).await?;
            items.extend(configs);
        }

        if self.config.include_recent_edits {
            let edits = self.collect_recent_edits(project_path).await?;
            items.extend(edits);
        }

        items.sort_by_key(|a| std::cmp::Reverse(a.priority));

        Ok(items)
    }

    pub async fn build_context_window(
        &self,
        project_path: &Path,
        additional_context: Option<&str>,
    ) -> Result<String> {
        let items = self.collect_context(project_path).await?;
        let mut total_tokens: u32 = 0;
        let mut context_parts = Vec::new();

        if let Some(extra) = additional_context {
            context_parts.push(extra.to_string());
            total_tokens += estimate_tokens(extra);
        }

        for item in &items {
            let item_tokens = item.estimated_tokens;
            if total_tokens + item_tokens > self.config.max_context_tokens {
                continue;
            }
            total_tokens += item_tokens;
            context_parts.push(format!(
                "--- {} ---\n{}",
                item.source_description(),
                item.content
            ));
        }

        Ok(context_parts.join("\n\n"))
    }

    async fn collect_project_structure(&self, project_path: &Path) -> Result<ContextItem> {
        let mut structure = String::new();
        structure.push_str("Project Structure:\n");

        if let Ok(entries) = read_dir_recursive(project_path, &self.config, 0, 3) {
            structure.push_str(&entries);
        }

        let tokens = estimate_tokens(&structure);

        Ok(ContextItem {
            source: ContextSource::ProjectStructure,
            content: structure,
            priority: self.config.context_priorities.project_structure,
            estimated_tokens: tokens,
            path: None,
        })
    }

    async fn collect_contract_code(&self, project_path: &Path) -> Result<Vec<ContextItem>> {
        let mut items = Vec::new();

        if let Ok(entries) = std::fs::read_dir(project_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && path
                        .file_name()
                        .is_some_and(|n| n == "contracts" || n == "src")
                {
                    if let Ok(contract_items) = collect_rust_files_sync(&path, &self.config) {
                        items.extend(contract_items);
                    }
                }
            }
        }

        for item in &mut items {
            item.priority = self.config.context_priorities.contract_code;
        }

        Ok(items)
    }

    async fn collect_config_files(&self, project_path: &Path) -> Result<Vec<ContextItem>> {
        let mut items = Vec::new();
        let config_names = ["Cargo.toml", "soroban.toml", ".env", "stellar.toml"];

        for name in &config_names {
            let path = project_path.join(name);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if content.len() as u64 <= self.config.max_file_size_bytes {
                        let tokens = estimate_tokens(&content);
                        items.push(ContextItem {
                            source: ContextSource::ConfigFile,
                            content: format!("{}\n{}", path.display(), content),
                            priority: self.config.context_priorities.config_files,
                            estimated_tokens: tokens,
                            path: Some(path.display().to_string()),
                        });
                    }
                }
            }
        }

        Ok(items)
    }

    async fn collect_recent_edits(&self, project_path: &Path) -> Result<Vec<ContextItem>> {
        let mut items = Vec::new();
        let file_cache = self.file_cache.read().await;

        for (path, (content, _modified)) in file_cache.iter() {
            if path.starts_with(project_path) {
                let tokens = estimate_tokens(content);
                items.push(ContextItem {
                    source: ContextSource::RecentEdit,
                    content: format!("Recently modified: {}\n{}", path.display(), content),
                    priority: self.config.context_priorities.recent_edits,
                    estimated_tokens: tokens,
                    path: Some(path.display().to_string()),
                });
            }
        }

        Ok(items)
    }

    pub async fn update_file_cache(&self, path: PathBuf, content: String) {
        let mut cache = self.file_cache.write().await;
        cache.insert(path, (content, chrono::Utc::now()));
    }

    pub fn compress_context(&self, context: &str, max_tokens: u32) -> String {
        let current_tokens = estimate_tokens(context);
        if current_tokens <= max_tokens {
            return context.to_string();
        }

        let ratio = max_tokens as f64 / current_tokens as f64;
        let target_chars = (context.len() as f64 * ratio) as usize;

        let lines: Vec<&str> = context.lines().collect();
        let mut compressed = Vec::new();
        let mut char_count = 0;

        for line in &lines {
            if char_count + line.len() + 1 > target_chars {
                break;
            }
            compressed.push(*line);
            char_count += line.len() + 1;
        }

        compressed.join("\n")
    }
}

fn estimate_tokens(text: &str) -> u32 {
    (text.len() as f64 / 4.0).ceil() as u32
}

fn read_dir_recursive(
    path: &Path,
    config: &ContextConfig,
    current_depth: u32,
    max_depth: u32,
) -> Result<String, std::io::Error> {
    if current_depth >= max_depth {
        return Ok(String::new());
    }

    let mut output = String::new();
    let indent = "  ".repeat(current_depth as usize);

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }

        if path.is_dir() {
            output.push_str(&format!("{}{}/\n", indent, name));
            if let Ok(sub) = read_dir_recursive(&path, config, current_depth + 1, max_depth) {
                output.push_str(&sub);
            }
        } else if let Ok(metadata) = std::fs::metadata(&path) {
            if metadata.len() <= config.max_file_size_bytes {
                output.push_str(&format!("{}{}\n", indent, name));
            }
        }
    }

    Ok(output)
}

fn collect_rust_files_sync(
    dir: &Path,
    config: &ContextConfig,
) -> Result<Vec<ContextItem>, std::io::Error> {
    let mut items = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.len() <= config.max_file_size_bytes {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let tokens = estimate_tokens(&content);
                        items.push(ContextItem {
                            source: ContextSource::ContractCode,
                            content: format!("// File: {}\n{}", path.display(), content),
                            priority: config.context_priorities.contract_code,
                            estimated_tokens: tokens,
                            path: Some(path.display().to_string()),
                        });
                    }
                }
            }
        } else if path.is_dir() {
            if let Ok(sub_items) = collect_rust_files_sync(&path, config) {
                items.extend(sub_items);
            }
        }
    }

    Ok(items)
}

impl ContextItem {
    fn source_description(&self) -> &str {
        match self.source {
            ContextSource::ProjectStructure => "Project Structure",
            ContextSource::ContractCode => "Contract Code",
            ContextSource::ConfigFile => "Configuration",
            ContextSource::RecentEdit => "Recent Edits",
            ContextSource::Documentation => "Documentation",
            ContextSource::UserHistory => "User History",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 2);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("1234"), 1);
    }

    #[test]
    fn test_session_add_message() {
        let mut session = Session::new(PathBuf::from("/tmp/test"));
        session.add_message(MessageRole::User, "Hello".into());
        session.add_message(MessageRole::Assistant, "Hi there".into());
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn test_session_recent_messages() {
        let mut session = Session::new(PathBuf::from("/tmp/test"));
        for i in 0..10 {
            session.add_message(MessageRole::User, format!("Message {}", i));
        }
        let recent = session.recent_messages(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].content, "Message 7");
    }

    #[test]
    fn test_compress_context() {
        let config = ContextConfig::default();
        let manager = AIContextManager::new(config);
        let context = "line1\nline2\nline3\nline4\nline5";
        let compressed = manager.compress_context(context, 2);
        assert!(compressed.len() <= context.len());
    }
}
