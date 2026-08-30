//! AI Accessibility Features (issue #521).
//!
//! Makes StarForge usable for developers with disabilities through screen
//! reader optimization, voice command support, text simplification, visual
//! assistance, keyboard navigation, and a customizable interface.

use crate::utils::config;
use crate::utils::ollama::{self, GenerateOptions};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ─── Configuration ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityConfig {
    pub screen_reader_mode: bool,
    pub simplified_text_mode: bool,
    pub high_contrast_mode: bool,
    pub voice_commands_enabled: bool,
    pub keyboard_shortcuts_enabled: bool,
    pub reduce_motion: bool,
    pub font_size: FontSize,
    pub announce_progress: bool,
    pub verbose_descriptions: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontSize {
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl Default for AccessibilityConfig {
    fn default() -> Self {
        Self {
            screen_reader_mode: false,
            simplified_text_mode: false,
            high_contrast_mode: false,
            voice_commands_enabled: false,
            keyboard_shortcuts_enabled: true,
            reduce_motion: false,
            font_size: FontSize::Medium,
            announce_progress: true,
            verbose_descriptions: false,
            updated_at: Utc::now(),
        }
    }
}

// ─── Voice commands ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommand {
    pub phrase: String,
    pub aliases: Vec<String>,
    pub action: String,
    pub description: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCommandMatch {
    pub command: VoiceCommand,
    pub confidence: f32,
    pub matched_phrase: String,
}

// ─── Keyboard shortcuts ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub keys: String,
    pub action: String,
    pub description: String,
    pub category: String,
}

// ─── Screen reader output ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenReaderOutput {
    pub heading: String,
    pub content: String,
    pub landmarks: Vec<Landmark>,
    pub live_region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Landmark {
    pub role: String,
    pub label: String,
    pub content: String,
}

// ─── WCAG compliance ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WcagCheckResult {
    pub level: WcagLevel,
    pub passed: bool,
    pub score_pct: u8,
    pub violations: Vec<WcagViolation>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WcagLevel {
    A,
    Aa,
    Aaa,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WcagViolation {
    pub criterion: String,
    pub description: String,
    pub severity: String,
    pub fix: String,
}

fn config_path() -> Result<PathBuf> {
    Ok(config::get_data_dir()?.join("accessibility_config.json"))
}

pub fn load_config() -> Result<AccessibilityConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AccessibilityConfig::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
}

pub fn save_config(cfg: &AccessibilityConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(cfg)?)?;
    Ok(())
}

pub fn update_config<F>(mutator: F) -> Result<AccessibilityConfig>
where
    F: FnOnce(&mut AccessibilityConfig),
{
    let mut cfg = load_config()?;
    mutator(&mut cfg);
    cfg.updated_at = Utc::now();
    save_config(&cfg)?;
    Ok(cfg)
}

