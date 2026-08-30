//! AI-driven security training: lessons, interactive exercises, progress
//! tracking, and personalized learning paths (issue #576).
//!
//! Vulnerability-pattern lessons are grounded in the same detection logic
//! used by the real security auditor (`crate::utils::security::ai_audit`),
//! so "spot the vulnerability" exercises are graded by actually running the
//! static checks against the exercise code, not just a fixed answer key.

use crate::utils::config;
use crate::utils::security::ai_audit::{SecurityPatterns, VulnerabilityCategory};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ── Content model ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrainingTopic {
    SecureCoding,
    VulnerabilityPatterns,
    ThreatModeling,
    SecurityTesting,
    IncidentResponse,
    Compliance,
}

impl std::fmt::Display for TrainingTopic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TrainingTopic::SecureCoding => "secure-coding",
            TrainingTopic::VulnerabilityPatterns => "vulnerability-patterns",
            TrainingTopic::ThreatModeling => "threat-modeling",
            TrainingTopic::SecurityTesting => "security-testing",
            TrainingTopic::IncidentResponse => "incident-response",
            TrainingTopic::Compliance => "compliance",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SkillLevel {
    Beginner,
    Intermediate,
    Advanced,
}

impl std::fmt::Display for SkillLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SkillLevel::Beginner => "beginner",
            SkillLevel::Intermediate => "intermediate",
            SkillLevel::Advanced => "advanced",
        };
        write!(f, "{}", s)
    }
}

/// A single interactive exercise attached to a lesson.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Exercise {
    /// A multiple-choice question with a single correct option index.
    MultipleChoice {
        id: String,
        prompt: String,
        options: Vec<String>,
        correct_index: usize,
        explanation: String,
    },
    /// A practical exercise: given a code snippet, the learner names which
    /// vulnerability category applies. Graded against the real static
    /// checker in `security::ai_audit`, not a fixed answer key.
    SpotTheVulnerability {
        id: String,
        prompt: String,
        code: String,
        category: VulnerabilityCategory,
        explanation: String,
    },
}

