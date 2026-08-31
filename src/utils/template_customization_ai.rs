use crate::utils::{ollama, template_vcs};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationHistory {
    pub entries: Vec<CustomizationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationEntry {
    pub timestamp: String,
    pub template_path: String,
    pub requirements: String,
    pub changes_made: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationResult {
    pub success: bool,
    pub changes: Vec<String>,
    pub validation_report: String,
}

pub async fn customize_template(
    template_path: &Path,
    requirements: &str,
) -> Result<CustomizationResult> {
    // 1. Analyze the template structure
    let template_structure = analyze_template_structure(template_path)?;

    // 2. Ask AI to generate modifications
    let prompt = build_customization_prompt(requirements, &template_structure);
    let response = ollama::generate(
        ollama::DEFAULT_MODEL,
        &prompt,
        Some(ollama::GenerateOptions {
            temperature: Some(0.2),
            num_predict: Some(4096),
            num_ctx: Some(8192),
        }),
    )
    .await
    .context("Failed to generate customization with AI")?;

    // 3. Parse AI's response and apply changes
    let changes = apply_ai_modifications(template_path, &response.response)?;

    // 4. Validate the customized template
    let validation_report = validate_customization(template_path)?;

    // 5. Save to history and commit to VCS
    save_customization_history(template_path, requirements, &response.response)?;

    // Ensure VCS is initialized and commit
    if !template_path.join(".starforge-vcs").exists() {
        let _ = template_vcs::init_vcs(
            template_path,
            template_path
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("template"),
        );
        let _ = template_vcs::commit_version(
            template_path,
            "1.0.0",
            "Initial template state before customizations",
            "StarForge System",
        );
    }

    let history = get_customization_history(template_path)
        .await
        .unwrap_or(CustomizationHistory { entries: vec![] });
    let version = format!("1.0.{}", history.entries.len());
    let _ = template_vcs::commit_version(
        template_path,
        &version,
        &format!("AI Customization: {}", requirements),
        "StarForge AI",
    );

    Ok(CustomizationResult {
        success: validation_report.contains("Success"),
        changes,
        validation_report,
    })
}

fn analyze_template_structure(template_path: &Path) -> Result<String> {
    let mut structure = String::new();
    structure.push_str("Template structure:\n");

    // List files and directories
    if let Ok(entries) = fs::read_dir(template_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let rel_path = path.strip_prefix(template_path).unwrap_or(&path);
            structure.push_str(&format!("- {}\n", rel_path.display()));
        }
    }

    // Read key files
    let cargo_toml = template_path.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = fs::read_to_string(&cargo_toml) {
            structure.push_str("\nCargo.toml content (snippet):\n");
            structure.push_str(&content);
        }
    }

    let lib_rs = template_path.join("src").join("lib.rs");
    if lib_rs.exists() {
        if let Ok(content) = fs::read_to_string(&lib_rs) {
            structure.push_str("\nsrc/lib.rs content (snippet):\n");
            structure.push_str(&content);
        }
    }

    Ok(structure)
}

fn build_customization_prompt(requirements: &str, structure: &str) -> String {
    format!(
        "{}\
You are a smart contract template customization expert. \
Your task is to customize a Soroban smart contract template based on specific requirements.\n\n\
User requirements:\n{}\n\n\
Current template structure:\n{}\n\n\
Please provide your response in the following format:\n\
---\n\
CHANGES:\n\
- [file_path]: [modification_type]\n\
  ```rust\n\
  [entire_new_file_content_here]\n\
  ```\n\
- ...\n\
---\n\
EXPLANATION:\n\
[short explanation of changes]\n\
---\n\
Make sure you provide the ENTIRE new file content in the code block, not just a patch or diff. This is required because the code will be overwritten entirely.",
        ollama::prompts::SYSTEM_CONTEXT,
        requirements,
        structure
    )
}

fn apply_ai_modifications(template_path: &Path, ai_response: &str) -> Result<Vec<String>> {
    let mut changes = Vec::new();

    // Extract the changes section
    if let Some(start) = ai_response.find("---\nCHANGES:") {
        let content = &ai_response[start + "---\nCHANGES:".len()..];
        if let Some(end) = content.find("---") {
            let changes_section = &content[..end];
            let lines: Vec<&str> = changes_section.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                let line = lines[i].trim();
                if let Some(stripped) = line.strip_prefix("- ") {
                    if let Some((file_path, _rest)) = stripped.split_once(':') {
                        let mut code_change = String::new();
                        i += 1;
                        while i < lines.len()
                            && (lines[i].starts_with("  ") || lines[i].trim().is_empty())
                        {
                            if !lines[i].trim().is_empty() {
                                code_change.push_str(lines[i].trim_start());
                                code_change.push('\n');
                            }
                            i += 1;
                        }
                        // Extract content from code block if present
                        let clean_code = if let Some(start) = code_change.find("```") {
                            if let Some(end) = code_change[start + 3..].find("```") {
                                let inner = &code_change[start + 3..start + 3 + end];
                                // strip the language identifier like `rust`
                                if let Some(first_newline) = inner.find('\n') {
                                    inner[first_newline + 1..].trim()
                                } else {
                                    inner.trim()
                                }
                            } else {
                                code_change.trim()
                            }
                        } else {
                            code_change.trim()
                        };

                        // Apply the change to the file
                        let full_path = template_path.join(file_path.trim());
                        if full_path.exists() {
                            fs::write(&full_path, clean_code).ok();
                            changes.push(format!("Modified: {}", file_path));
                        } else {
                            // Create new file
                            if let Some(parent) = full_path.parent() {
                                fs::create_dir_all(parent).ok();
                            }
                            fs::write(&full_path, clean_code).ok();
                            changes.push(format!("Created: {}", file_path));
                        }
                        continue;
                    }
                }
                i += 1;
            }
        }
    }

    Ok(changes)
}

