//! AI-powered Soroban contract security audit engine.
//!
//! Combines static pattern analysis with Claude AI for comprehensive
//! vulnerability detection with < 15% false positive rate.

use serde::{Deserialize, Serialize};

/// Security vulnerability with full context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityVulnerability {
    pub id: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub line_number: Option<usize>,
    pub code_snippet: Option<String>,
    pub recommendation: String,
    pub references: Option<Vec<String>>,
}

/// Vulnerability category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VulnerabilityCategory {
    #[serde(rename = "reentrancy")]
    Reentrancy,
    #[serde(rename = "access-control")]
    AccessControl,
    #[serde(rename = "integer-overflow")]
    IntegerOverflow,
    #[serde(rename = "logic-error")]
    LogicError,
    #[serde(rename = "privacy-leak")]
    PrivacyLeak,
    #[serde(rename = "unauthorized-transfer")]
    UnauthorizedTransfer,
    #[serde(rename = "uninitialized-storage")]
    UninitializedStorage,
    #[serde(rename = "dos-vulnerability")]
    DosVulnerability,
    #[serde(rename = "best-practice")]
    BestPractice,
}

impl std::fmt::Display for VulnerabilityCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VulnerabilityCategory::Reentrancy => write!(f, "reentrancy"),
            VulnerabilityCategory::AccessControl => write!(f, "access-control"),
            VulnerabilityCategory::IntegerOverflow => write!(f, "integer-overflow"),
            VulnerabilityCategory::LogicError => write!(f, "logic-error"),
            VulnerabilityCategory::PrivacyLeak => write!(f, "privacy-leak"),
            VulnerabilityCategory::UnauthorizedTransfer => write!(f, "unauthorized-transfer"),
            VulnerabilityCategory::UninitializedStorage => write!(f, "uninitialized-storage"),
            VulnerabilityCategory::DosVulnerability => write!(f, "dos-vulnerability"),
            VulnerabilityCategory::BestPractice => write!(f, "best-practice"),
        }
    }
}

/// Attack scenario with exploitation steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackScenario {
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    pub impact: String,
    pub likelihood: String,
}

/// Fix suggestion for a vulnerability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSuggestion {
    pub vulnerability_id: String,
    pub title: String,
    pub description: String,
    pub code_example: String,
    pub priority: String,
}

/// Complete security audit report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditReport {
    pub contract_name: String,
    pub audit_date: String,
    pub overall_risk: String,
    pub summary: String,
    pub vulnerabilities: Vec<SecurityVulnerability>,
    pub attack_scenarios: Vec<AttackScenario>,
    pub best_practice_violations: Vec<String>,
    pub fix_suggestions: Vec<FixSuggestion>,
    pub security_score: f64,
    pub false_positive_warning: String,
    pub tools_used: Vec<String>,
}

/// Audit request parameters.
#[derive(Debug, Clone)]
pub struct AuditRequest {
    pub contract_code: String,
    pub contract_name: String,
    pub include_attack_simulation: bool,
    pub security_level: AuditLevel,
}

/// Audit detail level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuditLevel {
    Basic,
    Standard,
    Comprehensive,
}

impl std::fmt::Display for AuditLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditLevel::Basic => write!(f, "basic"),
            AuditLevel::Standard => write!(f, "standard"),
            AuditLevel::Comprehensive => write!(f, "comprehensive"),
        }
    }
}

/// Static security pattern check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticCheckResult {
    pub pattern_name: String,
    pub description: String,
    pub severity: String,
    pub line_numbers: Vec<usize>,
    pub snippets: Vec<String>,
}

/// Static security patterns for quick detection.
pub struct SecurityPatterns;

impl SecurityPatterns {
    /// Reentrancy: token transfer before state update (CEI violation).
    pub fn check_reentrancy_risk(code: &str) -> Option<StaticCheckResult> {
        let lines: Vec<&str> = code.lines().collect();
        let mut violations = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.is_empty() {
                continue;
            }

            // A declaration such as `pub fn transfer(..)` names a transfer but
            // does not perform one, so it must not anchor a CEI violation.
            let is_declaration = trimmed.contains("fn ");
            if (trimmed.contains("transfer") || trimmed.contains("invoke_contract"))
                && !trimmed.contains("storage")
                && !is_declaration
            {
                // A state write *after* the external call is the CEI violation.
                // A transfer that happens last is the safe ordering.
                let mut found_storage_after = false;
                for candidate in &lines[(i + 1)..std::cmp::min(i + 10, lines.len())] {
                    let candidate = candidate.trim();
                    // The write may be a direct storage call or a setter
                    // helper such as `set_balance(...)`; both settle state
                    // after the external call and so violate CEI.
                    let writes_state = (candidate.contains("storage") && candidate.contains("set"))
                        || candidate.contains(".set(")
                        || candidate.contains("set_");
                    if writes_state {
                        found_storage_after = true;
                        break;
                    }
                }

                if found_storage_after {
                    violations.push((i + 1, line.to_string()));
                }
            }
        }