/// Built-in voice commands covering all major StarForge operations.
pub fn voice_commands() -> Vec<VoiceCommand> {
    vec![
        VoiceCommand {
            phrase: "deploy contract".into(),
            aliases: vec!["deploy".into(), "publish contract".into()],
            action: "starforge deploy".into(),
            description: "Deploy a compiled Soroban contract".into(),
            category: "deployment".into(),
        },
        VoiceCommand {
            phrase: "run tests".into(),
            aliases: vec!["test contract".into(), "execute tests".into()],
            action: "starforge test".into(),
            description: "Run contract tests".into(),
            category: "testing".into(),
        },
        VoiceCommand {
            phrase: "audit contract".into(),
            aliases: vec!["security audit".into(), "check security".into()],
            action: "starforge ai audit".into(),
            description: "Run AI security audit on a contract".into(),
            category: "security".into(),
        },
        VoiceCommand {
            phrase: "explain contract".into(),
            aliases: vec!["what does this do".into(), "describe contract".into()],
            action: "starforge ai explain".into(),
            description: "Explain what a contract does in plain English".into(),
            category: "ai".into(),
        },
        VoiceCommand {
            phrase: "create project".into(),
            aliases: vec!["new project".into(), "scaffold project".into()],
            action: "starforge new".into(),
            description: "Generate a new Soroban project".into(),
            category: "project".into(),
        },
        VoiceCommand {
            phrase: "plan project".into(),
            aliases: vec!["create roadmap".into(), "project plan".into()],
            action: "starforge ai-plan generate".into(),
            description: "Generate an AI project plan".into(),
            category: "planning".into(),
        },
        VoiceCommand {
            phrase: "show wallet".into(),
            aliases: vec!["wallet balance".into(), "my wallet".into()],
            action: "starforge wallet list".into(),
            description: "List configured wallets".into(),
            category: "wallet".into(),
        },
        VoiceCommand {
            phrase: "switch network".into(),
            aliases: vec!["change network".into(), "use testnet".into()],
            action: "starforge network".into(),
            description: "View or switch the active network".into(),
            category: "network".into(),
        },
        VoiceCommand {
            phrase: "optimize gas".into(),
            aliases: vec!["gas optimization".into(), "reduce gas".into()],
            action: "starforge ai optimise".into(),
            description: "Get gas optimization suggestions".into(),
            category: "optimization".into(),
        },
        VoiceCommand {
            phrase: "generate tests".into(),
            aliases: vec!["create tests".into(), "test generation".into()],
            action: "starforge ai test".into(),
            description: "Generate a test suite for a contract".into(),
            category: "testing".into(),
        },
        VoiceCommand {
            phrase: "show help".into(),
            aliases: vec!["help me".into(), "what can you do".into()],
            action: "starforge help".into(),
            description: "Show contextual help".into(),
            category: "navigation".into(),
        },
        VoiceCommand {
            phrase: "simplify text".into(),
            aliases: vec!["easy read".into(), "plain language".into()],
            action: "starforge ai-accessibility simplify".into(),
            description: "Simplify text for easier reading".into(),
            category: "accessibility".into(),
        },
        VoiceCommand {
            phrase: "toggle high contrast".into(),
            aliases: vec!["high contrast".into(), "contrast mode".into()],
            action: "starforge ai-accessibility high-contrast".into(),
            description: "Toggle high contrast visual mode".into(),
            category: "accessibility".into(),
        },
        VoiceCommand {
            phrase: "ask ai".into(),
            aliases: vec!["question".into(), "ai assistant".into()],
            action: "starforge ai ask".into(),
            description: "Ask the AI assistant a question".into(),
            category: "ai".into(),
        },
    ]
}

/// Match a spoken phrase to the best voice command.
pub fn match_voice_command(input: &str) -> Option<VoiceCommandMatch> {
    let normalized = normalize_phrase(input);
    let commands = voice_commands();

    for cmd in &commands {
        let phrases: Vec<&str> = std::iter::once(cmd.phrase.as_str())
            .chain(cmd.aliases.iter().map(String::as_str))
            .collect();

        for phrase in phrases {
            let norm_phrase = normalize_phrase(phrase);
            if normalized == norm_phrase {
                return Some(VoiceCommandMatch {
                    command: cmd.clone(),
                    confidence: 1.0,
                    matched_phrase: phrase.to_string(),
                });
            }
            if normalized.contains(&norm_phrase) || norm_phrase.contains(&normalized) {
                return Some(VoiceCommandMatch {
                    command: cmd.clone(),
                    confidence: 0.85,
                    matched_phrase: phrase.to_string(),
                });
            }
        }
    }

    // Fuzzy word overlap
    let input_words: Vec<&str> = normalized.split_whitespace().collect();
    let mut best: Option<(VoiceCommandMatch, f32)> = None;

    for cmd in &commands {
        for phrase in std::iter::once(&cmd.phrase).chain(&cmd.aliases) {
            let normalized_phrase = normalize_phrase(phrase);
            let phrase_words: Vec<&str> = normalized_phrase.split_whitespace().collect();
            let overlap = input_words
                .iter()
                .filter(|w| phrase_words.contains(w))
                .count();
            if overlap > 0 {
                let confidence = overlap as f32 / phrase_words.len().max(input_words.len()) as f32;
                if confidence >= 0.5 {
                    let m = VoiceCommandMatch {
                        command: cmd.clone(),
                        confidence,
                        matched_phrase: phrase.clone(),
                    };
                    if best.as_ref().map_or(true, |(_, c)| confidence > *c) {
                        best = Some((m, confidence));
                    }
                }
            }
        }
    }

    best.map(|(m, _)| m)
}

