use crate::utils::print as p;
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum CicdCommands {
    /// Generate CI/CD configuration templates for your project
    Init(InitArgs),
    /// List available CI/CD platforms and template types
    List,
    /// Validate an existing CI/CD configuration
    Validate(ValidateArgs),
}

#[derive(Args)]
pub struct InitArgs {
    /// Target CI/CD platform
    #[arg(long, value_parser = ["github", "gitlab", "jenkins", "all"], default_value = "github")]
    pub platform: String,

    /// Output directory for generated templates
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Network the contracts will deploy to
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,

    /// Notification webhook URL (Slack/Discord/Teams)
    #[arg(long)]
    pub webhook_url: Option<String>,

    /// Enable contract monitoring job
    #[arg(long, default_value = "true")]
    pub monitoring: bool,

    /// Enable automated rollback on failure
    #[arg(long, default_value = "true")]
    pub rollback: bool,

    /// Overwrite existing files without prompting
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Path to CI/CD config file to validate
    pub path: PathBuf,
}

pub fn handle(cmd: CicdCommands) -> Result<()> {
    match cmd {
        CicdCommands::Init(args) => handle_init(args),
        CicdCommands::List => handle_list(),
        CicdCommands::Validate(args) => handle_validate(args),
    }
}

fn handle_list() -> Result<()> {
    p::header("Available CI/CD Platforms & Templates");
    p::separator();

    let platforms = [
        (
            "github",
            "GitHub Actions",
            &[
                "contract-test.yml  — automated contract testing pipeline",
                "contract-monitor.yml — scheduled contract health monitoring",
                "deployment.yml     — safe deploy / rollback with quality gate",
            ] as &[&str],
        ),
        (
            "gitlab",
            "GitLab CI/CD",
            &[".gitlab-ci.yml    — full pipeline: quality, test, deploy, monitor, notify"],
        ),
        (
            "jenkins",
            "Jenkins",
            &["Jenkinsfile        — multi-stage pipeline with approval gates"],
        ),
    ];

    for (key, name, templates) in &platforms {
        println!("  {} {}", "◆".cyan(), name.bright_white().bold());
        println!("  {} platform flag: {}", "→".dimmed(), key.cyan());
        for t in *templates {
            println!("    {}", t.dimmed());
        }
        println!();
    }

    p::info("Generate templates with: starforge cicd init --platform <platform>");
    p::separator();
    Ok(())
}

fn handle_init(args: InitArgs) -> Result<()> {
    p::header("Generate CI/CD Templates");

    let platforms: Vec<&str> = if args.platform == "all" {
        vec!["github", "gitlab", "jenkins"]
    } else {
        vec![args.platform.as_str()]
    };

    let mut generated = 0usize;

    for platform in &platforms {
        match *platform {
            "github" => {
                let out = args
                    .output
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(".github/workflows"));
                fs::create_dir_all(&out)?;
                generated += write_template(
                    &out.join("contract-test.yml"),
                    &github_contract_test_template(&args.network, args.monitoring),
                    args.force,
                )?;
                generated += write_template(
                    &out.join("contract-monitor.yml"),
                    GITHUB_MONITOR_TEMPLATE,
                    args.force,
                )?;
            }
            "gitlab" => {
                let out = args.output.clone().unwrap_or_else(|| PathBuf::from("."));
                generated +=
                    write_template(&out.join(".gitlab-ci.yml"), GITLAB_CI_TEMPLATE, args.force)?;
            }
            "jenkins" => {
                let out = args.output.clone().unwrap_or_else(|| PathBuf::from("."));
                generated +=
                    write_template(&out.join("Jenkinsfile"), JENKINS_TEMPLATE, args.force)?;
            }
            _ => {}
        }
    }

    if generated == 0 {
        p::warn("No files written. Use --force to overwrite existing files.");
        return Ok(());
    }

    println!();
    p::success(&format!("{} template(s) generated.", generated));
    println!();

    // Post-generation guidance
    println!("  {}", "Next steps:".bright_white().bold());
    println!();

    if platforms.contains(&"github") || platforms.contains(&"all") {
        println!("  {}", "GitHub Actions:".cyan());
        println!(
            "  {}",
            "  Add these secrets to your repository Settings → Secrets:".dimmed()
        );
        let secrets = [
            "STARFORGE_DEPLOY_COMMAND   — shell command that deploys your contract",
            "STARFORGE_ROLLBACK_COMMAND — shell command that rolls back",
            "STARFORGE_HEALTHCHECK_URL  — URL polled after deploy to verify health",
            "SLACK_WEBHOOK_URL          — (optional) Slack webhook for notifications",
            "STARFORGE_CONTRACT_ID      — (optional) deployed contract ID for monitoring",
        ];
        for s in &secrets {
            println!("    {}", s.dimmed());
        }
        println!();
    }

    if let Some(ref webhook) = args.webhook_url {
        p::kv("Webhook configured", webhook);
        p::info("Set STARFORGE_NOTIFY_WEBHOOK in your CI secrets to the same URL.");
    }

    p::info("Run `starforge cicd validate <file>` to check a generated config.");
    p::separator();
    Ok(())
}