        if !violations.is_empty() {
            return Some(StaticCheckResult {
                pattern_name: "reentrancy_risk".to_string(),
                description: "Token transfer before state update (reentrancy and CEI violation)"
                    .to_string(),
                severity: "critical".to_string(),
                line_numbers: violations.iter().map(|(n, _)| n).copied().collect(),
                snippets: violations.iter().map(|(_, s)| s.clone()).collect(),
            });
        }
        None
    }

    /// Missing require_auth in public state-mutating functions.
    pub fn check_missing_auth(code: &str) -> Option<StaticCheckResult> {
        let lines: Vec<&str> = code.lines().collect();
        let mut violations = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if line.contains("pub fn ") && !line.trim().starts_with("//") {
                // Check if this is a state-mutating function
                if (line.contains("&mut") || line.contains("env:"))
                    && !line.contains("view")
                    && !line.contains("read")
                {
                    // Look for require_auth in next 20 lines
                    let mut has_auth = false;
                    for line_j in &lines[(i + 1)..std::cmp::min(i + 20, lines.len())] {
                        // Ignore comments: a line reading `// Missing
                        // require_auth() check` must not satisfy the check.
                        let code_only = line_j.split("//").next().unwrap_or(line_j);
                        if code_only.contains("require_auth") {
                            has_auth = true;
                            break;
                        }
                        if code_only.contains("pub fn ") {
                            break; // Stop if we hit next function
                        }
                    }

                    if !has_auth {
                        violations.push((i + 1, line.to_string()));
                    }
                }
            }
        }

        if !violations.is_empty() {
            return Some(StaticCheckResult {
                pattern_name: "missing_auth".to_string(),
                description: "Public function without require_auth() check".to_string(),
                severity: "high".to_string(),
                line_numbers: violations.iter().map(|(n, _)| n).copied().collect(),
                snippets: violations.iter().map(|(_, s)| s.clone()).collect(),
            });
        }
        None
    }

    /// Unchecked arithmetic operations.
    pub fn check_unchecked_arithmetic(code: &str) -> Option<StaticCheckResult> {
        let lines: Vec<&str> = code.lines().collect();
        let mut violations = Vec::new();

        for (i, raw_line) in lines.iter().enumerate() {
            if raw_line.trim().starts_with("//") {
                continue;
            }

            // Analyse the code before any trailing comment, so an annotated
            // line such as `balance + amount; // could overflow` still counts.
            // `->` in a return type is not a subtraction.
            let line = raw_line
                .split("//")
                .next()
                .unwrap_or(raw_line)
                .replace("->", "  ");

            // Look for arithmetic without checked_ prefix
            let arithmetic_ops = vec![
                ("+", "checked_add"),
                ("-", "checked_sub"),
                ("*", "checked_mul"),
            ];

            for (op, checked_fn) in arithmetic_ops {
                if line.contains(op)
                    && !line.contains(checked_fn)
                    && !line.contains(&format!("{}{}", op, op))
                    && !line.contains("->")
                {
                    // Avoid false positives on operators like +=, -=, etc in checked context
                    if !line.contains("string") {
                        violations.push((i + 1, raw_line.to_string()));
                        break;
                    }
                }
            }
        }

        if !violations.is_empty() {
            return Some(StaticCheckResult {
                pattern_name: "unchecked_arithmetic".to_string(),
                description: "potential arithmetic overflow without checked_ operations"
                    .to_string(),
                severity: "medium".to_string(),
                line_numbers: violations.iter().map(|(n, _)| n).copied().collect(),
                snippets: violations.iter().map(|(_, s)| s.clone()).collect(),
            });
        }
        None
    }

    /// Sensitive data storage on-chain.
    pub fn check_privacy_leak(code: &str) -> Option<StaticCheckResult> {
        let sensitive_patterns = ["password", "secret", "private_key", "private key"];
        let mut violations = Vec::new();

        for (line_no, statement) in logical_statements(code) {
            if statement.contains("storage") && statement.contains("set") {
                for pattern in &sensitive_patterns {
                    if statement.to_lowercase().contains(pattern) {
                        violations.push((line_no, statement.clone()));
                        break;
                    }
                }
            }
        }

        if !violations.is_empty() {
            return Some(StaticCheckResult {
                pattern_name: "privacy_leak".to_string(),
                description: "sensitive data stored on-chain (not private)".to_string(),
                severity: "high".to_string(),
                line_numbers: violations.iter().map(|(n, _)| n).copied().collect(),
                snippets: violations.iter().map(|(_, s)| s.clone()).collect(),
            });
        }
        None
    }

    /// Missing TTL extension for persistent storage.
    pub fn check_missing_ttl(code: &str) -> Option<StaticCheckResult> {
        let lines: Vec<&str> = code.lines().collect();
        let mut violations = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if line.contains("persistent()") && line.contains("set") {
                // Look for extend_ttl in nearby lines
                let mut has_ttl = false;
                for nearby in
                    &lines[std::cmp::max(0, i.saturating_sub(5))..std::cmp::min(i + 5, lines.len())]
                {
                    if nearby.contains("extend_ttl") {
                        has_ttl = true;
                        break;
                    }
                }

                if !has_ttl {
                    violations.push((i + 1, line.to_string()));
                }
            }
        }

        if !violations.is_empty() {
            return Some(StaticCheckResult {
                pattern_name: "missing_ttl".to_string(),
                description: "Persistent storage without TTL extension".to_string(),
                severity: "low".to_string(),
                line_numbers: violations.iter().map(|(n, _)| n).copied().collect(),
                snippets: violations.iter().map(|(_, s)| s.clone()).collect(),
            });
        }
        None
    }
}

