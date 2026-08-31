//! AI Code Search and Discovery for Soroban contracts.
//!
//! Provides:
//! - Natural language code search across the codebase
//! - Semantic code matching by functionality
//! - Pattern discovery and similar code finding
//! - Usage example generation and recommendation

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ── Types ────────────────────────────────────────────────────────────────────

/// A code search result with relevance scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub snippet: String,
    pub relevance_score: f64,
    pub match_type: MatchType,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    ExactMatch,
    SemanticMatch,
    PatternMatch,
    StructuralMatch,
}

/// A discovered code pattern in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPattern {
    pub pattern_id: String,
    pub name: String,
    pub category: String,
    pub occurrences: Vec<PatternOccurrence>,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternOccurrence {
    pub file_path: String,
    pub line: usize,
    pub context: String,
}

/// Similar code block found in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarCode {
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub snippet: String,
    pub similarity_score: f64,
    pub shared_patterns: Vec<String>,
}

/// A usage example found or generated for a contract function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageExample {
    pub function_name: String,
    pub example_type: UsageExampleType,
    pub code: String,
    pub description: String,
    pub source_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageExampleType {
    TestUsage,
    DeploymentUsage,
    IntegrationUsage,
    Generated,
}

/// Result of a code discovery operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub patterns: Vec<DiscoveredPattern>,
    pub similar_code: Vec<SimilarCode>,
    pub usage_examples: Vec<UsageExample>,
    pub summary: String,
    pub total_matches: usize,
}

/// Configuration for code search.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub max_results: usize,
    pub min_relevance: f64,
    pub search_tests: bool,
    pub search_examples: bool,
    pub include_context_lines: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: 20,
            min_relevance: 0.3,
            search_tests: true,
            search_examples: true,
            include_context_lines: 3,
        }
    }
}

// ── Code Index ───────────────────────────────────────────────────────────────

/// A simplified code index entry for search.
#[derive(Debug, Clone)]
struct IndexEntry {
    file_path: String,
    line: usize,
    content: String,
    tokens: Vec<String>,
    is_test: bool,
    // Not currently called from any code path in this crate. Kept rather than
    // removed since deleting it is a product decision, not a lint-scoping one.
    #[allow(dead_code)]
    is_contract: bool,
}

fn build_index(project_dir: &Path) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let rust_files = find_rust_files(project_dir)?;

    for path in &rust_files {
        if let Ok(content) = fs::read_to_string(path) {
            let is_test = path.to_string_lossy().contains("test")
                || content.contains("#[cfg(test)]")
                || content.contains("#[test]");
            let is_contract = content.contains("#[contract]")
                || content.contains("#[contractimpl]")
                || content.contains("#[contracttype]");

            for (line_num, line) in content.lines().enumerate() {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() && !trimmed.starts_with("//") {
                    let tokens: Vec<String> = trimmed
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .filter(|t| t.len() > 2)
                        .map(|t| t.to_lowercase())
                        .collect();

                    entries.push(IndexEntry {
                        file_path: path.to_string_lossy().to_string(),
                        line: line_num + 1,
                        content: trimmed,
                        tokens,
                        is_test,
                        is_contract,
                    });
                }
            }
        }
    }

    Ok(entries)
}

fn find_rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.')
                    && name != "target"
                    && name != "node_modules"
                    && name != "wasm"
                {
                    files.extend(find_rust_files(&path)?);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    Ok(files)
}

// ── Search Functions ─────────────────────────────────────────────────────────

/// Search code by natural language query.
pub fn search_code(
    query: &str,
    project_dir: &Path,
    config: &SearchConfig,
) -> Result<DiscoveryResult> {
    let index = build_index(project_dir)?;
    let query_tokens: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();

    let mut results: Vec<SearchResult> = Vec::new();

    for entry in &index {
        if !config.search_tests && entry.is_test {
            continue;
        }

        let score = calculate_relevance(&query_tokens, &entry.tokens, &entry.content);
        if score >= config.min_relevance {
            let line_start = entry.line.saturating_sub(config.include_context_lines);
            let line_end = entry.line + config.include_context_lines;

            results.push(SearchResult {
                file_path: entry.file_path.clone(),
                line_start,
                line_end,
                snippet: entry.content.clone(),
                relevance_score: score,
                match_type: if score > 0.8 {
                    MatchType::ExactMatch
                } else if score > 0.5 {
                    MatchType::SemanticMatch
                } else {
                    MatchType::PatternMatch
                },
                context: format!("Line {} in {}", entry.line, entry.file_path),
            });
        }
    }

    results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());
    results.truncate(config.max_results);

    let total = results.len();

    Ok(DiscoveryResult {
        query: query.to_string(),
        results,
        patterns: vec![],
        similar_code: vec![],
        usage_examples: vec![],
        summary: format!("Found {} matches for '{}'", total, query),
        total_matches: total,
    })
}