/// Write content to path, skipping if the file exists and force is false.
/// Returns 1 if written, 0 if skipped.
fn write_template(path: &PathBuf, content: &str, force: bool) -> Result<usize> {
    if path.exists() && !force {
        p::warn(&format!(
            "Skipping {} (already exists — use --force to overwrite)",
            path.display()
        ));
        return Ok(0);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    p::success(&format!("Written: {}", path.display()));
    Ok(1)
}

fn handle_validate(args: ValidateArgs) -> Result<()> {
    p::header("Validate CI/CD Configuration");

    if !args.path.exists() {
        anyhow::bail!("File not found: {}", args.path.display());
    }

    let content = fs::read_to_string(&args.path)?;
    let ext = args.path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let name = args.path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    p::kv("File", &args.path.display().to_string());

    let mut issues = 0usize;

    match ext {
        "yml" | "yaml" => {
            // Basic YAML parse check
            if let Err(e) = serde_yaml::from_str::<serde_json::Value>(&content) {
                p::warn(&format!("YAML parse error: {}", e));
                issues += 1;
            } else {
                p::kv_accent("YAML syntax", "✓ valid");
            }

            // Starforge-specific checks
            if content.contains("STARFORGE_DEPLOY_COMMAND") {
                p::kv_accent("Deploy command secret", "✓ referenced");
            }
            if content.contains("STARFORGE_ROLLBACK_COMMAND") {
                p::kv_accent("Rollback command secret", "✓ referenced");
            }
            if content.contains("STARFORGE_HEALTHCHECK_URL") || content.contains("healthcheck") {
                p::kv_accent("Health check", "✓ configured");
            }
            if !content.contains("notify") && !content.contains("SLACK_WEBHOOK") {
                p::warn("No notification step detected — consider adding ci-notify.sh");
                issues += 1;
            }
        }
        _ if name == "Jenkinsfile" => {
            if content.contains("pipeline") && content.contains("stages") {
                p::kv_accent("Jenkinsfile structure", "✓ valid");
            } else {
                p::warn("Jenkinsfile may be missing pipeline or stages block");
                issues += 1;
            }
        }
        _ => {
            p::info("Unknown file type — skipping deep validation.");
        }
    }

    println!();
    if issues == 0 {
        p::success("No issues found.");
    } else {
        p::warn(&format!("{} issue(s) found.", issues));
    }
    p::separator();
    Ok(())
}

// ── Template strings ──────────────────────────────────────────────────────────

fn github_contract_test_template(network: &str, monitoring: bool) -> String {
    let monitor_note = if monitoring {
        "\n      - name: Post-deploy monitoring\n        run: starforge inspect state \"$CONTRACT_ID\" --network \"$NETWORK\" --json > monitor.json || true\n        env:\n          CONTRACT_ID: ${{ secrets.STARFORGE_CONTRACT_ID }}\n          NETWORK: ${{ inputs.network }}"
    } else {
        ""
    };

    format!(
        r#"name: Contract Test Pipeline

on:
  push:
    paths: ['src/**', 'tests/**', 'Cargo.toml', 'Cargo.lock']
  pull_request:
    paths: ['src/**', 'tests/**', 'Cargo.toml', 'Cargo.lock']
  workflow_dispatch:
    inputs:
      network:
        description: Target network
        required: false
        default: {network}
        type: choice
        options: [testnet, mainnet]
      wasm_path:
        description: Path to WASM file (optional)
        required: false
        type: string

permissions:
  contents: read

jobs:
  unit-tests:
    name: Unit Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install system dependencies
        run: sudo apt-get update && sudo apt-get install -y libudev-dev
      - name: Build starforge
        run: cargo build --locked --release && cp target/release/starforge /usr/local/bin/starforge
      - name: Run tests
        run: cargo test --locked -- --test-threads=1
      - name: Contract lint
        if: ${{{{ inputs.wasm_path != '' }}}}
        run: starforge lint --wasm "${{{{ inputs.wasm_path }}}}" || true
      - name: Security audit
        if: ${{{{ inputs.wasm_path != '' }}}}
        run: starforge audit --wasm "${{{{ inputs.wasm_path }}}}" --output json > audit.json || true{monitor_note}

  notify:
    name: Notify Results
    runs-on: ubuntu-latest
    needs: [unit-tests]
    if: always()
    steps:
      - name: Send notification
        if: ${{{{ secrets.SLACK_WEBHOOK_URL != '' }}}}
        uses: slackapi/slack-github-action@v1
        with:
          payload: |
            {{
              "text": "${{{{ contains(needs.*.result, 'failure') && '❌' || '✅' }}}} Contract tests ${{{{ contains(needs.*.result, 'failure') && 'failed' || 'passed' }}}}: ${{{{ github.repository }}}} @ ${{{{ github.ref_name }}}}"
            }}
        env:
          SLACK_WEBHOOK_URL: ${{{{ secrets.SLACK_WEBHOOK_URL }}}}
          SLACK_WEBHOOK_TYPE: INCOMING_WEBHOOK
"#,
        network = network,
        monitor_note = monitor_note,
    )
}

const GITHUB_MONITOR_TEMPLATE: &str = r#"name: Contract Monitoring

on:
  schedule:
    - cron: '*/30 * * * *'
  workflow_dispatch:
    inputs:
      contract_id:
        description: Contract ID to monitor
        required: true
        type: string
      network:
        description: Network
        required: false
        default: testnet
        type: choice
        options: [testnet, mainnet]

permissions:
  contents: read

jobs:
  monitor:
    name: Monitor Contract Health
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install system dependencies
        run: sudo apt-get update && sudo apt-get install -y libudev-dev
      - name: Build starforge
        run: cargo build --locked --release && cp target/release/starforge /usr/local/bin/starforge
      - name: Inspect contract state
        run: |
          CONTRACT_ID="${{ inputs.contract_id || secrets.STARFORGE_CONTRACT_ID }}"
          if [ -n "$CONTRACT_ID" ]; then
            starforge inspect state "$CONTRACT_ID" \
              --network "${{ inputs.network || 'testnet' }}" \
              --json > monitor-snapshot.json
          fi
        continue-on-error: true
      - name: Upload snapshot
        uses: actions/upload-artifact@v4
        with:
          name: monitor-snapshot-${{ github.run_number }}
          path: monitor-snapshot.json
          if-no-files-found: ignore
          retention-days: 30
      - name: Alert on failure
        if: ${{ failure() && secrets.SLACK_WEBHOOK_URL != '' }}
        uses: slackapi/slack-github-action@v1
        with:
          payload: '{"text":"⚠️ Contract monitoring alert for ${{ inputs.contract_id || secrets.STARFORGE_CONTRACT_ID }}"}'
        env:
          SLACK_WEBHOOK_URL: ${{ secrets.SLACK_WEBHOOK_URL }}
          SLACK_WEBHOOK_TYPE: INCOMING_WEBHOOK
"#;

const GITLAB_CI_TEMPLATE: &str = r#"stages: [quality, test, deploy, monitor, notify]

default:
  image: rust:latest

quality:
  stage: quality
  before_script:
    - apt-get update -y && apt-get install -y libudev-dev
    - rustup component add rustfmt clippy
  script:
    - cargo fmt --all --check
    - cargo build --locked
    - cargo test --locked
    - cargo clippy --all-features --locked -- -D warnings

contract_test:
  stage: test
  needs: [quality]
  before_script:
    - apt-get update -y && apt-get install -y libudev-dev
    - cargo build --locked --release
    - cp target/release/starforge /usr/local/bin/starforge
  script:
    - cargo test --locked -- --test-threads=1
    - '[ -n "${STARFORGE_WASM_PATH:-}" ] && starforge lint --wasm "$STARFORGE_WASM_PATH" || true'
    - '[ -n "${STARFORGE_WASM_PATH:-}" ] && starforge audit --wasm "$STARFORGE_WASM_PATH" --output json > audit.json || true'
  artifacts:
    paths: [audit.json]
    expire_in: 30 days
    when: always

deploy_staging:
  stage: deploy
  needs: [contract_test]
  environment: staging
  resource_group: deployment-staging
  when: manual
  script:
    - STARFORGE_DEPLOY_ENVIRONMENT=staging bash scripts/ci-deploy.sh

deploy_production:
  stage: deploy
  needs: [contract_test]
  environment: production
  resource_group: deployment-production
  when: manual
  rules:
    - if: '$CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH'
  script:
    - STARFORGE_DEPLOY_ENVIRONMENT=production STARFORGE_DEPLOY_APPROVED=true bash scripts/ci-deploy.sh

rollback_production:
  stage: deploy
  needs: [contract_test]
  environment: production
  resource_group: deployment-production
  when: manual
  rules:
    - if: '$CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH'
  script:
    - STARFORGE_DEPLOY_ENVIRONMENT=production STARFORGE_ROLLBACK_APPROVED=true bash scripts/ci-rollback.sh

monitor_contract:
  stage: monitor
  needs: [deploy_staging]
  when: on_success
  script:
    - '[ -n "${STARFORGE_CONTRACT_ID:-}" ] && starforge inspect state "$STARFORGE_CONTRACT_ID" --network testnet --json > monitor.json || true'
  artifacts:
    paths: [monitor.json]
    expire_in: 90 days
    when: always
  allow_failure: true

notify_failure:
  stage: notify
  when: on_failure
  script:
    - STARFORGE_NOTIFY_STATUS=failure STARFORGE_NOTIFY_ENV="$CI_ENVIRONMENT_NAME" STARFORGE_NOTIFY_URL="$CI_PIPELINE_URL" bash scripts/ci-notify.sh || true
"#;

const JENKINS_TEMPLATE: &str = r#"pipeline {
  agent any
  options { disableConcurrentBuilds(); timestamps() }

  parameters {
    choice(name: 'ACTION', choices: ['verify', 'deploy', 'rollback'], description: 'Action')
    choice(name: 'TARGET_ENVIRONMENT', choices: ['staging', 'production'], description: 'Target')
    string(name: 'WASM_PATH', defaultValue: '', description: 'Optional WASM path')
    string(name: 'CONTRACT_ID', defaultValue: '', description: 'Optional contract ID')
  }

  stages {
    stage('Quality gate') {
      steps {
        sh '''#!/usr/bin/env bash
          set -euo pipefail
          rustup component add rustfmt clippy
          cargo fmt --all --check
          cargo build --locked
          cargo test --locked
          cargo clippy --all-features --locked -- -D warnings
        '''
      }
    }

    stage('Contract tests') {
      steps {
        sh 'cargo test --locked -- --test-threads=1'
      }
    }

    stage('Deploy or rollback') {
      when { expression { params.ACTION != 'verify' } }
      steps {
        script {
          if (params.TARGET_ENVIRONMENT == 'production') {
            input message: "Approve production ${params.ACTION}?", ok: 'Approve'
          }
        }
        withCredentials([
          string(credentialsId: 'starforge-deploy-command',   variable: 'STARFORGE_DEPLOY_COMMAND'),
          string(credentialsId: 'starforge-rollback-command',  variable: 'STARFORGE_ROLLBACK_COMMAND'),
          string(credentialsId: 'starforge-healthcheck-url',   variable: 'STARFORGE_HEALTHCHECK_URL'),
          string(credentialsId: 'starforge-notify-webhook',    variable: 'STARFORGE_NOTIFY_WEBHOOK')
        ]) {
          sh '''#!/usr/bin/env bash
            export STARFORGE_DEPLOY_ENVIRONMENT="$TARGET_ENVIRONMENT"
            export STARFORGE_NOTIFY_ACTOR="${BUILD_USER:-Jenkins}"
            export STARFORGE_NOTIFY_URL="$BUILD_URL"
            [ "$TARGET_ENVIRONMENT" = production ] && export STARFORGE_DEPLOY_APPROVED=true STARFORGE_ROLLBACK_APPROVED=true
            [ "$ACTION" = deploy ] && bash scripts/ci-deploy.sh || bash scripts/ci-rollback.sh
          '''
        }
      }
    }
  }

  post {
    always {
      withCredentials([string(credentialsId: 'starforge-notify-webhook', variable: 'STARFORGE_NOTIFY_WEBHOOK')]) {
        sh '''#!/usr/bin/env bash
          STARFORGE_NOTIFY_STATUS="${currentBuild.currentResult == 'SUCCESS' ? 'success' : 'failure'}" \
          STARFORGE_NOTIFY_ENV="$TARGET_ENVIRONMENT" \
          STARFORGE_NOTIFY_URL="$BUILD_URL" \
          bash scripts/ci-notify.sh || true
        '''
      }
    }
  }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_handle_list() {
        assert!(handle_list().is_ok());
    }

    #[test]
    fn test_handle_init_github() {
        let temp_dir = TempDir::new().unwrap();
        let out_dir = temp_dir.path().join("github");

        let args = InitArgs {
            platform: "github".into(),
            output: Some(out_dir.clone()),
            network: "testnet".into(),
            webhook_url: None,
            monitoring: true,
            rollback: true,
            force: true,
        };

        assert!(handle_init(args).is_ok());
        assert!(out_dir.join("contract-test.yml").exists());
        assert!(out_dir.join("contract-monitor.yml").exists());
    }

    #[test]
    fn test_handle_init_gitlab() {
        let temp_dir = TempDir::new().unwrap();
        let out_dir = temp_dir.path().join("gitlab");

        let args = InitArgs {
            platform: "gitlab".into(),
            output: Some(out_dir.clone()),
            network: "testnet".into(),
            webhook_url: Some("https://hooks.slack.com/test".into()),
            monitoring: true,
            rollback: true,
            force: true,
        };

        assert!(handle_init(args).is_ok());
        assert!(out_dir.join(".gitlab-ci.yml").exists());
    }

    #[test]
    fn test_handle_init_jenkins() {
        let temp_dir = TempDir::new().unwrap();
        let out_dir = temp_dir.path().join("jenkins");

        let args = InitArgs {
            platform: "jenkins".into(),
            output: Some(out_dir.clone()),
            network: "mainnet".into(),
            webhook_url: None,
            monitoring: true,
            rollback: true,
            force: true,
        };

        assert!(handle_init(args).is_ok());
        assert!(out_dir.join("Jenkinsfile").exists());
    }

    #[test]
    fn test_handle_validate_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test-workflow.yml");

        let valid_yaml = r#"
name: Test
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo "STARFORGE_DEPLOY_COMMAND"
      - run: echo "STARFORGE_ROLLBACK_COMMAND"
      - run: echo "healthcheck"
      - run: echo "notify"
"#;
        fs::write(&file_path, valid_yaml).unwrap();

        let args = ValidateArgs { path: file_path };
        assert!(handle_validate(args).is_ok());
    }

    #[test]
    fn test_handle_validate_jenkinsfile() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Jenkinsfile");

        let jenkins_content = "pipeline { stages { stage('Test') {} } }";
        fs::write(&file_path, jenkins_content).unwrap();

        let args = ValidateArgs { path: file_path };
        assert!(handle_validate(args).is_ok());
    }
}