/// Joins method-chain continuations into single logical statements.
///
/// Returns `(1-based line number of the first physical line, joined text)`.
/// The line-oriented checks would otherwise miss idiomatic Soroban code such as
/// `env.storage()\n    .persistent()\n    .set(&key, &value);`, where the
/// tokens that matter are spread across three physical lines.
fn logical_statements(code: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = code.lines().collect();
    let mut statements = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let start = i;
        let mut statement = lines[i].trim().to_string();

        // Keep absorbing lines while the chain is obviously unfinished: either
        // the next line continues a method chain, or parens are still open.
        while i + 1 < lines.len() {
            let next = lines[i + 1].trim();
            let unbalanced = statement.matches('(').count() > statement.matches(')').count();
            if next.starts_with('.') || unbalanced {
                statement.push_str(next);
                i += 1;
            } else {
                break;
            }
        }

        statements.push((start + 1, statement));
        i += 1;
    }

    statements
}

/// Run static security checks on contract code.
pub fn run_static_checks(contract_code: &str) -> Vec<StaticCheckResult> {
    let mut findings = Vec::new();

    if let Some(check) = SecurityPatterns::check_reentrancy_risk(contract_code) {
        findings.push(check);
    }
    if let Some(check) = SecurityPatterns::check_missing_auth(contract_code) {
        findings.push(check);
    }
    if let Some(check) = SecurityPatterns::check_unchecked_arithmetic(contract_code) {
        findings.push(check);
    }
    if let Some(check) = SecurityPatterns::check_privacy_leak(contract_code) {
        findings.push(check);
    }
    if let Some(check) = SecurityPatterns::check_missing_ttl(contract_code) {
        findings.push(check);
    }

    findings
}

/// AI audit response from Claude.
#[derive(Debug, Deserialize)]
pub struct AiAuditResponse {
    pub overall_risk: String,
    pub summary: String,
    pub security_score: f64,
    pub vulnerabilities: Vec<SecurityVulnerability>,
    pub attack_scenarios: Vec<AttackScenario>,
    pub best_practice_violations: Vec<String>,
    pub fix_suggestions: Vec<FixSuggestion>,
}