fn calculate_relevance(query_tokens: &[String], entry_tokens: &[String], raw_content: &str) -> f64 {
    if query_tokens.is_empty() || entry_tokens.is_empty() {
        return 0.0;
    }

    let matches = query_tokens
        .iter()
        .filter(|qt| {
            entry_tokens
                .iter()
                .any(|et| et.contains(qt.as_str()) || qt.contains(et.as_str()))
        })
        .count();

    let token_score = matches as f64 / query_tokens.len() as f64;

    let raw_lower = raw_content.to_lowercase();
    let exact_bonus: f64 = query_tokens
        .iter()
        .map(|qt| if raw_lower.contains(qt) { 0.1 } else { 0.0 })
        .sum();

    (token_score + exact_bonus).min(1.0)
}

/// Search for similar code blocks to a given snippet.
pub fn find_similar_code(
    snippet: &str,
    project_dir: &Path,
    max_results: usize,
) -> Result<Vec<SimilarCode>> {
    let index = build_index(project_dir)?;
    let snippet_tokens: Vec<String> = snippet
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() > 2)
        .map(|t| t.to_lowercase())
        .collect();

    let mut results: Vec<SimilarCode> = Vec::new();

    for entry in &index {
        let score = calculate_relevance(&snippet_tokens, &entry.tokens, &entry.content);
        if score > 0.3 && entry.content.trim() != snippet.trim() {
            results.push(SimilarCode {
                file_path: entry.file_path.clone(),
                line_start: entry.line,
                line_end: entry.line,
                snippet: entry.content.clone(),
                similarity_score: score,
                shared_patterns: vec![],
            });
        }
    }

    results.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score).unwrap());
    results.truncate(max_results);

    Ok(results)
}

/// Discover patterns in the codebase by analyzing code structure.
pub fn discover_patterns(project_dir: &Path) -> Result<Vec<DiscoveredPattern>> {
    let index = build_index(project_dir)?;
    let mut patterns: Vec<DiscoveredPattern> = Vec::new();

    let pattern_indicators: Vec<(&str, &str, &str)> = vec![
        ("access_control", "require_auth", "Authorization pattern"),
        ("storage_pattern", "env.storage()", "Persistent storage"),
        ("token_pattern", "transfer", "Token transfer pattern"),
        ("event_emission", "events()", "Event emission pattern"),
        ("error_handling", "Result<", "Error handling pattern"),
        ("test_pattern", "#[test]", "Test function pattern"),
    ];

    for (pattern_id, indicator, description) in &pattern_indicators {
        let occurrences: Vec<PatternOccurrence> = index
            .iter()
            .filter(|e| e.content.contains(indicator))
            .map(|e| PatternOccurrence {
                file_path: e.file_path.clone(),
                line: e.line,
                context: e.content.clone(),
            })
            .collect();

        if occurrences.len() >= 2 {
            patterns.push(DiscoveredPattern {
                pattern_id: pattern_id.to_string(),
                name: description.to_string(),
                category: "code_pattern".to_string(),
                occurrences: occurrences.clone(),
                description: format!(
                    "Found {} occurrences of '{}' pattern",
                    occurrences.len(),
                    indicator
                ),
                suggestion: format!(
                    "Consider abstracting {} into a reusable module",
                    description
                ),
            });
        }
    }

    Ok(patterns)
}

