use anyhow::{Context, Result};
use minijinja::Environment;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::PathBuf;

pub struct PromptManager {
    conn: Connection,
}

impl PromptManager {
    pub fn new() -> Result<Self> {
        let db_path = get_db_path()?;
        let conn = Connection::open(&db_path)?;
        let mut manager = Self { conn };
        manager.init_db()?;
        manager.seed_default_prompts()?;
        Ok(manager)
    }

    fn init_db(&mut self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS prompts (
                id INTEGER PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                category TEXT NOT NULL,
                description TEXT
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS prompt_versions (
                id INTEGER PRIMARY KEY,
                prompt_id INTEGER NOT NULL,
                version_tag TEXT NOT NULL,
                template_text TEXT NOT NULL,
                is_active BOOLEAN NOT NULL DEFAULT 0,
                FOREIGN KEY(prompt_id) REFERENCES prompts(id)
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS prompt_analytics (
                id INTEGER PRIMARY KEY,
                version_id INTEGER NOT NULL UNIQUE,
                uses INTEGER NOT NULL DEFAULT 0,
                successes INTEGER NOT NULL DEFAULT 0,
                failures INTEGER NOT NULL DEFAULT 0,
                rating_sum INTEGER NOT NULL DEFAULT 0,
                rating_count INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(version_id) REFERENCES prompt_versions(id)
            )",
            [],
        )?;

        Ok(())
    }

    fn seed_default_prompts(&mut self) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))?;
        if count == 0 {
            self.add_prompt_and_version(
                "contract_generator",
                "code_generation",
                "Generates Soroban contracts from natural language",
                "v1",
                "You are an expert Soroban smart contract developer. \
                Write ONLY valid, compilable Rust code for Soroban. \
                Include `#![no_std]`, proper `#[contract]`, `#[contractimpl]`, `#[contracttype]` macros. \
                {% if need_tests %}Include basic test scaffolding.{% endif %} \
                Do NOT wrap your response in markdown blocks. \
                User request: {{ user_prompt }}"
            )?;

            self.add_prompt_and_version(
                "security_reviewer",
                "security_review",
                "Reviews Soroban code for security vulnerabilities",
                "v1",
                "You are a top-tier security auditor for Stellar/Soroban. \
                Analyze the following code for vulnerabilities (reentrancy, arithmetic overflow, authorization bypass). \
                Code: {{ code }}"
            )?;

            self.add_prompt_and_version(
                "code_analyzer",
                "code_analysis",
                "Analyzes Soroban smart contracts for best practices",
                "v1",
                "Analyze this Soroban contract for gas optimization, state management, and best practices. \
                Code: {{ code }}"
            )?;

            self.add_prompt_and_version(
                "doc_generator",
                "documentation",
                "Generates comprehensive technical documentation",
                "v1",
                "Generate markdown documentation for the following Soroban contract. Include descriptions of public endpoints, structs, and events. \
                Code: {{ code }}"
            )?;

            self.add_prompt_and_version(
                "error_explainer",
                "error_explanation",
                "Explains build or runtime errors in plain language",
                "v1",
                "You are a helpful assistant. The user encountered the following Soroban error. Explain what it means and how to fix it. \
                Error: {{ error_msg }} \
                {% if code %}Code context: {{ code }}{% endif %}"
            )?;

            self.add_prompt_and_version(
                "test_generator",
                "test_generation",
                "Generates unit tests for Soroban contracts",
                "v1",
                "Write thorough Rust unit tests for this Soroban contract. Cover edge cases and authorization. \
                Code: {{ code }}"
            )?;
            self.add_prompt_and_version(
                "code_explainer",
                "code_explanation",
                "Multi-level AI code explanation",
                "v1",
                "You are an expert Soroban/Rust instructor. Explain the provided smart contract code.\n\
                 The user requested the explanation in {{ language }}.\n\
                 \n\
                 Explanation Level: {{ level }}
                {% if level == 'beginner' %}
                Use high-level concepts and simple analogies. Avoid deep technical jargon.
                {% elif level == 'intermediate' %}
                Focus on function details, parameter passing, and logic flow.
                {% elif level == 'advanced' %}
                Focus on Soroban implementation details, memory layout, and state management.
                {% elif level == 'expert' %}
                Focus heavily on gas optimization, security patterns, and low-level architecture.
                {% endif %}

                REQUIREMENTS:
                1. Always output a Mermaid markdown diagram (```mermaid) representing the code architecture or logic flow.
                2. Include Markdown links to official Soroban/Stellar documentation for key concepts discussed.
                
                Code to explain:
                {{ code }}"
            )?;
        }
        Ok(())
    }

    pub fn add_prompt_and_version(
        &mut self,
        name: &str,
        category: &str,
        description: &str,
        version_tag: &str,
        template_text: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO prompts (name, category, description) VALUES (?1, ?2, ?3)",
            params![name, category, description],
        )?;

        let prompt_id: i64 = self.conn.query_row(
            "SELECT id FROM prompts WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO prompt_versions (prompt_id, version_tag, template_text, is_active) VALUES (?1, ?2, ?3, 1)",
            params![prompt_id, version_tag, template_text],
        )?;

        let version_id = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO prompt_analytics (version_id) VALUES (?1)",
            params![version_id],
        )?;

        Ok(())
    }

    pub fn get_rendered_prompt(&self, name: &str, context: Value) -> Result<(i64, String)> {
        let mut stmt = self.conn.prepare(
            "SELECT v.id, v.template_text 
             FROM prompt_versions v
             JOIN prompts p ON v.prompt_id = p.id
             WHERE p.name = ?1 AND v.is_active = 1
             LIMIT 1",
        )?;

        let (version_id, template_text): (i64, String) = stmt
            .query_row(params![name], |row| Ok((row.get(0)?, row.get(1)?)))
            .context(format!("Failed to find active prompt for '{}'", name))?;

        let mut env = Environment::new();
        env.add_template(name, &template_text)?;
        let tmpl = env.get_template(name)?;

        let rendered = tmpl.render(context)?;

        self.conn.execute(
            "UPDATE prompt_analytics SET uses = uses + 1 WHERE version_id = ?1",
            params![version_id],
        )?;

        Ok((version_id, rendered))
    }

    pub fn record_feedback(
        &self,
        version_id: i64,
        success: bool,
        rating: Option<u8>,
    ) -> Result<()> {
        if success {
            self.conn.execute(
                "UPDATE prompt_analytics SET successes = successes + 1 WHERE version_id = ?1",
                params![version_id],
            )?;
        } else {
            self.conn.execute(
                "UPDATE prompt_analytics SET failures = failures + 1 WHERE version_id = ?1",
                params![version_id],
            )?;
        }

        if let Some(r) = rating {
            self.conn.execute(
                "UPDATE prompt_analytics SET rating_sum = rating_sum + ?1, rating_count = rating_count + 1 WHERE version_id = ?2",
                params![r, version_id],
            )?;
        }

        Ok(())
    }

    pub fn list_prompts(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.name, p.category, v.version_tag 
             FROM prompts p
             JOIN prompt_versions v ON p.id = v.prompt_id
             WHERE v.is_active = 1",
        )?;

        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;

        let mut prompts = Vec::new();
        for r in rows {
            prompts.push(r?);
        }
        Ok(prompts)
    }

    pub fn get_stats(&self) -> Result<Vec<(String, String, i64, i64, i64, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.name, v.version_tag, a.uses, a.successes, a.failures, 
                    CAST(a.rating_sum AS REAL) / NULLIF(a.rating_count, 0)
             FROM prompt_analytics a
             JOIN prompt_versions v ON a.version_id = v.id
             JOIN prompts p ON v.prompt_id = p.id",
        )?;

        let rows = stmt.query_map([], |row| {
            let avg: Option<f64> = row.get(5)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                avg.unwrap_or(0.0),
            ))
        })?;

        let mut stats = Vec::new();
        for r in rows {
            stats.push(r?);
        }
        Ok(stats)
    }

    pub fn set_active_version(&self, name: &str, version_tag: &str) -> Result<()> {
        let prompt_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM prompts WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .context("Prompt not found")?;

        self.conn.execute(
            "UPDATE prompt_versions SET is_active = 0 WHERE prompt_id = ?1",
            params![prompt_id],
        )?;

        let updated = self.conn.execute(
            "UPDATE prompt_versions SET is_active = 1 WHERE prompt_id = ?1 AND version_tag = ?2",
            params![prompt_id, version_tag],
        )?;

        if updated == 0 {
            anyhow::bail!("Version tag not found");
        }

        Ok(())
    }
}

fn get_db_path() -> Result<PathBuf> {
    let mut dir = crate::utils::config::config_dir();
    std::fs::create_dir_all(&dir)?;
    dir.push("prompts.db");
    Ok(dir)
}