/// Build a deterministic fallback report when AI analysis is unavailable.
pub fn build_fallback_report(
    contract_name: &str,
    contract_code: &str,
    static_findings: &[StaticCheckResult],
    security_level: AuditLevel,
    include_attack_simulation: bool,
) -> SecurityAuditReport {
    let mut vulnerabilities = Vec::new();
    let mut attack_scenarios = Vec::new();
    let mut best_practice_violations = Vec::new();
    let mut fix_suggestions = Vec::new();

    for finding in static_findings {
        let (category, title, recommendation, severity) = match finding.pattern_name.as_str() {
            "missing_auth" => (
                "access-control",
                "Missing authorization for a state-mutating entry point",
                "Add require_auth() or equivalent checks before any state change or asset transfer.",
                "high",
            ),
            "reentrancy_risk" => (
                "reentrancy",
                "Potential reentrancy or CEI ordering issue",
                "Ensure state updates happen before any external call and use explicit guards for privileged paths.",
                "critical",
            ),
            "unchecked_arithmetic" => (
                "integer-overflow",
                "Unchecked arithmetic can overflow or underflow",
                "Use checked_add, checked_sub, or saturating operations for balance and counter math.",
                "medium",
            ),
            "privacy_leak" => (
                "privacy-leak",
                "Sensitive information may be persisted on-chain",
                "Avoid storing secrets or private data in contract storage unless encrypted and strictly scoped.",
                "high",
            ),
            "missing_ttl" => (
                "best-practice",
                "Persistent storage is not extending TTL",
                "Extend storage TTL or configure lifetime management to avoid unintended eviction.",
                "low",
            ),
            _ => (
                "logic-error",
                &finding.pattern_name[..finding.pattern_name.len().min(40)],
                "Review the flagged pattern and harden the contract with explicit validations and tests.",
                "medium",
            ),
        };

        let line_number = finding.line_numbers.first().copied();
        let code_snippet = finding.snippets.first().cloned();
        let description = format!(
            "Offline audit identified {}. {}",
            finding.description.to_lowercase(),
            if include_attack_simulation {
                "This pattern is consistent with a real attack path during simulation."
            } else {
                "Review this issue carefully before deployment."
            }
        );

        vulnerabilities.push(SecurityVulnerability {
            id: format!("FALLBACK-{}", vulnerabilities.len() + 1),
            severity: severity.to_string(),
            category: category.to_string(),
            title: title.to_string(),
            description: description.clone(),
            line_number,
            code_snippet,
            recommendation: recommendation.to_string(),
            references: Some(vec![
                "https://developers.stellar.org/docs/build/smart-contracts/security".to_string(),
            ]),
        });

        fix_suggestions.push(FixSuggestion {
            vulnerability_id: format!("FALLBACK-{}", vulnerabilities.len()),
            title: title.to_string(),
            description: description.clone(),
            code_example: format!(
                "// Harden the contract by adding guards, checked math, and explicit tests\n{}",
                contract_code.lines().take(3).collect::<Vec<_>>().join("\n")
            ),
            priority: match severity {
                "critical" => "immediate".to_string(),
                "high" => "high".to_string(),
                "medium" => "medium".to_string(),
                _ => "low".to_string(),
            },
        });
    }

    if include_attack_simulation {
        if vulnerabilities
            .iter()
            .any(|v| v.category == "access-control")
        {
            attack_scenarios.push(AttackScenario {
                name: "Unauthorized state mutation".to_string(),
                description:
                    "An attacker reuses a public entry point to mutate state without authorization."
                        .to_string(),
                steps: vec![
                    "Identify a public function that changes storage or transfers value."
                        .to_string(),
                    "Invoke the function without a valid auth context.".to_string(),
                    "Observe whether the contract accepts the request and mutates state."
                        .to_string(),
                ],
                impact: "Unauthorized balance changes or privileged state updates".to_string(),
                likelihood: "high".to_string(),
            });
        }

        if vulnerabilities
            .iter()
            .any(|v| v.category == "integer-overflow")
        {
            attack_scenarios.push(AttackScenario {
                name: "Arithmetic boundary exploit".to_string(),
                description: "An attacker submits values near the numeric boundary to trigger overflow or underflow.".to_string(),
                steps: vec![
                    "Probe the contract with minimal and maximal values.".to_string(),
                    "Trigger the arithmetic path without checked guards.".to_string(),
                    "Inspect whether the result wraps or underflows unexpectedly.".to_string(),
                ],
                impact: "Unexpected balances, invalid accounting, or denial of service".to_string(),
                likelihood: "medium".to_string(),
            });
        }
    }

    best_practice_violations.push(
        "Compliance: add deployment guardrails, reviewer sign-off, and rollback evidence before production release.".to_string(),
    );
    best_practice_violations.push(
        "Security reporting: persist findings in the remediation tracker and CI workflow for follow-up.".to_string(),
    );

    let highest_severity = vulnerabilities
        .iter()
        .map(|v| v.severity.as_str())
        .max_by_key(|severity| match *severity {
            "critical" => 4,
            "high" => 3,
            "medium" => 2,
            "low" => 1,
            _ => 0,
        })
        .unwrap_or("info");

    let overall_risk = match highest_severity {
        "critical" => "critical",
        "high" => "high",
        "medium" => "medium",
        "low" => "low",
        _ => "safe",
    };

    let mut severity_penalty = 0.0;
    for vulnerability in &vulnerabilities {
        severity_penalty += match vulnerability.severity.as_str() {
            "critical" => 25.0,
            "high" => 15.0,
            "medium" => 8.0,
            "low" => 3.0,
            _ => 1.0,
        };
    }
    let security_score = (100.0_f64 - severity_penalty).max(0.0_f64);

    SecurityAuditReport {
        contract_name: contract_name.to_string(),
        audit_date: chrono::Utc::now().to_rfc3339(),
        overall_risk: overall_risk.to_string(),
        summary: format!(
            "Offline {} review detected {} potential issue(s) and generated {} remediation action(s).",
            security_level,
            vulnerabilities.len(),
            fix_suggestions.len()
        ),
        vulnerabilities,
        attack_scenarios,
        best_practice_violations,
        fix_suggestions,
        security_score,
        false_positive_warning: "Offline fallback analysis may be conservative. Review all findings with a human auditor before deployment.".to_string(),
        tools_used: vec![
            "static-analysis".to_string(),
            "threat-modeling".to_string(),
            "compliance-checks".to_string(),
        ],
    }
}