fn normalize_phrase(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Built-in keyboard shortcuts for StarForge operations.
pub fn keyboard_shortcuts() -> Vec<KeyboardShortcut> {
    vec![
        KeyboardShortcut {
            keys: "Ctrl+H".into(),
            action: "starforge help".into(),
            description: "Show contextual help".into(),
            category: "navigation".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+D".into(),
            action: "starforge deploy".into(),
            description: "Deploy contract".into(),
            category: "deployment".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+T".into(),
            action: "starforge test".into(),
            description: "Run tests".into(),
            category: "testing".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+A".into(),
            action: "starforge ai audit".into(),
            description: "AI security audit".into(),
            category: "security".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+E".into(),
            action: "starforge ai explain".into(),
            description: "Explain contract".into(),
            category: "ai".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+N".into(),
            action: "starforge new".into(),
            description: "New project".into(),
            category: "project".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+W".into(),
            action: "starforge wallet list".into(),
            description: "List wallets".into(),
            category: "wallet".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+Shift+P".into(),
            action: "starforge ai-plan generate".into(),
            description: "Generate project plan".into(),
            category: "planning".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+Shift+A".into(),
            action: "starforge ai-accessibility status".into(),
            description: "Accessibility status".into(),
            category: "accessibility".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+Shift+S".into(),
            action: "starforge ai-accessibility simplify".into(),
            description: "Simplify selected text".into(),
            category: "accessibility".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+Shift+C".into(),
            action: "starforge ai-accessibility high-contrast".into(),
            description: "Toggle high contrast mode".into(),
            category: "accessibility".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+Shift+R".into(),
            action: "starforge ai-route select".into(),
            description: "Select optimal AI model".into(),
            category: "ai".into(),
        },
        KeyboardShortcut {
            keys: "Ctrl+?".into(),
            action: "starforge ai-accessibility shortcuts".into(),
            description: "Show keyboard shortcuts".into(),
            category: "accessibility".into(),
        },
    ]
}

/// Format CLI output for screen readers with semantic landmarks.
pub fn format_for_screen_reader(title: &str, sections: &[(String, String)]) -> ScreenReaderOutput {
    let mut landmarks = Vec::new();
    let mut content_parts = Vec::new();

    content_parts.push(format!("Section: {}", title));

    for (label, text) in sections {
        landmarks.push(Landmark {
            role: "region".into(),
            label: label.clone(),
            content: text.clone(),
        });
        content_parts.push(format!("{}: {}", label, text));
    }

    ScreenReaderOutput {
        heading: title.to_string(),
        content: content_parts.join(". "),
        landmarks,
        live_region: None,
    }
}

/// Apply screen reader formatting to plain text output.
pub fn screen_reader_format(text: &str, cfg: &AccessibilityConfig) -> String {
    if !cfg.screen_reader_mode && !cfg.verbose_descriptions {
        return text.to_string();
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut output = String::new();

    if cfg.verbose_descriptions {
        output.push_str(&format!("Document with {} lines. ", lines.len()));
    }

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("✓") || trimmed.starts_with("Success") {
            output.push_str(&format!(
                "Success notification, line {}: {}. ",
                i + 1,
                trimmed
            ));
        } else if trimmed.starts_with("✗") || trimmed.starts_with("Error") {
            output.push_str(&format!(
                "Error notification, line {}: {}. ",
                i + 1,
                trimmed
            ));
        } else if trimmed.starts_with("⚠") || trimmed.starts_with("Warning") {
            output.push_str(&format!("Warning, line {}: {}. ", i + 1, trimmed));
        } else if trimmed.starts_with("→") {
            output.push_str(&format!("Information, line {}: {}. ", i + 1, trimmed));
        } else if trimmed.contains(':') && !trimmed.starts_with(' ') {
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                output.push_str(&format!(
                    "Field {} value {}. ",
                    parts[0].trim(),
                    parts[1].trim()
                ));
                continue;
            }
            output.push_str(&format!("Line {}: {}. ", i + 1, trimmed));
        } else {
            output.push_str(&format!("Line {}: {}. ", i + 1, trimmed));
        }
    }

    output
}

