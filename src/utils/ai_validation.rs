use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub passed: bool,
    pub layers: Vec<LayerResult>,
    pub sanitized_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    pub enable_syntax_check: bool,
    pub enable_security_check: bool,
    pub enable_compatibility_check: bool,
    pub enable_format_check: bool,
    pub enable_content_filter: bool,
    pub dangerous_patterns: Vec<String>,
    pub blocked_content_patterns: Vec<String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enable_syntax_check: true,
            enable_security_check: true,
            enable_compatibility_check: true,
            enable_format_check: true,
            enable_content_filter: true,
            dangerous_patterns: vec![
                "unsafe".into(),
                "std::process::Command".into(),
                "std::fs::remove".into(),
                "eval!".into(),
                "env::var".into(),
                "reqwest::get".into(),
            ],
            blocked_content_patterns: vec![
                "password".into(),
                "secret".into(),
                "private_key".into(),
                "api_key".into(),
            ],
        }
    }
}

pub struct AIResponseValidator {
    config: ValidationConfig,
}

impl AIResponseValidator {
    pub fn new(config: ValidationConfig) -> Self {
        Self { config }
    }

    pub fn validate(&self, response: &str, language: &str) -> ValidationResult {
        let mut layers = Vec::new();

        if self.config.enable_syntax_check {
            layers.push(self.check_syntax(response, language));
        }

        if self.config.enable_security_check {
            layers.push(self.check_security(response));
        }

        if self.config.enable_compatibility_check {
            layers.push(self.check_soroban_compatibility(response));
        }

        if self.config.enable_format_check {
            layers.push(self.check_format(response));
        }

        if self.config.enable_content_filter {
            layers.push(self.check_content(response));
        }

        let passed = layers
            .iter()
            .all(|l| l.passed || matches!(l.severity, Severity::Warning));
        let sanitized = if passed {
            Some(response.to_string())
        } else {
            None
        };

        ValidationResult {
            passed,
            layers,
            sanitized_output: sanitized,
        }
    }

    fn check_syntax(&self, response: &str, language: &str) -> LayerResult {
        match language {
            "rust" => self.check_rust_syntax(response),
            "javascript" | "typescript" => self.check_js_syntax(response),
            _ => LayerResult {
                name: "syntax".into(),
                passed: true,
                message: format!("No syntax checker for language: {}", language),
                severity: Severity::Info,
            },
        }
    }

    fn check_rust_syntax(&self, code: &str) -> LayerResult {
        let mut issues = Vec::new();

        let open_braces = code.matches('{').count();
        let close_braces = code.matches('}').count();
        if open_braces != close_braces {
            issues.push(format!(
                "Mismatched braces: {} open, {} close",
                open_braces, close_braces
            ));
        }

        let open_parens = code.matches('(').count();
        let close_parens = code.matches(')').count();
        if open_parens != close_parens {
            issues.push(format!(
                "Mismatched parentheses: {} open, {} close",
                open_parens, close_parens
            ));
        }

        let open_brackets = code.matches('[').count();
        let close_brackets = code.matches(']').count();
        if open_brackets != close_brackets {
            issues.push(format!(
                "Mismatched brackets: {} open, {} close",
                open_brackets, close_brackets
            ));
        }

        if code.contains("fn ") && !code.contains('{') {
            issues.push("Function declaration without body".into());
        }

        if issues.is_empty() {
            LayerResult {
                name: "syntax".into(),
                passed: true,
                message: "Rust syntax basic checks passed".into(),
                severity: Severity::Info,
            }
        } else {
            LayerResult {
                name: "syntax".into(),
                passed: false,
                message: issues.join("; "),
                severity: Severity::Error,
            }
        }
    }

    fn check_js_syntax(&self, code: &str) -> LayerResult {
        let mut issues = Vec::new();

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        if open != close {
            issues.push(format!("Mismatched braces: {} vs {}", open, close));
        }

        if issues.is_empty() {
            LayerResult {
                name: "syntax".into(),
                passed: true,
                message: "JS/TS syntax basic checks passed".into(),
                severity: Severity::Info,
            }
        } else {
            LayerResult {
                name: "syntax".into(),
                passed: false,
                message: issues.join("; "),
                severity: Severity::Error,
            }
        }
    }

    fn check_security(&self, response: &str) -> LayerResult {
        let mut findings = Vec::new();

        for pattern in &self.config.dangerous_patterns {
            if response.contains(pattern.as_str()) {
                findings.push(format!("Dangerous pattern detected: {}", pattern));
            }
        }

        if findings.is_empty() {
            LayerResult {
                name: "security".into(),
                passed: true,
                message: "No dangerous patterns found".into(),
                severity: Severity::Info,
            }
        } else {
            LayerResult {
                name: "security".into(),
                passed: false,
                message: findings.join("; "),
                severity: Severity::Error,
            }
        }
    }