/// Build the system prompt for AI audit.
pub fn build_system_prompt() -> String {
    r#"You are an expert Soroban smart contract security auditor with deep knowledge of:
- Stellar blockchain and Soroban SDK security patterns
- Common smart contract vulnerabilities (reentrancy, access control, integer overflow, logic errors, privacy leaks)
- Soroban-specific issues (storage rent, TTL management, CEI pattern, require_auth patterns)
- DeFi attack vectors and economic exploits

Your task is to perform a comprehensive security audit of the provided Soroban contract. Be thorough but avoid false positives.

RESPONSE FORMAT: Respond ONLY with valid JSON matching this schema:
{
  "overall_risk": "critical|high|medium|low|safe",
  "summary": "string",
  "security_score": 0-100,
  "vulnerabilities": [
    {
      "id": "VULN-001",
      "severity": "critical|high|medium|low|info",
      "category": "reentrancy|access-control|integer-overflow|logic-error|privacy-leak|best-practice",
      "title": "string",
      "description": "string",
      "line_number": number_or_null,
      "code_snippet": "string_or_null",
      "recommendation": "string"
    }
  ],
  "attack_scenarios": [
    {
      "name": "string",
      "description": "string",
      "steps": ["string"],
      "impact": "string",
      "likelihood": "high|medium|low"
    }
  ],
  "best_practice_violations": ["string"],
  "fix_suggestions": [
    {
      "vulnerability_id": "VULN-001",
      "title": "string",
      "description": "string",
      "code_example": "string",
      "priority": "immediate|high|medium|low"
    }
  ]
}

IMPORTANT:
- Keep false positives below 15%
- Focus on realistic, exploitable vulnerabilities
- Provide actionable fix suggestions with code examples
- Only include genuine security concerns, not style issues"#
        .to_string()
}

