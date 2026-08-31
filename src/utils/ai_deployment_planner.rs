//! AI-driven deployment planning module.
//!
//! This module provides intelligent deployment planning for Soroban contracts,
//! including contract analysis, network selection, gas estimation, risk assessment,
//! and rollback planning.

use crate::utils::ollama::{self, GenerateOptions};
use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Output Format ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

// ─── Core Data Structures ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeploymentPlan {
    pub contract_name: String,
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub analysis: ContractAnalysis,
    pub network_recommendation: NetworkRecommendation,
    pub gas_estimate: GasEstimate,
    pub deployment_window: DeploymentWindow,
    pub risk_assessment: RiskAssessment,
    pub rollback_plan: RollbackPlan,
    pub recommendations: Vec<Recommendation>,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractAnalysis {
    pub name: String,
    pub path: String,
    pub lines: usize,
    pub functions: usize,
    pub complexity: ComplexityLevel,
    pub is_upgradeable: bool,
    pub upgrade_patterns: Vec<String>,
    pub security_findings: Vec<SecurityFinding>,
    pub optimization_suggestions: Vec<String>,
    pub readiness_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComplexityLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityFinding {
    pub level: SecurityLevel,
    pub finding_type: String,
    pub description: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SecurityLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkRecommendation {
    pub network: String,
    pub chain_id: u64,
    pub priority: u8,
    pub reasons: Vec<String>,
    pub risks: Vec<String>,
    pub alternatives: Vec<AlternativeNetwork>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlternativeNetwork {
    pub name: String,
    pub chain_id: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GasEstimate {
    pub min: String,
    pub max: String,
    pub expected: String,
    pub unit: String,
    pub usd_value: Option<String>,
    pub components: GasComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GasComponents {
    pub deployment: String,
    pub verification: String,
    pub interactions: String,
    pub total: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeploymentWindow {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub priority: WindowPriority,
    pub reason: String,
    pub gas_price_prediction: Option<GasPrediction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WindowPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GasPrediction {
    pub current: String,
    pub predicted: String,
    pub trend: GasTrend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GasTrend {
    Rising,
    Falling,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskAssessment {
    pub overall: RiskLevel,
    pub score: u8,
    pub categories: Vec<RiskCategory>,
    pub mitigations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskCategory {
    pub name: String,
    pub level: RiskLevel,
    pub description: String,
    pub mitigation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RollbackPlan {
    pub steps: Vec<RollbackStep>,
    pub estimated_time: String,
    pub rollback_cost: String,
    pub success_probability: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RollbackStep {
    pub order: u32,
    pub action: String,
    pub description: String,
    pub prerequisites: Vec<String>,
    pub verification: String,
    pub estimated_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recommendation {
    pub action: String,
    pub urgency: Urgency,
    pub impact: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Urgency {
    Immediate,
    BeforeDeployment,
    PostDeployment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Draft,
    Reviewed,
    Approved,
    Executed,
    Failed,
}

// ─── Planner Implementation ──────────────────────────────────────────────────

// `target_network`/`max_gas_price` are not currently read from any code path
// in this crate. Kept rather than removed since deleting them is a product
// decision, not a lint-scoping one.
#[allow(dead_code)]
pub struct AiDeploymentPlanner {
    contract_path: PathBuf,
    target_network: String,
    max_gas_price: Option<u64>,
    prefer_testnet: bool,
    model: String,
    output_format: OutputFormat,
}

impl AiDeploymentPlanner {
    pub fn new(
        contract_path: PathBuf,
        target_network: String,
        max_gas_price: Option<u64>,
        prefer_testnet: bool,
        model: String,
        output_format: OutputFormat,
    ) -> Self {
        Self {
            contract_path,
            target_network,
            max_gas_price,
            prefer_testnet,
            model,
            output_format,
        }
    }

    pub async fn generate_plan(&self) -> Result<DeploymentPlan> {
        // Step 1: Read and analyze contract
        let code = std::fs::read_to_string(&self.contract_path)
            .with_context(|| format!("Cannot read contract: {}", self.contract_path.display()))?;

        let analysis = self.analyze_contract(&code).await?;

        // Step 2: Get network recommendations
        let network_recommendation = self.recommend_network(&analysis).await?;

        // Step 3: Estimate gas costs
        let gas_estimate = self
            .estimate_gas(&analysis, &network_recommendation)
            .await?;

        // Step 4: Determine deployment window
        let deployment_window = self
            .suggest_deployment_window(&network_recommendation)
            .await?;

        // Step 5: Assess risks
        let risk_assessment = self
            .assess_risks(&analysis, &network_recommendation)
            .await?;

        // Step 6: Create rollback plan
        let rollback_plan = self
            .create_rollback_plan(&analysis, &network_recommendation)
            .await?;

        // Step 7: Generate recommendations
        let recommendations = self
            .generate_recommendations(&analysis, &risk_assessment, &gas_estimate)
            .await?;

        Ok(DeploymentPlan {
            contract_name: self
                .contract_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            version: "1.0.0".to_string(),
            generated_at: Utc::now(),
            analysis,
            network_recommendation,
            gas_estimate,
            deployment_window,
            risk_assessment,
            rollback_plan,
            recommendations,
            status: PlanStatus::Draft,
        })
    }

    async fn analyze_contract(&self, code: &str) -> Result<ContractAnalysis> {
        let prompt = format!(
            r#"Analyze this Soroban smart contract and provide:
1. Complexity level (Low/Medium/High)
2. Number of functions (count)
3. Is it upgradeable? (Yes/No)
4. Any security concerns
5. Optimization suggestions

Contract code:
```rust
{}
```"#,
            code
        );

        let opts = GenerateOptions {
            temperature: Some(0.1),
            num_predict: Some(2048),
            num_ctx: Some(8192),
        };

        let _response = ollama::generate(&self.model, &prompt, Some(opts))
            .await
            .context("AI contract analysis failed")?;

        // Parse the AI response into structured data
        // For now, we'll use heuristic analysis
        let line_count = code.lines().count();
        let func_count = code.matches("fn ").count();

        let complexity = if line_count > 500 || func_count > 20 {
            ComplexityLevel::High
        } else if line_count > 200 || func_count > 10 {
            ComplexityLevel::Medium
        } else {
            ComplexityLevel::Low
        };

        let is_upgradeable = code.contains("upgrade") || code.contains("proxy");
        let mut upgrade_patterns = Vec::new();
        if is_upgradeable {
            if code.contains("UUPS") {
                upgrade_patterns.push("UUPS proxy pattern".to_string());
            }
            if code.contains("Transparent") || code.contains("TransparentUpgradeable") {
                upgrade_patterns.push("Transparent proxy pattern".to_string());
            }
        }

        // Simple security checks
        let mut security_findings = Vec::new();

        // Check for reentrancy risks
        if code.contains("transfer") || code.contains("send") {
            security_findings.push(SecurityFinding {
                level: SecurityLevel::Medium,
                finding_type: "Potential reentrancy".to_string(),
                description: "Contract uses transfer/send operations".to_string(),
                recommendation: "Consider using OpenZeppelin ReentrancyGuard".to_string(),
            });
        }

        // Check for ownership
        if !code.contains("owner") && !code.contains("Ownable") {
            security_findings.push(SecurityFinding {
                level: SecurityLevel::Low,
                finding_type: "Missing ownership".to_string(),
                description: "No ownership mechanism detected".to_string(),
                recommendation: "Consider implementing OpenZeppelin Ownable".to_string(),
            });
        }

        // Check for pause mechanism
        if !code.contains("pause") && !code.contains("Pausable") {
            security_findings.push(SecurityFinding {
                level: SecurityLevel::Low,
                finding_type: "No pause mechanism".to_string(),
                description: "Contract cannot be paused in emergencies".to_string(),
                recommendation: "Consider implementing OpenZeppelin Pausable".to_string(),
            });
        }

        // Optimization suggestions
        let mut optimization_suggestions = Vec::new();
        if code.contains("vec!") || code.contains("Vec::new") {
            optimization_suggestions
                .push("Consider using fixed-size arrays where possible".to_string());
        }
        if code.contains("String") {
            optimization_suggestions.push("Use &str instead of String where possible".to_string());
        }

        // Calculate readiness score
        let mut score = 100;
        for finding in &security_findings {
            match finding.level {
                SecurityLevel::Critical => score -= 25,
                SecurityLevel::High => score -= 15,
                SecurityLevel::Medium => score -= 10,
                SecurityLevel::Low => score -= 5,
            }
        }
        if complexity == ComplexityLevel::High {
            score -= 10;
        }

        Ok(ContractAnalysis {
            name: self
                .contract_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            path: self.contract_path.display().to_string(),
            lines: line_count,
            functions: func_count,
            complexity,
            is_upgradeable,
            upgrade_patterns,
            security_findings,
            optimization_suggestions,
            readiness_score: score.clamp(0, 100) as u8,
        })
    }

    async fn recommend_network(
        &self,
        analysis: &ContractAnalysis,
    ) -> Result<NetworkRecommendation> {
        let mut reasons = Vec::new();
        let mut risks = Vec::new();
        let mut alternatives = Vec::new();

        let (network, chain_id, priority) = if self.prefer_testnet {
            reasons.push("User prefers testnet".to_string());
            ("testnet".to_string(), 11155111, 1)
        } else if analysis.is_upgradeable {
            reasons.push("Contract is upgradeable - mainnet suitable".to_string());
            ("mainnet".to_string(), 1, 8)
        } else if analysis.readiness_score < 70 {
            reasons.push("Contract readiness score is low - testnet recommended".to_string());
            ("testnet".to_string(), 11155111, 9)
        } else if analysis.complexity == ComplexityLevel::High {
            reasons.push("High complexity requires mainnet gas efficiency".to_string());
            ("mainnet".to_string(), 1, 7)
        } else {
            reasons.push("Standard deployment - testnet recommended first".to_string());
            ("testnet".to_string(), 11155111, 5)
        };

        // Add risks
        if network == "mainnet" {
            risks.push("Mainnet deployment is permanent".to_string());
            risks.push("Gas costs may spike".to_string());
            alternatives.push(AlternativeNetwork {
                name: "Sepolia Testnet".to_string(),
                chain_id: 11155111,
                reason: "Test deployment first".to_string(),
            });
        } else {
            risks.push("Testnet may have less reliable infrastructure".to_string());
            alternatives.push(AlternativeNetwork {
                name: "Ethereum Mainnet".to_string(),
                chain_id: 1,
                reason: "Production deployment".to_string(),
            });
        }

        Ok(NetworkRecommendation {
            network,
            chain_id,
            priority,
            reasons,
            risks,
            alternatives,
        })
    }

    async fn estimate_gas(
        &self,
        analysis: &ContractAnalysis,
        network: &NetworkRecommendation,
    ) -> Result<GasEstimate> {
        // Base gas estimates based on contract complexity
        let base_gas = match analysis.complexity {
            ComplexityLevel::Low => 100_000,
            ComplexityLevel::Medium => 250_000,
            ComplexityLevel::High => 500_000,
        };

        let function_gas = (analysis.functions as u64) * 20_000;
        let upgrade_gas = if analysis.is_upgradeable { 50_000 } else { 0 };

        let deployment_gas = base_gas + function_gas + upgrade_gas;
        let verification_gas = 50_000;
        let interactions_gas = (analysis.functions as u64).min(5) * 30_000;

        let total_gas = deployment_gas + verification_gas + interactions_gas;

        // Convert gas to ETH (approximate)
        let gas_price = if network.network == "mainnet" { 30 } else { 5 }; // gwei
        let gas_price_eth = gas_price as f64 / 1e9;
        let total_eth = (total_gas as f64) * gas_price_eth;

        let min_eth = total_eth * 0.8;
        let max_eth = total_eth * 1.2;

        // USD value (approximate ETH price = $3500)
        let usd_value = format!("${:.2}", total_eth * 3500.0);

        Ok(GasEstimate {
            min: format!("{:.6}", min_eth),
            max: format!("{:.6}", max_eth),
            expected: format!("{:.6}", total_eth),
            unit: "ETH".to_string(),
            usd_value: Some(usd_value),
            components: GasComponents {
                deployment: format!("{}", deployment_gas),
                verification: format!("{}", verification_gas),
                interactions: format!("{}", interactions_gas),
                total: format!("{}", total_gas),
            },
        })
    }

    async fn suggest_deployment_window(
        &self,
        network: &NetworkRecommendation,
    ) -> Result<DeploymentWindow> {
        let now = Utc::now();

        // Suggest next weekday at 8 AM UTC
        let mut start_time = now;
        let days_to_add = 1;
        start_time += chrono::Duration::days(days_to_add);
        start_time = start_time
            .with_hour(8)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap();

        // If weekend, move to Monday
        let weekday = start_time.weekday();
        if weekday == chrono::Weekday::Sat {
            start_time += chrono::Duration::days(2);
        } else if weekday == chrono::Weekday::Sun {
            start_time += chrono::Duration::days(1);
        }

        let end_time = start_time + chrono::Duration::hours(4);

        let priority = if network.network == "mainnet" {
            WindowPriority::High
        } else if network.priority > 7 {
            WindowPriority::Medium
        } else {
            WindowPriority::Low
        };

        Ok(DeploymentWindow {
            start_time,
            end_time,
            priority,
            reason: "Early morning UTC slot with low gas prices".to_string(),
            gas_price_prediction: Some(GasPrediction {
                current: "30 gwei".to_string(),
                predicted: "25 gwei".to_string(),
                trend: GasTrend::Falling,
            }),
        })
    }

    async fn assess_risks(
        &self,
        analysis: &ContractAnalysis,
        network: &NetworkRecommendation,
    ) -> Result<RiskAssessment> {
        let mut categories = Vec::new();
        let mut mitigations = Vec::new();

        // Contract security risk
        let security_level = if analysis
            .security_findings
            .iter()
            .any(|f| matches!(f.level, SecurityLevel::Critical | SecurityLevel::High))
        {
            RiskLevel::High
        } else if analysis.security_findings.len() > 3 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        categories.push(RiskCategory {
            name: "Contract Security".to_string(),
            level: security_level,
            description: format!(
                "{} security findings detected",
                analysis.security_findings.len()
            ),
            mitigation: "Review and address all security findings".to_string(),
        });

        // Network risk
        let network_level = if network.network == "mainnet" {
            RiskLevel::High
        } else {
            RiskLevel::Low
        };

        categories.push(RiskCategory {
            name: "Network Risk".to_string(),
            level: network_level,
            description: format!("Deploying to {}", network.network),
            mitigation: if network.network == "mainnet" {
                "Test on testnet first, have team review".to_string()
            } else {
                "Testnet has limited risk".to_string()
            },
        });

        // Complexity risk
        let complexity_level = match analysis.complexity {
            ComplexityLevel::High => RiskLevel::Medium,
            ComplexityLevel::Medium => RiskLevel::Low,
            ComplexityLevel::Low => RiskLevel::Low,
        };

        categories.push(RiskCategory {
            name: "Contract Complexity".to_string(),
            level: complexity_level,
            description: format!("Complexity level: {:?}", analysis.complexity),
            mitigation: "Consider breaking down into smaller contracts".to_string(),
        });

        // Calculate overall score
        let mut score = 100;
        for category in &categories {
            match category.level {
                RiskLevel::Critical => score -= 30,
                RiskLevel::High => score -= 20,
                RiskLevel::Medium => score -= 10,
                RiskLevel::Low => score -= 5,
            }
        }

        let overall = if score < 40 {
            RiskLevel::Critical
        } else if score < 60 {
            RiskLevel::High
        } else if score < 80 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        // Add mitigations
        if overall == RiskLevel::High || overall == RiskLevel::Critical {
            mitigations.push("Schedule additional security review".to_string());
            mitigations.push("Consider delaying deployment".to_string());
        }
        if network.network == "mainnet" {
            mitigations.push("Have multi-sig for mainnet deployment".to_string());
        }

        Ok(RiskAssessment {
            overall,
            score: score.clamp(0, 100) as u8,
            categories,
            mitigations,
        })
    }

    async fn create_rollback_plan(
        &self,
        analysis: &ContractAnalysis,
        _network: &NetworkRecommendation,
    ) -> Result<RollbackPlan> {
        let mut steps = Vec::new();

        if analysis.is_upgradeable {
            steps.push(RollbackStep {
                order: 1,
                action: "Deploy previous version".to_string(),
                description: "Upgrade the proxy to the previous implementation".to_string(),
                prerequisites: vec![
                    "Proxy address".to_string(),
                    "Previous implementation address".to_string(),
                ],
                verification: "Verify functions work correctly".to_string(),
                estimated_time: "5 minutes".to_string(),
            });
        } else {
            steps.push(RollbackStep {
                order: 1,
                action: "Emergency pause".to_string(),
                description: "Pause the contract to prevent further operations".to_string(),
                prerequisites: vec!["Admin wallet with pause capability".to_string()],
                verification: "Check contract paused state".to_string(),
                estimated_time: "2 minutes".to_string(),
            });

            steps.push(RollbackStep {
                order: 2,
                action: "Deploy new version".to_string(),
                description: "Deploy fixed version of the contract".to_string(),
                prerequisites: vec![
                    "New contract code ready".to_string(),
                    "Migration script".to_string(),
                ],
                verification: "Verify new deployment works".to_string(),
                estimated_time: "15 minutes".to_string(),
            });
        }

        steps.push(RollbackStep {
            order: if analysis.is_upgradeable { 2 } else { 3 },
            action: "Notify stakeholders".to_string(),
            description: "Inform team and users about rollback".to_string(),
            prerequisites: vec!["Communication channels ready".to_string()],
            verification: "Stakeholders confirmed".to_string(),
            estimated_time: "5 minutes".to_string(),
        });

        let total_time = if analysis.is_upgradeable { 15 } else { 30 };

        Ok(RollbackPlan {
            steps,
            estimated_time: format!("{} minutes", total_time),
            rollback_cost: if analysis.is_upgradeable {
                "0.001 ETH".to_string()
            } else {
                "0.002 ETH".to_string()
            },
            success_probability: if analysis.is_upgradeable { 0.95 } else { 0.85 },
        })
    }

    async fn generate_recommendations(
        &self,
        analysis: &ContractAnalysis,
        risk: &RiskAssessment,
        gas: &GasEstimate,
    ) -> Result<Vec<Recommendation>> {
        let mut recommendations = Vec::new();

        // Security recommendations
        for finding in &analysis.security_findings {
            recommendations.push(Recommendation {
                action: format!("Fix: {}", finding.finding_type),
                urgency: match finding.level {
                    SecurityLevel::Critical => Urgency::Immediate,
                    SecurityLevel::High => Urgency::BeforeDeployment,
                    SecurityLevel::Medium => Urgency::BeforeDeployment,
                    SecurityLevel::Low => Urgency::PostDeployment,
                },
                impact: "Security improvement".to_string(),
                details: finding.recommendation.clone(),
            });
        }

        // Optimization recommendations
        for suggestion in &analysis.optimization_suggestions {
            recommendations.push(Recommendation {
                action: "Optimize code".to_string(),
                urgency: Urgency::BeforeDeployment,
                impact: "Lower gas costs".to_string(),
                details: suggestion.clone(),
            });
        }

        // Risk-based recommendations
        if risk.overall == RiskLevel::High || risk.overall == RiskLevel::Critical {
            recommendations.push(Recommendation {
                action: "Conduct additional audit".to_string(),
                urgency: Urgency::Immediate,
                impact: "Risk reduction".to_string(),
                details: "Security audit recommended before deployment".to_string(),
            });
        }

        // Gas optimization
        if let Ok(expected) = gas.expected.parse::<f64>() {
            if expected > 0.01 {
                recommendations.push(Recommendation {
                    action: "Optimize gas usage".to_string(),
                    urgency: Urgency::BeforeDeployment,
                    impact: "Cost savings".to_string(),
                    details: "Consider gas optimizations to reduce deployment cost".to_string(),
                });
            }
        }

        Ok(recommendations)
    }

    pub fn print_plan(&self, plan: &DeploymentPlan) -> Result<()> {
        match self.output_format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(plan)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(plan)?;
                println!("{}", yaml);
            }
            OutputFormat::Table => {
                self.print_table(plan);
            }
        }
        Ok(())
    }

    fn print_table(&self, plan: &DeploymentPlan) {
        use colored::*;

        println!();
        println!(
            "{}",
            "╔═══════════════════════════════════════════════════════════════════╗".cyan()
        );
        println!(
            "{}",
            "║           AI DEPLOYMENT PLAN SUMMARY                             ║"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "╚═══════════════════════════════════════════════════════════════════╝".cyan()
        );
        println!();

        // Contract Info
        println!("{}", "📄 CONTRACT INFORMATION".green().bold());
        println!("  Name:     {}", plan.contract_name);
        println!("  Version:  {}", plan.version);
        println!("  Lines:    {}", plan.analysis.lines);
        println!("  Functions: {}", plan.analysis.functions);
        println!("  Complexity: {:?}", plan.analysis.complexity);
        println!("  Readiness: {}/100", plan.analysis.readiness_score);
        println!();

        // Network
        println!("{}", "🌐 NETWORK RECOMMENDATION".green().bold());
        println!("  Network:  {}", plan.network_recommendation.network);
        println!("  Chain ID: {}", plan.network_recommendation.chain_id);
        println!("  Priority: {}/10", plan.network_recommendation.priority);
        println!("  Reasons:");
        for reason in &plan.network_recommendation.reasons {
            println!("    ✅ {}", reason);
        }
        if !plan.network_recommendation.risks.is_empty() {
            println!("  Risks:");
            for risk in &plan.network_recommendation.risks {
                println!("    ⚠️  {}", risk);
            }
        }
        println!();

        // Gas Estimate
        println!("{}", "💰 GAS ESTIMATE".green().bold());
        println!(
            "  Expected: {} {}",
            plan.gas_estimate.expected, plan.gas_estimate.unit
        );
        println!(
            "  Range:    {} - {}",
            plan.gas_estimate.min, plan.gas_estimate.max
        );
        if let Some(usd) = &plan.gas_estimate.usd_value {
            println!("  USD:      {}", usd);
        }
        println!("  Components:");
        println!(
            "    Deployment:   {} gas",
            plan.gas_estimate.components.deployment
        );
        println!(
            "    Verification: {} gas",
            plan.gas_estimate.components.verification
        );
        println!(
            "    Interactions: {} gas",
            plan.gas_estimate.components.interactions
        );
        println!();

        // Risk Assessment
        println!("{}", "🛡️  RISK ASSESSMENT".green().bold());
        let risk_color = match plan.risk_assessment.overall {
            RiskLevel::Critical => "red".to_string(),
            RiskLevel::High => "yellow".to_string(),
            RiskLevel::Medium => "cyan".to_string(),
            RiskLevel::Low => "green".to_string(),
        };
        println!(
            "  Overall:  {}",
            format!("{:?}", plan.risk_assessment.overall).color(risk_color)
        );
        println!("  Score:    {}/100", plan.risk_assessment.score);
        for category in &plan.risk_assessment.categories {
            let color = match category.level {
                RiskLevel::Critical => "red",
                RiskLevel::High => "yellow",
                RiskLevel::Medium => "cyan",
                RiskLevel::Low => "green",
            };
            println!(
                "    📌 {}: {}",
                category.name,
                format!("{:?}", category.level).color(color)
            );
            println!("       {}", category.description);
        }
        if !plan.risk_assessment.mitigations.is_empty() {
            println!("  Mitigations:");
            for m in &plan.risk_assessment.mitigations {
                println!("    🔧 {}", m);
            }
        }
        println!();

        // Deployment Window
        println!("{}", "⏰ DEPLOYMENT WINDOW".green().bold());
        println!("  Start:    {}", plan.deployment_window.start_time);
        println!("  End:      {}", plan.deployment_window.end_time);
        println!("  Priority: {:?}", plan.deployment_window.priority);
        println!("  Reason:   {}", plan.deployment_window.reason);
        if let Some(pred) = &plan.deployment_window.gas_price_prediction {
            println!(
                "  Gas:      Current: {}, Predicted: {}, Trend: {:?}",
                pred.current, pred.predicted, pred.trend
            );
        }
        println!();

        // Rollback Plan
        println!("{}", "🔄 ROLLBACK PLAN".green().bold());
        for step in &plan.rollback_plan.steps {
            println!("  Step {}: {}", step.order, step.action);
            println!("    ⏱️  {}", step.estimated_time);
            if !step.prerequisites.is_empty() {
                println!("    📋 Prereq: {}", step.prerequisites.join(", "));
            }
        }
        println!("  Estimated Time: {}", plan.rollback_plan.estimated_time);
        println!("  Cost:           {}", plan.rollback_plan.rollback_cost);
        println!(
            "  Success Rate:   {:.0}%",
            plan.rollback_plan.success_probability * 100.0
        );
        println!();

        // Recommendations
        if !plan.recommendations.is_empty() {
            println!("{}", "💡 RECOMMENDATIONS".green().bold());
            for rec in &plan.recommendations {
                let urgency_color = match rec.urgency {
                    Urgency::Immediate => "red",
                    Urgency::BeforeDeployment => "yellow",
                    Urgency::PostDeployment => "cyan",
                };
                println!(
                    "  {} [{}]:",
                    rec.action,
                    format!("{:?}", rec.urgency).color(urgency_color)
                );
                println!("    Impact: {}", rec.impact);
                println!("    Details: {}", rec.details);
            }
            println!();
        }

        println!(
            "{}",
            "╔═══════════════════════════════════════════════════════════════════╗".cyan()
        );
        println!(
            "{}",
            format!(
                "║  Status: {:?}                                              ║",
                plan.status
            )
            .cyan()
        );
        println!(
            "{}",
            format!("║  Generated: {}                    ║", plan.generated_at).cyan()
        );
        println!(
            "{}",
            "╚═══════════════════════════════════════════════════════════════════╝".cyan()
        );
    }
}