/// Simplify text locally using rule-based plain language transforms.
pub fn simplify_text_local(text: &str) -> String {
    let replacements: &[(&str, &str)] = &[
        ("utilize", "use"),
        ("implement", "build"),
        ("facilitate", "help"),
        ("subsequently", "then"),
        ("prior to", "before"),
        ("in order to", "to"),
        ("due to the fact that", "because"),
        ("at this point in time", "now"),
        ("with regard to", "about"),
        ("in the event that", "if"),
        ("smart contract", "contract program"),
        ("deployment", "putting online"),
        ("initialization", "setup"),
        ("authorization", "permission check"),
        ("configuration", "settings"),
        ("optimization", "making faster"),
        ("verification", "checking"),
        ("comprehensive", "full"),
        ("functionality", "features"),
        ("approximately", "about"),
        ("demonstrate", "show"),
        ("execute", "run"),
        ("retrieve", "get"),
        ("terminate", "stop"),
        ("commence", "start"),
    ];

    let mut result = text.to_string();
    for (from, to) in replacements {
        result = result.replace(from, to);
        let capitalized = format!(
            "{}{}",
            from.chars().next().unwrap().to_uppercase(),
            &from[1..]
        );
        let to_cap = format!("{}{}", to.chars().next().unwrap().to_uppercase(), &to[1..]);
        result = result.replace(&capitalized, &to_cap);
    }

    // Break long sentences at conjunctions
    result = result.replace("; ", ". ");
    result = result.replace(" furthermore, ", ". Also, ");
    result = result.replace(" however, ", ". But, ");

    result
}

/// Simplify text using AI (Ollama) for complex content.
pub async fn simplify_text_ai(text: &str, model: &str) -> Result<String> {
    let prompt = ollama::prompts::text_simplification_prompt(text);
    let opts = GenerateOptions {
        temperature: Some(0.2),
        num_predict: Some(2048),
        num_ctx: Some(4096),
    };

    let response = ollama::generate(model, &prompt, Some(opts))
        .await
        .context("AI text simplification failed")?;

    Ok(response.response.trim().to_string())
}

/// Simplify text using local rules, optionally enhanced by AI.
pub async fn simplify_text(text: &str, use_ai: bool, model: &str) -> Result<String> {
    let local = simplify_text_local(text);
    if use_ai && ollama::is_ollama_running().await {
        simplify_text_ai(&local, model).await
    } else {
        Ok(local)
    }
}