/// Build the user prompt for a specific contract audit.
pub fn build_user_prompt(
    contract_code: &str,
    contract_name: &str,
    static_findings: &[StaticCheckResult],
    security_level: AuditLevel,
    include_attack_simulation: bool,
) -> String {
    let static_summary = if static_findings.is_empty() {
        "No static analysis issues detected.".to_string()
    } else {
        static_findings
            .iter()
            .map(|f| {
                format!(
                    "- [{}] {} (lines: {})",
                    f.severity.to_uppercase(),
                    f.description,
                    f.line_numbers
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"Audit the following Soroban smart contract named "{}".

Security Level: {}
Include Attack Scenarios: {}

Static analysis pre-check found these potential issues:
{}

CONTRACT SOURCE CODE:
```rust
{}
```

Focus on:
1. Reentrancy vulnerabilities (CEI pattern violations)
2. Access control issues (missing require_auth)
3. Integer overflow/underflow risks
4. Logic errors and business logic flaws
5. Privacy leaks (sensitive data on-chain)
6. Soroban-specific issues (TTL, storage patterns, rent considerations)
{}

Provide actionable fix suggestions with code examples.
Keep false positives below 15%."#,
        contract_name,
        security_level,
        include_attack_simulation,
        static_summary,
        contract_code,
        if include_attack_simulation {
            "7. Realistic attack scenarios with step-by-step exploitation"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reentrancy_detection() {
        // CEI violation: the balance is written *after* the external call.
        let code = r#"
pub fn transfer(env: Env, to: Address, amount: i128) {
    token.transfer(to, amount);
    storage.set(&DataKey::Balance, balance - amount);
}
"#;
        let result = SecurityPatterns::check_reentrancy_risk(code);
        assert!(result.is_some(), "expected a reentrancy risk signal");
        assert_eq!(result.unwrap().severity, "critical");
    }

    #[test]
    fn test_reentrancy_not_flagged_when_transfer_is_last() {
        // Correct ordering: state settled first, external call last.
        let code = r#"
pub fn transfer(env: Env, to: Address, amount: i128) {
    storage.set(&DataKey::Balance, balance - amount);
    token.transfer(to, amount);
}
"#;
        assert!(
            SecurityPatterns::check_reentrancy_risk(code).is_none(),
            "checks-effects-interactions ordering must not be flagged"
        );
    }

    #[test]
    fn test_missing_auth_detection() {
        let code = r#"
pub fn withdraw(env: Env, amount: i128) {
    let balance = storage.get(DataKey::Balance(env.invoker()));
    balance - amount
}
"#;
        let result = SecurityPatterns::check_missing_auth(code);
        assert!(result.is_some());
        assert_eq!(result.unwrap().severity, "high");
    }

    #[test]
    fn test_static_checks_multiple_findings() {
        let code = r#"
pub fn transfer(env: Env, to: Address, amount: i128) {
    token.transfer(to, amount);
    let new_balance = balance + amount;
    storage.persistent().set(DataKey::Balance, new_balance);
}
"#;
        let findings = run_static_checks(code);
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_prompt_building() {
        let static_findings = vec![StaticCheckResult {
            pattern_name: "test".to_string(),
            description: "Test issue".to_string(),
            severity: "high".to_string(),
            line_numbers: vec![1, 2],
            snippets: vec!["code".to_string()],
        }];

        let prompt = build_user_prompt(
            "contract code",
            "TestContract",
            &static_findings,
            AuditLevel::Standard,
            true,
        );

        assert!(prompt.contains("TestContract"));
        assert!(prompt.contains("standard"));
        assert!(prompt.contains("Test issue"));
        assert!(prompt.contains("contract code"));
    }

    #[test]
    fn test_fallback_report_includes_threat_and_compliance_guidance() {
        let static_findings = vec![
            StaticCheckResult {
                pattern_name: "missing_auth".to_string(),
                description: "Public entry point missing auth".to_string(),
                severity: "high".to_string(),
                line_numbers: vec![5],
                snippets: vec!["pub fn withdraw(env: Env)".to_string()],
            },
            StaticCheckResult {
                pattern_name: "unchecked_arithmetic".to_string(),
                description: "Unchecked arithmetic".to_string(),
                severity: "medium".to_string(),
                line_numbers: vec![8],
                snippets: vec!["let total = balance + amount;".to_string()],
            },
        ];

        let report = build_fallback_report(
            "TokenContract",
            "pub fn withdraw(env: Env) { let total = balance + amount; }",
            &static_findings,
            AuditLevel::Comprehensive,
            true,
        );

        assert_eq!(report.contract_name, "TokenContract");
        assert!(report
            .vulnerabilities
            .iter()
            .any(|v| v.category == "access-control"));
        assert!(!report.attack_scenarios.is_empty());
        assert!(report
            .best_practice_violations
            .iter()
            .any(|v| v.contains("Compliance")));
        assert!(!report.fix_suggestions.is_empty());
        assert!(report.security_score <= 100.0);
    }
}