    fn check_soroban_compatibility(&self, response: &str) -> LayerResult {
        let mut warnings: Vec<String> = Vec::new();

        if response.contains("#[no_mangle]") && !response.contains("extern") {
            warnings.push("#[no_mangle] without extern may not be Soroban-compatible".into());
        }

        if response.contains("println!") && response.contains("#[contract]") {
            warnings.push("println! in contract code uses host resources".into());
        }

        if response.contains("std::thread::sleep") {
            warnings.push("std::thread::sleep is not available in Soroban environment".into());
        }

        if warnings.is_empty() {
            LayerResult {
                name: "soroban_compatibility".into(),
                passed: true,
                message: "No Soroban compatibility issues detected".into(),
                severity: Severity::Info,
            }
        } else {
            LayerResult {
                name: "soroban_compatibility".into(),
                passed: false,
                message: warnings.join("; "),
                severity: Severity::Warning,
            }
        }
    }

    fn check_format(&self, response: &str) -> LayerResult {
        if response.trim().is_empty() {
            return LayerResult {
                name: "format".into(),
                passed: false,
                message: "Response is empty".into(),
                severity: Severity::Error,
            };
        }

        let has_code =
            response.contains("```") || response.contains("fn ") || response.contains("pub ");
        if !has_code && response.len() > 500 {
            return LayerResult {
                name: "format".into(),
                passed: false,
                message: "Long response without code block markers".into(),
                severity: Severity::Warning,
            };
        }

        LayerResult {
            name: "format".into(),
            passed: true,
            message: "Format check passed".into(),
            severity: Severity::Info,
        }
    }

    fn check_content(&self, response: &str) -> LayerResult {
        let lower = response.to_lowercase();
        for pattern in &self.config.blocked_content_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return LayerResult {
                    name: "content_filter".into(),
                    passed: false,
                    message: format!("Blocked content pattern detected: {}", pattern),
                    severity: Severity::Error,
                };
            }
        }

        LayerResult {
            name: "content_filter".into(),
            passed: true,
            message: "Content filter passed".into(),
            severity: Severity::Info,
        }
    }

    pub fn validate_and_sanitize(&self, response: &str, language: &str) -> Result<String> {
        let result = self.validate(response, language);

        if result.passed {
            Ok(result
                .sanitized_output
                .unwrap_or_else(|| response.to_string()))
        } else {
            let errors: Vec<String> = result
                .layers
                .iter()
                .filter(|l| !l.passed && matches!(l.severity, Severity::Error))
                .map(|l| format!("[{}] {}", l.name, l.message))
                .collect();

            if errors.is_empty() {
                Ok(response.to_string())
            } else {
                anyhow::bail!("Validation failed: {}", errors.join("; "));
            }
        }
    }
}

pub fn extract_code_blocks(text: &str) -> Vec<(Option<String>, String)> {
    let mut blocks = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("```") {
        let after_fence = &remaining[start + 3..];
        let lang_end = after_fence.find('\n').unwrap_or(after_fence.len());
        let lang_line = &after_fence[..lang_end].trim();
        let lang = if lang_line.is_empty() {
            None
        } else {
            Some(lang_line.to_string())
        };

        let code_start = start + 3 + lang_end + 1;
        if code_start > remaining.len() {
            break;
        }

        let rest = &remaining[code_start..];
        if let Some(end) = rest.find("```") {
            let code = rest[..end].trim().to_string();
            blocks.push((lang, code));
            remaining = &rest[end + 3..];
        } else {
            let code = rest.trim().to_string();
            blocks.push((lang, code));
            break;
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_rust() {
        let validator = AIResponseValidator::new(ValidationConfig::default());
        let code = r#"
            pub fn add(a: u32, b: u32) -> u32 {
                a + b
            }
        "#;
        let result = validator.validate(code, "rust");
        assert!(result.passed);
    }

    #[test]
    fn test_detect_mismatched_braces() {
        let validator = AIResponseValidator::new(ValidationConfig::default());
        let code = "pub fn foo() -> u32 { 42";
        let result = validator.validate(code, "rust");
        assert!(!result.passed);
        assert!(result
            .layers
            .iter()
            .any(|l| l.name == "syntax" && !l.passed));
    }

    #[test]
    fn test_detect_dangerous_pattern() {
        let config = ValidationConfig {
            dangerous_patterns: vec!["std::process::Command".into()],
            ..ValidationConfig::default()
        };
        let validator = AIResponseValidator::new(config);
        let code = r#"use std::process::Command;
        pub fn foo() { Command::new("rm").args(["-rf", "/"]).spawn(); }"#;
        let result = validator.validate(code, "rust");
        assert!(!result.passed);
    }

    #[test]
    fn test_empty_response() {
        let validator = AIResponseValidator::new(ValidationConfig::default());
        let result = validator.validate("", "rust");
        assert!(!result.passed);
    }

    #[test]
    fn test_extract_code_blocks() {
        let text = r#"Here is some code:
```rust
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}
```
And another:
```javascript
function add(a, b) { return a + b; }
```"#;
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, Some("rust".into()));
        assert_eq!(blocks[1].0, Some("javascript".into()));
    }
}