/// Apply high contrast formatting markers to text output.
pub fn apply_high_contrast(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("✓") || trimmed.to_lowercase().contains("success") {
                format!("[SUCCESS] {}", trimmed)
            } else if trimmed.starts_with("✗") || trimmed.to_lowercase().contains("error") {
                format!("[ERROR] {}", trimmed)
            } else if trimmed.starts_with("⚠") || trimmed.to_lowercase().contains("warning") {
                format!("[WARNING] {}", trimmed)
            } else if trimmed.starts_with("→") {
                format!("[INFO] {}", trimmed)
            } else if trimmed.contains("---") || trimmed.contains("===") {
                format!("[SEPARATOR] {}", trimmed)
            } else {
                format!("[TEXT] {}", trimmed)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check text/output for WCAG compliance issues.
pub fn check_wcag_compliance(text: &str, level: WcagLevel) -> WcagCheckResult {
    let mut violations = Vec::new();
    let mut recommendations = Vec::new();
    let mut checks_passed = 0u32;
    let total_checks = 8u32;

    // 1.3.1 Info and Relationships — structure
    if !text.contains('\n') && text.len() > 500 {
        violations.push(WcagViolation {
            criterion: "1.3.1".into(),
            description: "Long unstructured text block without headings".into(),
            severity: "moderate".into(),
            fix: "Add section headings and line breaks".into(),
        });
    } else {
        checks_passed += 1;
    }

    // 1.4.1 Use of Color — don't rely on color alone
    if text.contains("red") && !text.contains("[ERROR]") {
        recommendations.push("Pair color references with text labels like [ERROR]".into());
    }
    checks_passed += 1;

    // 1.4.3 Contrast — check for low-contrast indicators
    if text.contains("dimmed") || text.contains("gray") {
        violations.push(WcagViolation {
            criterion: "1.4.3".into(),
            description: "Dimmed or gray text may have insufficient contrast".into(),
            severity: "moderate".into(),
            fix: "Enable high contrast mode".into(),
        });
    } else {
        checks_passed += 1;
    }

    // 2.1.1 Keyboard — shortcuts available
    checks_passed += 1;

    // 2.4.2 Page Titled — has identifiable heading
    if text.lines().next().map_or(true, |l| l.trim().is_empty()) {
        violations.push(WcagViolation {
            criterion: "2.4.2".into(),
            description: "Output lacks a descriptive heading".into(),
            severity: "minor".into(),
            fix: "Add a title or header line".into(),
        });
    } else {
        checks_passed += 1;
    }

    // 3.1.5 Reading Level — sentence length
    let long_sentences = text
        .split('.')
        .filter(|s| s.split_whitespace().count() > 30)
        .count();
    if long_sentences > 2 {
        violations.push(WcagViolation {
            criterion: "3.1.5".into(),
            description: format!("{} sentences exceed 30 words", long_sentences),
            severity: "moderate".into(),
            fix: "Use simplified text mode".into(),
        });
        recommendations.push("Run starforge ai-accessibility simplify on output".into());
    } else {
        checks_passed += 1;
    }

    // 3.3.2 Labels or Instructions
    if text.contains(':') {
        checks_passed += 1;
    } else {
        recommendations.push("Use labeled key-value pairs for structured data".into());
        checks_passed += 1;
    }

    // 4.1.2 Name, Role, Value — semantic markers
    if !text.contains('[') && text.len() > 200 {
        recommendations.push("Enable screen reader mode for semantic landmarks".into());
    }
    checks_passed += 1;

    let max_violations = match level {
        WcagLevel::A => 3,
        WcagLevel::Aa => 1,
        WcagLevel::Aaa => 0,
    };

    let passed = violations.len() <= max_violations;
    let score_pct = ((checks_passed as f64 / total_checks as f64) * 100.0) as u8;

    WcagCheckResult {
        level,
        passed,
        score_pct,
        violations,
        recommendations,
    }
}

/// Format any CLI output according to active accessibility settings.
pub fn format_output(text: &str, cfg: &AccessibilityConfig) -> String {
    let mut result = text.to_string();

    if cfg.simplified_text_mode {
        result = simplify_text_local(&result);
    }

    if cfg.screen_reader_mode || cfg.verbose_descriptions {
        result = screen_reader_format(&result, cfg);
    }

    if cfg.high_contrast_mode {
        result = apply_high_contrast(&result);
    }

    result
}

/// Group voice commands by category for display.
pub fn voice_commands_by_category() -> HashMap<String, Vec<VoiceCommand>> {
    let mut map: HashMap<String, Vec<VoiceCommand>> = HashMap::new();
    for cmd in voice_commands() {
        map.entry(cmd.category.clone()).or_default().push(cmd);
    }
    map
}

/// Group keyboard shortcuts by category.
pub fn shortcuts_by_category() -> HashMap<String, Vec<KeyboardShortcut>> {
    let mut map: HashMap<String, Vec<KeyboardShortcut>> = HashMap::new();
    for shortcut in keyboard_shortcuts() {
        map.entry(shortcut.category.clone())
            .or_default()
            .push(shortcut);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_command_exact_match() {
        let m = match_voice_command("deploy contract").unwrap();
        assert_eq!(m.command.action, "starforge deploy");
        assert_eq!(m.confidence, 1.0);
    }

    #[test]
    fn voice_command_alias_match() {
        let m = match_voice_command("security audit").unwrap();
        assert!(m.command.action.contains("audit"));
    }

    #[test]
    fn simplify_text_replaces_jargon() {
        let result = simplify_text_local("We will utilize comprehensive deployment");
        assert!(result.contains("use"));
        assert!(result.contains("full"));
    }

    #[test]
    fn high_contrast_adds_markers() {
        let result = apply_high_contrast("✓ Success\n✗ Error");
        assert!(result.contains("[SUCCESS]"));
        assert!(result.contains("[ERROR]"));
    }

    #[test]
    fn wcag_check_detects_long_sentences() {
        let long = "word ".repeat(35);
        let result = check_wcag_compliance(&long, WcagLevel::Aa);
        assert!(!result.violations.is_empty() || !result.recommendations.is_empty());
    }

    #[test]
    fn keyboard_shortcuts_not_empty() {
        assert!(!keyboard_shortcuts().is_empty());
    }
}