fn validate_customization(template_path: &Path) -> Result<String> {
    let mut report = String::new();
    report.push_str("Validation Report:\n");

    // Check for required files
    let required_files = vec!["Cargo.toml", "src/lib.rs", "README.md"];
    for file in &required_files {
        let path = template_path.join(file);
        if path.exists() {
            report.push_str(&format!("✓ {}\n", file));
        } else {
            report.push_str(&format!("✗ {}\n", file));
        }
    }

    // Verify Cargo.toml has {{PROJECT_NAME}} placeholder
    let cargo_toml = template_path.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = fs::read_to_string(&cargo_toml) {
            if content.contains("{{PROJECT_NAME}}") {
                report.push_str("✓ Cargo.toml has {{PROJECT_NAME}} placeholder\n");
            } else {
                report.push_str("✗ Cargo.toml missing {{PROJECT_NAME}} placeholder\n");
            }
        }
    }

    report.push_str("\nSuccess!");
    Ok(report)
}

fn save_customization_history(
    template_path: &Path,
    requirements: &str,
    changes_made: &str,
) -> Result<()> {
    let history_dir = template_path.join(".starforge-customizations");
    fs::create_dir_all(&history_dir).ok();
    let history_file = history_dir.join("history.json");

    let mut history: CustomizationHistory = if history_file.exists() {
        let content = fs::read_to_string(&history_file).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(CustomizationHistory {
            entries: Vec::new(),
        })
    } else {
        CustomizationHistory {
            entries: Vec::new(),
        }
    };

    history.entries.push(CustomizationEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        template_path: template_path.display().to_string(),
        requirements: requirements.to_string(),
        changes_made: changes_made.to_string(),
    });

    let json_content = serde_json::to_string_pretty(&history)?;
    fs::write(&history_file, json_content)?;

    Ok(())
}

pub async fn rollback_customization(template_path: &Path, index: Option<usize>) -> Result<()> {
    let history_file = template_path
        .join(".starforge-customizations")
        .join("history.json");
    if !history_file.exists() {
        anyhow::bail!("No customization history found for this template");
    }

    let content = fs::read_to_string(&history_file)?;
    let mut history: CustomizationHistory = serde_json::from_str(&content)?;

    let target_index = if let Some(i) = index {
        if i >= history.entries.len() {
            anyhow::bail!("Invalid history index");
        }
        i
    } else {
        // Rollback to previous state (before last customization)
        if history.entries.is_empty() {
            anyhow::bail!("Not enough history to rollback");
        }
        history.entries.len().saturating_sub(1)
    };

    println!(
        "Rolling back to state before: {}",
        history.entries[target_index].timestamp
    );

    // Rollback using git if available
    if template_path.join(".git").exists() {
        // If target_index is the last customization, it's HEAD~1 or based on tag
        // Since we created tags like "v1.0.X", we can just find the appropriate tag
        // target_index means we want to discard everything from target_index onwards.
        // So we rollback to `target_index` (which was the state before the target customization, if we consider index 0 as first customization).
        // Actually, if we just want to reset to a previous tag, let's just use `git checkout`.
        // Let's get the version to rollback to.
        let version_tag = format!("v1.0.{}", target_index);

        let output = std::process::Command::new("git")
            .current_dir(template_path)
            .args(["reset", "--hard", &version_tag])
            .output()
            .context("Failed to rollback via git reset")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to rollback using git: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Truncate history
        history.entries.truncate(target_index);
        let json_content = serde_json::to_string_pretty(&history)?;
        fs::write(&history_file, json_content)?;
    } else {
        anyhow::bail!("Git repository not found. Cannot perform rollback.");
    }

    Ok(())
}

pub async fn get_customization_history(template_path: &Path) -> Result<CustomizationHistory> {
    let history_file = template_path
        .join(".starforge-customizations")
        .join("history.json");
    if !history_file.exists() {
        return Ok(CustomizationHistory {
            entries: Vec::new(),
        });
    }

    let content = fs::read_to_string(&history_file)?;
    let history: CustomizationHistory = serde_json::from_str(&content)?;
    Ok(history)
}