/// Generate usage examples for a specific function.
pub fn generate_usage_examples(
    function_name: &str,
    project_dir: &Path,
) -> Result<Vec<UsageExample>> {
    let index = build_index(project_dir)?;
    let mut examples = Vec::new();

    for entry in &index {
        if entry.content.contains(function_name)
            && (entry.content.contains("fn ") || entry.content.contains("invoke"))
        {
            examples.push(UsageExample {
                function_name: function_name.to_string(),
                example_type: if entry.is_test {
                    UsageExampleType::TestUsage
                } else {
                    UsageExampleType::DeploymentUsage
                },
                code: entry.content.clone(),
                description: format!("Found in {} at line {}", entry.file_path, entry.line),
                source_file: Some(entry.file_path.clone()),
            });
        }
    }

    if examples.is_empty() {
        examples.push(UsageExample {
            function_name: function_name.to_string(),
            example_type: UsageExampleType::Generated,
            code: format!(
                r#"// Example usage of `{}` (generated)
// In a test:
let result = contract.{}(&env, /* args */);
assert!(result.is_ok());"#,
                function_name, function_name,
            ),
            description: "Auto-generated example".to_string(),
            source_file: None,
        });
    }

    Ok(examples)
}

// ── Full Pipeline ────────────────────────────────────────────────────────────

/// Run a comprehensive code discovery pipeline.
pub fn run_discovery(
    query: &str,
    project_dir: &Path,
    config: &SearchConfig,
) -> Result<DiscoveryResult> {
    let mut result = search_code(query, project_dir, config)?;

    result.patterns = discover_patterns(project_dir)?;

    result.similar_code = if !result.results.is_empty() {
        find_similar_code(&result.results[0].snippet, project_dir, 5)?
    } else {
        vec![]
    };

    result.usage_examples = if let Some(first) = result.results.first() {
        let func_name = extract_function_name(&first.snippet);
        if let Some(name) = func_name {
            generate_usage_examples(&name, project_dir)?
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    result.total_matches = result.results.len();

    Ok(result)
}

fn extract_function_name(code: &str) -> Option<String> {
    if let Some(start) = code.find("fn ") {
        let rest = &code[start + 3..];
        if let Some(end) = rest.find('(') {
            let name = rest[..end].trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

// ── Prompt Building ──────────────────────────────────────────────────────────

/// Build a prompt for AI-enhanced code search.
pub fn build_search_prompt(query: &str, project_context: &str) -> String {
    format!(
        r#"Search this Soroban codebase for code related to: {}

Project context:
```
{}
```

Provide:
1. Relevant code snippets with file paths and line numbers
2. How each result relates to the search query
3. Suggestions for similar patterns or related code
4. Usage examples if the result is a function or method

Return JSON with results array."#,
        query, project_context,
    )
}

/// Build a prompt for code pattern discovery.
pub fn build_pattern_discovery_prompt(codebase_summary: &str) -> String {
    format!(
        r#"Analyze this codebase summary and identify reusable patterns, anti-patterns, and opportunities for abstraction.

Codebase summary:
```
{}
```

Identify:
1. Common patterns that could be extracted into shared utilities
2. Anti-patterns that should be refactored
3. Missing patterns (e.g., error handling, testing conventions)
4. Code duplication that could be eliminated

Return JSON with patterns, anti_patterns, missing_patterns, and refactoring_suggestions."#,
        codebase_summary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function_name() {
        assert_eq!(
            extract_function_name("pub fn transfer(env: Env, amount: i64) -> bool"),
            Some("transfer".to_string())
        );
        assert_eq!(
            extract_function_name("fn balance() -> i64"),
            Some("balance".to_string())
        );
        assert_eq!(extract_function_name("let x = 5;"), None);
    }

    #[test]
    fn test_calculate_relevance() {
        let query = vec!["transfer".to_string(), "amount".to_string()];
        let tokens = vec![
            "fn".to_string(),
            "transfer".to_string(),
            "amount".to_string(),
        ];
        let score = calculate_relevance(&query, &tokens, "fn transfer(amount: i64)");
        assert!(score > 0.5);
    }

    #[test]
    fn test_search_config_default() {
        let config = SearchConfig::default();
        assert_eq!(config.max_results, 20);
        assert_eq!(config.min_relevance, 0.3);
    }

    #[test]
    fn test_build_prompts() {
        let prompt = build_search_prompt("token transfer", "Sample codebase");
        assert!(prompt.contains("token transfer"));

        let pattern_prompt = build_pattern_discovery_prompt("Sample summary");
        assert!(pattern_prompt.contains("patterns"));
    }
}