impl Exercise {
    pub fn id(&self) -> &str {
        match self {
            Exercise::MultipleChoice { id, .. } => id,
            Exercise::SpotTheVulnerability { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub topic: TrainingTopic,
    pub level: SkillLevel,
    pub title: String,
    pub content: String,
    pub exercises: Vec<Exercise>,
}

/// The full security-training curriculum (issue #576's six training features).
pub fn all_lessons() -> Vec<Lesson> {
    vec![
        Lesson {
            id: "secure-coding-101".into(),
            topic: TrainingTopic::SecureCoding,
            level: SkillLevel::Beginner,
            title: "Secure Coding Practices for Soroban Contracts".into(),
            content: "Soroban contracts run on-chain and cannot be patched after a bug causes \
                loss of funds. Favor checked arithmetic (`checked_add`/`checked_sub`) over raw \
                operators, always call `env.require_auth()` before mutating state on behalf of \
                a user, and follow checks-effects-interactions: update your own storage BEFORE \
                calling out to another contract (e.g. a token transfer)."
                .into(),
            exercises: vec![Exercise::MultipleChoice {
                id: "secure-coding-101-q1".into(),
                prompt: "Which ordering follows the checks-effects-interactions pattern?".into(),
                options: vec![
                    "Transfer tokens, then update the internal balance".into(),
                    "Update the internal balance, then transfer tokens".into(),
                    "Transfer tokens, then check authorization".into(),
                ],
                correct_index: 1,
                explanation: "State should be updated before any external call (like a token \
                    transfer) to prevent reentrancy: if the external call re-enters your \
                    contract, it will see the already-updated state."
                    .into(),
            }],
        },
        Lesson {
            id: "vulnerability-patterns-101".into(),
            topic: TrainingTopic::VulnerabilityPatterns,
            level: SkillLevel::Beginner,
            title: "Common Vulnerability Patterns".into(),
            content: "The most common Soroban contract vulnerabilities are: reentrancy \
                (external call before state update), missing authorization checks, unchecked \
                arithmetic overflow/underflow, and uninitialized or unexpiring storage. This \
                lesson's exercise runs the real static analyzer used by `starforge ai-audit` \
                against a code snippet — try to name the vulnerability before checking the \
                answer."
                .into(),
            exercises: vec![Exercise::SpotTheVulnerability {
                id: "vulnerability-patterns-101-q1".into(),
                prompt: "What vulnerability category applies to this function?".into(),
                code: "pub fn withdraw(env: Env, to: Address, amount: i128) {\n    let client = token::Client::new(&env, &token_id);\n    client.transfer(&env.current_contract_address(), &to, &amount);\n    let mut balance: i128 = env.storage().persistent().get(&to).unwrap_or(0);\n    balance -= amount;\n    env.storage().persistent().set(&to, &balance);\n}"
                    .into(),
                category: VulnerabilityCategory::Reentrancy,
                explanation: "The token transfer happens BEFORE the balance is decremented. A \
                    malicious token contract could re-enter `withdraw` during the transfer and \
                    withdraw again before the balance is updated — classic reentrancy."
                    .into(),
            }],
        },
        Lesson {
            id: "threat-modeling-101".into(),
            topic: TrainingTopic::ThreatModeling,
            level: SkillLevel::Intermediate,
            title: "Threat Modeling for Smart Contracts".into(),
            content: "Threat modeling means systematically asking: who can call this function, \
                what do they control (inputs, timing, external contracts), and what's the worst \
                they could do? For each public function, enumerate: (1) the caller's privilege \
                level, (2) attacker-controlled inputs, (3) external calls that could be hijacked \
                or re-entered, and (4) the financial/state impact of misuse. Rank threats by \
                likelihood × impact, and prioritise fixes for high-likelihood, high-impact paths \
                (e.g. anything touching token transfers)."
                .into(),
            exercises: vec![Exercise::MultipleChoice {
                id: "threat-modeling-101-q1".into(),
                prompt: "When threat-modeling a public contract function, what should you enumerate first?".into(),
                options: vec![
                    "The gas cost of the function".into(),
                    "Who can call it and what inputs they control".into(),
                    "The function's return type".into(),
                ],
                correct_index: 1,
                explanation: "Threat modeling starts with the attack surface: who can invoke \
                    the function and what data/timing they control, since that determines what \
                    an attacker could actually exploit."
                    .into(),
            }],
        },
        Lesson {
            id: "security-testing-101".into(),
            topic: TrainingTopic::SecurityTesting,
            level: SkillLevel::Intermediate,
            title: "Security Testing Techniques".into(),
            content: "Beyond unit tests, security testing for Soroban contracts should include: \
                fuzz testing with boundary values (0, i128::MAX, negative amounts), property-based \
                tests asserting invariants (e.g. total supply never changes across transfers), \
                and running `starforge ai-audit` / `starforge audit` before every deployment to \
                catch known vulnerability patterns automatically."
                .into(),
            exercises: vec![Exercise::MultipleChoice {
                id: "security-testing-101-q1".into(),
                prompt: "Which value is most important to include in a fuzz test for a transfer function?".into(),
                options: vec![
                    "A typical, expected amount like 100".into(),
                    "Boundary values like 0 and i128::MAX".into(),
                    "The contract's own address as the amount".into(),
                ],
                correct_index: 1,
                explanation: "Boundary values are where overflow, underflow, and off-by-one \
                    bugs actually surface — typical values rarely expose these issues."
                    .into(),
            }],
        },
        Lesson {
            id: "incident-response-101".into(),
            topic: TrainingTopic::IncidentResponse,
            level: SkillLevel::Advanced,
            title: "Incident Response for Deployed Contracts".into(),
            content: "Smart contracts are hard to patch, so incident response focuses on \
                containment and communication: (1) if the contract has a pause/circuit-breaker \
                mechanism, use it immediately, (2) notify users through official channels before \
                attackers can front-run a fix, (3) snapshot on-chain state and transaction history \
                for post-mortem analysis, and (4) if funds are recoverable via a migration \
                contract, prepare and audit that migration before announcing it publicly."
                .into(),
            exercises: vec![Exercise::MultipleChoice {
                id: "incident-response-101-q1".into(),
                prompt: "What is usually the FIRST action after discovering an active exploit against a deployed contract?".into(),
                options: vec![
                    "Write a detailed public post-mortem".into(),
                    "Trigger the pause/circuit-breaker if one exists".into(),
                    "Redeploy an entirely new contract immediately".into(),
                ],
                correct_index: 1,
                explanation: "Containment comes first — pausing the contract (if supported) \
                    stops further losses while the team investigates and prepares a response."
                    .into(),
            }],
        },
        Lesson {
            id: "compliance-101".into(),
            topic: TrainingTopic::Compliance,
            level: SkillLevel::Advanced,
            title: "Compliance Considerations".into(),
            content: "Compliance for on-chain contracts typically covers: access controls that \
                map to regulatory roles (e.g. admin/operator separation via `require_auth`), \
                auditability (structured events for every state-changing action), and data \
                minimisation (avoid storing personally identifiable information on a public \
                ledger). Use `starforge audit` and `starforge ai-audit` as part of your \
                pre-deployment compliance checklist alongside a human review."
                .into(),
            exercises: vec![Exercise::MultipleChoice {
                id: "compliance-101-q1".into(),
                prompt: "Why should personally identifiable information (PII) generally be avoided in on-chain storage?".into(),
                options: vec![
                    "It increases gas costs only".into(),
                    "On-chain data is public and effectively permanent".into(),
                    "PII cannot be serialized with soroban_sdk".into(),
                ],
                correct_index: 1,
                explanation: "Ledger data is public and, for practical purposes, cannot be \
                    deleted — storing PII on-chain creates a permanent public exposure, which \
                    conflicts with most data-protection compliance regimes."
                    .into(),
            }],
        },
    ]
}

pub fn find_lesson(lesson_id: &str) -> Option<Lesson> {
    all_lessons().into_iter().find(|l| l.id == lesson_id)
}

/// Grades a `SpotTheVulnerability` exercise by running the real static
/// security checks against the exercise code and comparing the detected
/// category to the learner's guess, instead of a hardcoded answer key.
pub fn grade_spot_the_vulnerability(code: &str, guess: &VulnerabilityCategory) -> bool {
    let detected = detect_primary_category(code);
    detected.as_ref() == Some(guess)
}

fn detect_primary_category(code: &str) -> Option<VulnerabilityCategory> {
    if SecurityPatterns::check_reentrancy_risk(code).is_some() {
        return Some(VulnerabilityCategory::Reentrancy);
    }
    if SecurityPatterns::check_missing_auth(code).is_some() {
        return Some(VulnerabilityCategory::AccessControl);
    }
    if SecurityPatterns::check_unchecked_arithmetic(code).is_some() {
        return Some(VulnerabilityCategory::IntegerOverflow);
    }
    if SecurityPatterns::check_privacy_leak(code).is_some() {
        return Some(VulnerabilityCategory::PrivacyLeak);
    }
    if SecurityPatterns::check_missing_ttl(code).is_some() {
        return Some(VulnerabilityCategory::UninitializedStorage);
    }
    None
}

// ── Progress tracking & personalization ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExerciseResult {
    pub exercise_id: String,
    pub correct: bool,
    pub attempts: u32,
    pub last_attempt: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingProgress {
    pub completed_lessons: Vec<String>,
    pub exercise_results: HashMap<String, ExerciseResult>,
    pub last_updated: DateTime<Utc>,
}

impl Default for TrainingProgress {
    fn default() -> Self {
        Self {
            completed_lessons: Vec::new(),
            exercise_results: HashMap::new(),
            last_updated: Utc::now(),
        }
    }
}

fn training_dir() -> PathBuf {
    config::config_dir().join("security_training")
}

fn progress_path() -> PathBuf {
    training_dir().join("progress.json")
}

pub fn load_progress() -> Result<TrainingProgress> {
    let path = progress_path();
    if !path.exists() {
        return Ok(TrainingProgress::default());
    }
    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read training progress: {}", path.display()))?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn save_progress(progress: &TrainingProgress) -> Result<()> {
    let dir = training_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create training dir: {}", dir.display()))?;
    fs::write(progress_path(), serde_json::to_string_pretty(progress)?)?;
    Ok(())
}

pub fn reset_progress() -> Result<()> {
    let path = progress_path();
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Records an exercise attempt, marks the parent lesson complete once at
/// least one exercise for it has been answered correctly, and persists.
pub fn record_answer(
    progress: &mut TrainingProgress,
    lesson_id: &str,
    exercise_id: &str,
    correct: bool,
) {
    let entry = progress
        .exercise_results
        .entry(exercise_id.to_string())
        .or_insert_with(|| ExerciseResult {
            exercise_id: exercise_id.to_string(),
            correct: false,
            attempts: 0,
            last_attempt: Utc::now(),
        });
    entry.attempts += 1;
    entry.correct = entry.correct || correct;
    entry.last_attempt = Utc::now();

    if correct && !progress.completed_lessons.iter().any(|l| l == lesson_id) {
        progress.completed_lessons.push(lesson_id.to_string());
    }
    progress.last_updated = Utc::now();
}

/// Personalization: infers a skill level from completed lessons and
/// first-attempt accuracy, so the recommended learning path adapts over time.
pub fn assess_skill_level(progress: &TrainingProgress) -> SkillLevel {
    let completed = progress.completed_lessons.len();
    let total_attempts: u32 = progress.exercise_results.values().map(|r| r.attempts).sum();
    let correct: u32 = progress
        .exercise_results
        .values()
        .filter(|r| r.correct)
        .count() as u32;
    let accuracy = if total_attempts > 0 {
        correct as f64 / progress.exercise_results.len().max(1) as f64
    } else {
        0.0
    };

    if completed >= 5 && accuracy >= 0.8 {
        SkillLevel::Advanced
    } else if completed >= 2 {
        SkillLevel::Intermediate
    } else {
        SkillLevel::Beginner
    }
}

/// Personalization: recommends the next lesson matching (or just above) the
/// learner's current skill level that hasn't been completed yet.
pub fn recommend_next_lesson(progress: &TrainingProgress) -> Option<Lesson> {
    let level = assess_skill_level(progress);
    let lessons = all_lessons();

    lessons
        .iter()
        .filter(|l| !progress.completed_lessons.iter().any(|c| c == &l.id))
        .min_by_key(|l| {
            let level_gap = (l.level as i32 - level as i32).unsigned_abs();
            (level_gap, l.id.clone())
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lessons_cover_six_topics() {
        let lessons = all_lessons();
        let topics: std::collections::HashSet<_> =
            lessons.iter().map(|l| l.topic.to_string()).collect();
        assert_eq!(topics.len(), 6);
    }

    #[test]
    fn find_lesson_by_id() {
        assert!(find_lesson("secure-coding-101").is_some());
        assert!(find_lesson("does-not-exist").is_none());
    }

    #[test]
    fn grades_reentrancy_exercise_correctly() {
        let lesson = find_lesson("vulnerability-patterns-101").unwrap();
        if let Exercise::SpotTheVulnerability { code, category, .. } = &lesson.exercises[0] {
            assert!(grade_spot_the_vulnerability(code, category));
            assert!(!grade_spot_the_vulnerability(
                code,
                &VulnerabilityCategory::AccessControl
            ));
        } else {
            panic!("expected SpotTheVulnerability exercise");
        }
    }

    #[test]
    fn record_answer_marks_lesson_complete_on_correct() {
        let mut progress = TrainingProgress::default();
        record_answer(
            &mut progress,
            "secure-coding-101",
            "secure-coding-101-q1",
            true,
        );
        assert!(progress
            .completed_lessons
            .contains(&"secure-coding-101".to_string()));
        assert_eq!(progress.exercise_results.len(), 1);
    }

    #[test]
    fn record_answer_does_not_complete_lesson_on_incorrect() {
        let mut progress = TrainingProgress::default();
        record_answer(
            &mut progress,
            "secure-coding-101",
            "secure-coding-101-q1",
            false,
        );
        assert!(!progress
            .completed_lessons
            .contains(&"secure-coding-101".to_string()));
    }

    #[test]
    fn skill_level_starts_beginner() {
        let progress = TrainingProgress::default();
        assert_eq!(assess_skill_level(&progress), SkillLevel::Beginner);
    }

    #[test]
    fn recommend_next_lesson_skips_completed() {
        let mut progress = TrainingProgress::default();
        progress
            .completed_lessons
            .push("secure-coding-101".to_string());
        let next = recommend_next_lesson(&progress);
        assert!(next.is_some());
        assert_ne!(next.unwrap().id, "secure-coding-101");
    }
}
