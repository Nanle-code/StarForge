//! AI Documentation Q&A (#512)
//!
//! Answers questions about StarForge, Stellar, and Soroban documentation using
//! a retrieval-augmented generation (RAG) pipeline:
//!
//! 1. **Documentation indexing** — local Markdown/text docs (plus a curated
//!    built-in knowledge base for Stellar/Soroban/StarForge) are chunked and
//!    stored in an in-memory index with source metadata and URLs.
//! 2. **Question understanding** — the question is analysed for language,
//!    intent, and topics so the right documentation is retrieved.
//! 3. **Accurate answer generation** — the top-ranked documentation chunks are
//!    injected into an LLM prompt (via Ollama) so answers are grounded in the
//!    knowledge base rather than hallucinated. If Ollama is unavailable, an
//!    extractive fallback returns the most relevant documentation excerpts.
//! 4. **Source citation** — every answer carries citations (source, title, URL,
//!    snippet) that can be traced back to the indexed documentation.
//! 5. **Follow-up questions** — sessions retain conversation history so later
//!    questions ("what about on mainnet?", "and the gas cost?") resolve against
//!    the earlier context.
//! 6. **Multi-language support** — questions are auto-detected or explicitly
//!    set to one of the supported languages; the LLM is instructed to answer in
//!    that language.

use crate::utils::ai_cache;
use crate::utils::config;
use crate::utils::ollama;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ─── Multi-language support ─────────────────────────────────────────────────

/// Languages the Q&A system can answer in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QaLanguage {
    English,
    Spanish,
    French,
    German,
    Chinese,
    Japanese,
    Korean,
    Portuguese,
    Russian,
    Arabic,
}

impl QaLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            QaLanguage::English => "en",
            QaLanguage::Spanish => "es",
            QaLanguage::French => "fr",
            QaLanguage::German => "de",
            QaLanguage::Chinese => "zh",
            QaLanguage::Japanese => "ja",
            QaLanguage::Korean => "ko",
            QaLanguage::Portuguese => "pt",
            QaLanguage::Russian => "ru",
            QaLanguage::Arabic => "ar",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            QaLanguage::English => "English",
            QaLanguage::Spanish => "Spanish",
            QaLanguage::French => "French",
            QaLanguage::German => "German",
            QaLanguage::Chinese => "Chinese",
            QaLanguage::Japanese => "Japanese",
            QaLanguage::Korean => "Korean",
            QaLanguage::Portuguese => "Portuguese",
            QaLanguage::Russian => "Russian",
            QaLanguage::Arabic => "Arabic",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Some(QaLanguage::English),
            "es" | "spanish" | "espanol" => Some(QaLanguage::Spanish),
            "fr" | "french" | "francais" => Some(QaLanguage::French),
            "de" | "german" | "deutsch" => Some(QaLanguage::German),
            "zh" | "chinese" | "mandarin" => Some(QaLanguage::Chinese),
            "ja" | "japanese" | "nihongo" => Some(QaLanguage::Japanese),
            "ko" | "korean" | "hangul" => Some(QaLanguage::Korean),
            "pt" | "portuguese" => Some(QaLanguage::Portuguese),
            "ru" | "russian" => Some(QaLanguage::Russian),
            "ar" | "arabic" => Some(QaLanguage::Arabic),
            _ => None,
        }
    }

    pub fn all() -> Vec<QaLanguage> {
        vec![
            QaLanguage::English,
            QaLanguage::Spanish,
            QaLanguage::French,
            QaLanguage::German,
            QaLanguage::Chinese,
            QaLanguage::Japanese,
            QaLanguage::Korean,
            QaLanguage::Portuguese,
            QaLanguage::Russian,
            QaLanguage::Arabic,
        ]
    }

    /// Detect a language from raw text using script heuristics.
    pub fn detect(text: &str) -> QaLanguage {
        let mut cjk = 0usize;
        let mut cyrillic = 0usize;
        let mut arabic = 0usize;
        let mut accent = 0usize;
        let mut ascii = 0usize;

        for c in text.chars() {
            let cp = c as u32;
            if (0x4E00..=0x9FFF).contains(&cp) || (0x3040..=0x30FF).contains(&cp) {
                cjk += 1;
            } else if (0x0400..=0x04FF).contains(&cp) {
                cyrillic += 1;
            } else if (0x0600..=0x06FF).contains(&cp) {
                arabic += 1;
            } else if matches!(
                c,
                'á' | 'é'
                    | 'í'
                    | 'ó'
                    | 'ú'
                    | 'ñ'
                    | 'ü'
                    | 'ç'
                    | 'ã'
                    | 'õ'
                    | 'à'
                    | 'â'
                    | 'ê'
                    | 'ô'
            ) {
                accent += 1;
            } else if c.is_ascii_alphabetic() {
                ascii += 1;
            }
        }

        let total = cjk + cyrillic + arabic + accent + ascii;
        if total == 0 {
            return QaLanguage::English;
        }
        if cjk > ascii / 2 {
            // Kanji vs Hangul vs Han: default to Chinese for Han, Japanese if kana present.
            if text
                .chars()
                .any(|c| (0x3040..=0x30FF).contains(&(c as u32)))
            {
                QaLanguage::Japanese
            } else if text
                .chars()
                .any(|c| (0xAC00..=0xD7AF).contains(&(c as u32)))
            {
                QaLanguage::Korean
            } else {
                QaLanguage::Chinese
            }
        } else if cyrillic > ascii / 2 {
            QaLanguage::Russian
        } else if arabic > ascii / 2 {
            QaLanguage::Arabic
        } else if text.contains('¿') || text.contains('¡') || accent >= 3 {
            QaLanguage::Spanish
        } else {
            QaLanguage::English
        }
    }
}

// ─── Documentation sources ───────────────────────────────────────────────────

/// Which knowledge base a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    /// StarForge's own documentation.
    StarForge,
    /// Stellar network documentation (developers.stellar.org).
    Stellar,
    /// Soroban smart contract SDK documentation.
    SorobanSdk,
    /// Community resources (forums, guides, blog posts).
    Community,
    /// Best practices and conventions.
    BestPractices,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::StarForge => "StarForge",
            SourceKind::Stellar => "Stellar",
            SourceKind::SorobanSdk => "Soroban SDK",
            SourceKind::Community => "Community",
            SourceKind::BestPractices => "Best Practices",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "starforge" => Some(SourceKind::StarForge),
            "stellar" => Some(SourceKind::Stellar),
            "soroban" | "soroban-sdk" | "sdk" => Some(SourceKind::SorobanSdk),
            "community" => Some(SourceKind::Community),
            "best-practices" | "best_practices" | "practices" => Some(SourceKind::BestPractices),
            _ => None,
        }
    }
}

// ─── Indexed documentation ───────────────────────────────────────────────────

/// A single retrievable documentation chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocChunk {
    pub id: String,
    /// Human readable source name (file path or doc title).
    pub source: String,
    pub kind: SourceKind,
    pub title: String,
    /// Original documentation URL, when known.
    pub url: Option<String>,
    pub content: String,
    pub language: QaLanguage,
    pub chunk_index: usize,
}

/// A retrieved documentation chunk together with its relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub chunk: DocChunk,
    pub score: f64,
}

/// Statistics about the built documentation index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_chunks: usize,
    pub total_sources: usize,
    pub by_kind: HashMap<String, usize>,
    pub by_language: HashMap<String, usize>,
    pub built_at: DateTime<Utc>,
}

/// The documentation knowledge base.
#[derive(Debug, Clone)]
pub struct DocIndex {
    pub chunks: Vec<DocChunk>,
    pub built_at: DateTime<Utc>,
}

impl DocIndex {
    pub fn new() -> Self {
        DocIndex {
            chunks: Vec::new(),
            built_at: Utc::now(),
        }
    }

    pub fn stats(&self) -> IndexStats {
        let mut by_kind = HashMap::new();
        let mut by_language = HashMap::new();
        for chunk in &self.chunks {
            *by_kind.entry(chunk.kind.as_str().to_string()).or_insert(0) += 1;
            *by_language
                .entry(chunk.language.display().to_string())
                .or_insert(0) += 1;
        }
        let mut sources = std::collections::HashSet::new();
        for chunk in &self.chunks {
            sources.insert(chunk.source.clone());
        }
        IndexStats {
            total_chunks: self.chunks.len(),
            total_sources: sources.len(),
            by_kind,
            by_language,
            built_at: self.built_at,
        }
    }

    /// Retrieve the most relevant chunks for a set of query tokens.
    pub fn retrieve(&self, tokens: &[String], limit: usize, min_score: f64) -> Vec<SearchHit> {
        let mut hits: Vec<SearchHit> = self
            .chunks
            .iter()
            .map(|chunk| {
                let score = score_chunk(chunk, tokens);
                SearchHit {
                    chunk: chunk.clone(),
                    score,
                }
            })
            .filter(|hit| hit.score >= min_score)
            .collect();

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        hits
    }
}

impl Default for DocIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Question analysis ───────────────────────────────────────────────────────

/// The intent behind a documentation question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestionIntent {
    HowTo,
    WhatIs,
    Why,
    Troubleshooting,
    Comparison,
    General,
}

impl QuestionIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            QuestionIntent::HowTo => "how-to",
            QuestionIntent::WhatIs => "what-is",
            QuestionIntent::Why => "why",
            QuestionIntent::Troubleshooting => "troubleshooting",
            QuestionIntent::Comparison => "comparison",
            QuestionIntent::General => "general",
        }
    }
}

/// The result of understanding a question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnalysis {
    pub topics: Vec<String>,
    pub intent: QuestionIntent,
    pub language: QaLanguage,
    pub tokens: Vec<String>,
}

/// Knowledge domains the index can answer about.
const TOPIC_INDEX: &[(&str, &[&str])] = &[
    ("deployment", &["deploy", "deploying", "wasm", "upload"]),
    ("wallet", &["wallet", "fund", "keys", "secret", "account"]),
    ("gas", &["gas", "fee", "cost", "pricing"]),
    (
        "authentication",
        &["auth", "authentication", "signature", "signer", "authorize"],
    ),
    ("token", &["token", "mint", "balance", "transfer", "asset"]),
    (
        "storage",
        &["storage", "persistent", "instance", "temporary", "ledger"],
    ),
    ("testing", &["test", "testing", "assert", "unit"]),
    (
        "security",
        &["security", "vulnerability", "exploit", "audit", "sandbox"],
    ),
    (
        "error-handling",
        &["error", "panic", "fail", "exception", "retry"],
    ),
    ("migration", &["migrate", "migration", "upgrade", "version"]),
    (
        "network",
        &["network", "testnet", "mainnet", "horizon", "rpc", "node"],
    ),
    (
        "soroban",
        &["soroban", "contract", "smart", "env", "invoke"],
    ),
    (
        "stellar",
        &["stellar", "lumens", "xlm", "payment", "trustline"],
    ),
    ("events", &["event", "log", "emit"]),
];

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "do", "does", "did", "can",
    "could", "should", "would", "will", "may", "might", "of", "to", "in", "on", "for", "with",
    "at", "by", "from", "what", "how", "why", "when", "where", "which", "and", "or", "but", "not",
    "this", "that", "these", "those", "my", "your", "its", "about", "into", "over", "under",
    "then", "than", "too", "very", "just", "i",
];

/// Tokenize a query into search terms, dropping stopwords and short tokens.
pub fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        let token = raw.to_ascii_lowercase();
        if token.len() < 3 || STOPWORDS.contains(&token.as_str()) || seen.contains(&token) {
            continue;
        }
        seen.insert(token.clone());
        tokens.push(token);
    }
    tokens
}

/// Analyse a question for language, intent, and topics.
pub fn analyze_question(question: &str) -> QuestionAnalysis {
    let language = QaLanguage::detect(question);
    let lower = question.to_lowercase();
    let tokens = tokenize(question);

    let intent =
        if lower.starts_with("how") || lower.starts_with("what do i") || lower.contains("steps to")
        {
            QuestionIntent::HowTo
        } else if lower.starts_with("what is")
            || lower.starts_with("what are")
            || lower.starts_with("what's")
        {
            QuestionIntent::WhatIs
        } else if lower.contains("error")
            || lower.contains("fail")
            || lower.contains("not work")
            || lower.contains("fix")
            || lower.contains("problem")
            || lower.contains("issue")
        {
            QuestionIntent::Troubleshooting
        } else if lower.starts_with("why") || lower.contains("reason") {
            QuestionIntent::Why
        } else if lower.contains(" vs ")
            || lower.contains("difference")
            || lower.contains("compare")
            || lower.contains("better")
        {
            QuestionIntent::Comparison
        } else {
            QuestionIntent::General
        };

    let mut topics = Vec::new();
    for (domain, keywords) in TOPIC_INDEX {
        if keywords.iter().any(|k| lower.contains(k)) {
            topics.push(domain.to_string());
        }
    }

    QuestionAnalysis {
        topics,
        intent,
        language,
        tokens,
    }
}

// ─── Scoring ─────────────────────────────────────────────────────────────────

/// Score a chunk against query tokens. Title matches are weighted higher and
/// multi-token co-occurrence inside a single chunk is rewarded.
fn score_chunk(chunk: &DocChunk, tokens: &[String]) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let content_lower = chunk.content.to_lowercase();
    let title_lower = chunk.title.to_lowercase();
    let source_lower = chunk.source.to_lowercase();

    let mut score = 0.0;
    for token in tokens {
        if content_lower.contains(token) {
            score += 1.0;
        }
        if title_lower.contains(token) {
            score += 2.5;
        }
        if source_lower.contains(token) {
            score += 1.5;
        }
    }
    // Reward chunks where all query tokens appear together (higher precision).
    if tokens.iter().all(|t| content_lower.contains(t)) {
        score += 2.0;
    }
    score
}

// ─── Indexing ────────────────────────────────────────────────────────────────

/// Options for building the documentation index.
#[derive(Debug, Clone)]
pub struct IndexOptions {
    /// Additional directories to walk for local documentation.
    pub extra_dirs: Vec<PathBuf>,
    /// Maximum number of local files to index (performance guard).
    pub max_files: usize,
    /// Target chunk size in characters.
    pub chunk_size: usize,
    /// Overlap between consecutive chunks.
    pub chunk_overlap: usize,
    /// When true, seed the index with the curated Stellar/Soroban knowledge base.
    pub include_builtin: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            extra_dirs: Vec::new(),
            max_files: 300,
            chunk_size: 900,
            chunk_overlap: 120,
            include_builtin: true,
        }
    }
}

/// Build the documentation index.
///
/// The index is composed of:
/// - The curated built-in knowledge base (Stellar, Soroban SDK, StarForge,
///   best practices) with links to the official documentation.
/// - Local StarForge Markdown docs (README, docs/, tutorials/, and the
///   repository root `*.md` files).
/// - Any additional directories supplied by the user.
pub fn build_index(options: &IndexOptions) -> Result<DocIndex> {
    let mut index = DocIndex::new();

    if options.include_builtin {
        for chunk in builtin_knowledge_base() {
            index.chunks.push(chunk);
        }
    }

    let mut visited = std::collections::HashSet::new();
    let mut file_count = 0usize;

    let mut dirs = Vec::new();
    if let Ok(root) = std::env::current_dir() {
        dirs.push(root.clone());
        dirs.push(root.join("docs"));
        dirs.push(root.join("tutorials"));
    }
    dirs.extend(options.extra_dirs.iter().cloned());

    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        let mut files = Vec::new();
        walk_docs(&dir, &mut files);
        // Prefer top-level project docs so meaningful files are indexed first.
        files.sort();
        for file in files {
            if file_count >= options.max_files {
                break;
            }
            let canonical = fs::canonicalize(&file).unwrap_or_else(|_| file.clone());
            let key = canonical.display().to_string();
            if !visited.insert(key) {
                continue;
            }
            let kind = classify_source(&file);
            match index_file(&file, kind, options) {
                Ok(chunks) => {
                    index.chunks.extend(chunks);
                    file_count += 1;
                }
                Err(_) => continue,
            }
        }
        if file_count >= options.max_files {
            break;
        }
    }

    if index.chunks.is_empty() {
        anyhow::bail!(
            "No documentation found to index. Run from inside the StarForge repo or pass --dir."
        );
    }

    index.built_at = Utc::now();
    Ok(index)
}

/// Walk a directory collecting Markdown and text documentation files.
fn walk_docs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if matches!(name.as_str(), "target" | ".git" | "node_modules" | ".cargo") {
                continue;
            }
            walk_docs(&path, out);
        } else {
            let Some(ext) = path.extension() else {
                continue;
            };
            let ext = ext.to_string_lossy().to_lowercase();
            if matches!(ext.as_str(), "md" | "txt" | "rst" | "adoc") {
                out.push(path);
            }
        }
    }
}

/// Determine which knowledge base a local file belongs to.
fn classify_source(path: &Path) -> SourceKind {
    let lower = path.to_string_lossy().to_lowercase();
    if lower.contains("docs/") && lower.contains("gas")
        || lower.contains("best_practices")
        || lower.contains("best-practices")
        || lower.contains("standards")
    {
        SourceKind::BestPractices
    } else if lower.contains("stellar") || lower.contains("soroban") {
        SourceKind::SorobanSdk
    } else {
        SourceKind::StarForge
    }
}

/// Chunk a block of text into overlapping pieces.
fn chunk_text(content: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    if content.is_empty() {
        return vec![];
    }
    let step = chunk_size.saturating_sub(overlap).max(1);
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < content.len() {
        let mut end = (start + chunk_size).min(content.len());
        if end < content.len() {
            // Prefer breaking on a paragraph or sentence boundary.
            if let Some(rel) = content[start..end].rfind("\n\n") {
                end = start + rel;
            } else if let Some(rel) = content[start..end].rfind(". ") {
                end = start + rel + 1;
            }
            end = end.max(start + 1);
        }
        let mut piece = content[start..end].trim().to_string();
        if piece.len() > chunk_size {
            piece = piece[..chunk_size].to_string();
        }
        if !piece.is_empty() {
            chunks.push(piece);
        }
        if end >= content.len() {
            break;
        }
        start = end.saturating_sub(overlap / 2);
    }
    // If the document was too small to chunk, keep it whole.
    if chunks.is_empty() {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(trimmed);
        }
    }
    chunks
}

/// Index a single file into chunks.
fn index_file(path: &Path, kind: SourceKind, options: &IndexOptions) -> Result<Vec<DocChunk>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if content.len() > 2_000_000 {
        return Ok(vec![]);
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let title = file_name.trim_end_matches(".md");
    let relative = path.display().to_string();
    let chunks = chunk_text(&content, options.chunk_size, options.chunk_overlap);

    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(i, content)| DocChunk {
            id: format!("file:{}:{}", relative, i),
            source: relative.clone(),
            kind,
            title: title.to_string(),
            url: None,
            content,
            language: QaLanguage::English,
            chunk_index: i,
        })
        .collect())
}

// ─── Built-in knowledge base ─────────────────────────────────────────────────

struct SeedDoc {
    kind: SourceKind,
    title: &'static str,
    url: &'static str,
    content: &'static str,
}

const BUILTIN_DOCS: &[SeedDoc] = &[
    SeedDoc {
        kind: SourceKind::StarForge,
        title: "StarForge Overview",
        url: "https://github.com/Nanle-code/StarForge",
        content:
            "StarForge is a developer toolchain for building, testing, deploying, and managing \
Soroban smart contracts on the Stellar network. It provides a CLI (starforge) for contract \
scaffolding (starforge new), wallet management (starforge wallet), contract invocation \
(starforge contract invoke), deployment (starforge deploy), gas analysis (starforge gas), \
testing (starforge test), and AI-assisted development features such as code search, debugging, \
and documentation Q&A. StarForge supports testnet, mainnet, and local development networks. \
Projects are generated as Rust crates using the Soroban SDK.",
    },
    SeedDoc {
        kind: SourceKind::StarForge,
        title: "StarForge Deployment",
        url: "https://github.com/Nanle-code/StarForge",
        content: "To deploy a Soroban contract with StarForge, first compile the contract to WASM \
(cargo build --target wasm32-unknown-unknown --release), then run starforge deploy with the \
compiled .wasm file. You must have a funded wallet configured (starforge wallet create) and select \
a network (starforge network or --network testnet). Deployment uploads the WASM and creates a \
contract instance. Deployment history, verification, and rollback are available through \
starforge deployments.",
    },
    SeedDoc {
        kind: SourceKind::StarForge,
        title: "StarForge Wallets",
        url: "https://github.com/Nanle-code/StarForge",
        content:
            "StarForge wallets hold the keypairs used to sign and submit Stellar transactions. \
Create a wallet with starforge wallet create, list configured wallets with starforge wallet list, \
and fund a wallet on testnet using the Friendbot or starforge wallet fund. Wallet secrets are \
encrypted at rest and keys are derived from a BIP39 mnemonic. Ledger and Trezor hardware wallets \
are supported for signing.",
    },
    SeedDoc {
        kind: SourceKind::Stellar,
        title: "Stellar Network Basics",
        url: "https://developers.stellar.org/docs/learn/fundamentals",
        content:
            "The Stellar network is an open, decentralized payment network built to make money \
movement fast, cheap, and global. Stellar's native asset is the lumen (XLM). Accounts are \
identified by Stellar public keys (G... addresses). Every account has a sequence number that must \
increment with each transaction, a base reserve requirement, and a set of authorized signers. \
Transactions are submitted to the network and processed by validators running the Stellar Core \
protocol.",
    },
    SeedDoc {
        kind: SourceKind::Stellar,
        title: "Stellar Accounts and Transactions",
        url: "https://developers.stellar.org/docs/learn/fundamentals/transactions",
        content:
            "A Stellar account is created by funding it with a minimum balance (base reserve). \
Transactions group operations (payments, create accounts, manage trustlines, path payments, \
claimable balances) and must be signed by enough authorized signers. Each transaction specifies a \
fee (in stroops), a source account, and a sequence number one higher than the account's current \
sequence. The Horizon API (developers.stellar.org/docs/data) is the REST interface to query \
accounts, transactions, and ledgers.",
    },
    SeedDoc {
        kind: SourceKind::Stellar,
        title: "Stellar Assets and Trustlines",
        url: "https://developers.stellar.org/docs/learn/fundamentals/stellar-assets",
        content:
            "Stellar supports both the native asset (XLM) and custom issued assets. Issuing an \
asset requires the issuer account and users must establish a trustline (manage trustline \
operation) to hold the asset. The asset is identified by the asset code and the issuer's public \
key. Issuer controls the maximum amount and can freeze or clawback depending on the flags \
configured on the issuing account. Custom assets enable multi-asset payments, DEX trading, and \
liquidity pools.",
    },
    SeedDoc {
        kind: SourceKind::Stellar,
        title: "Stellar Fees and Sequence Numbers",
        url: "https://developers.stellar.org/docs/learn/fundamentals/transactions/fees",
        content:
            "Stellar transaction fees are paid in stroops (1 lumen = 10,000,000 stroops) and are \
set by the transaction's fee field. Fees are tiny relative to other networks. Each account has a \
monotonically increasing sequence number; a submitted transaction must use sequence_number + 1, \
otherwise it is rejected. This prevents replay attacks and enforces ordering. Failed transactions \
still increment the sequence number. Operations also have a per-operation fee multiplier known as \
the fee bump mechanism (fee-bump transactions) for sponsored operations.",
    },
    SeedDoc {
        kind: SourceKind::SorobanSdk,
        title: "Soroban Smart Contracts Overview",
        url: "https://developers.stellar.org/docs/soroban-and-smart-contracts",
        content:
            "Soroban is the smart contract platform on the Stellar network. Soroban contracts \
are written in Rust using the Soroban SDK and compiled to WASM. A contract is defined with the \
#[contract] attribute and implements functions decorated with #[contractimpl]. Contract functions \
receive an Env parameter to access ledger storage, emit events, call other contracts, and interact \
with the network. Contracts are deterministic and sandboxed, with metered execution and a gas \
limit per operation.",
    },
    SeedDoc {
        kind: SourceKind::SorobanSdk,
        title: "Soroban Env and Storage",
        url: "https://developers.stellar.org/docs/soroban-and-smart-contracts/env-and-storage",
        content: "The Soroban Env is the interface between a contract and the ledger. Storage has \
three types: temporary (Temporary), persistent (Persistent), and instance (Instance). Temporary \
storage is for short-lived data, persistent storage for long-lived user data, and instance storage \
for per-contract-instance data. Use env.storage().get(&key), env.storage().set(&key, &val), \
env.storage().del(&key) and access TTL management functions (extend_ttl) to manage data \
lifetimes. Keys are typically the contract's own scval or a combination of the caller and a \
constant.",
    },
    SeedDoc {
        kind: SourceKind::SorobanSdk,
        title: "Soroban Authentication",
        url: "https://developers.stellar.org/docs/soroban-and-smart-contracts/authentication",
        content: "Soroban contract authentication uses Stellar's Ed25519 signatures and \
Authorization (auth) entries. Contracts authorize actions on behalf of users using \
env.authorize_as_current_contract() or by requiring signers via the soroban_sdk::auth module. \
The #[contractimpl] #[allow_non_spec_constructors] and auth patterns let contracts check that a \
caller signed a particular action. Cross-contract calls pass authorization via Soroban \
authorization entries, and token contracts use the token interface (SorobanToken) which requires \
authorized transfers (transfer_from).",
    },
    SeedDoc {
        kind: SourceKind::SorobanSdk,
        title: "Soroban Tokens (Token Interface)",
        url: "https://developers.stellar.org/docs/soroban-and-smart-contracts/tokens",
        content: "Soroban provides a standard token interface (Stellar Asset Contract) used by \
wrapped assets and custom tokens. The interface includes name, symbol, decimals, balance, \
spendable_balance, transfer, transfer_from, mint, burn, allowance, approve, and set_authorized. \
Token contracts track balances in persistent storage and enforce authorization. To create a \
custom token, implement the token interface (or use the built-in Stellar Asset Contract) and \
deploy it with a class and address.",
    },
    SeedDoc {
        kind: SourceKind::SorobanSdk,
        title: "Soroban Events",
        url: "https://developers.stellar.org/docs/soroban-and-smart-contracts/events",
        content:
            "Soroban contracts emit events with env.events().publish((topics, data)) to record \
state changes on-chain. Events have topics (a symbol-like identifier and optional keys) and a data \
value (any Soroban value). Events are stored in the ledger and can be queried via the Soroban RPC \
API. Emitting events for transfers, mints, burns, and admin actions is considered best practice \
for off-chain indexing and user notifications.",
    },
    SeedDoc {
        kind: SourceKind::BestPractices,
        title: "Soroban Contract Best Practices",
        url: "https://developers.stellar.org/docs/soroban-and-smart-contracts/guides/security",
        content: "Best practices for Soroban contracts: validate all inputs and require \
authorization for privileged operations; use instance storage for contract configuration and \
persistent storage for user data; extend TTLs on long-lived storage to avoid data expiration; \
handle panic and error cases with clear error types; emit events for every significant state \
change; test with the Soroban SDK testutils; profile gas with starforge gas; and run security \
reviews before mainnet deployment. Avoid unbounded loops and expensive storage reads inside hot \
paths.",
    },
    SeedDoc {
        kind: SourceKind::BestPractices,
        title: "Stellar Integration Best Practices",
        url: "https://developers.stellar.org/docs/guides",
        content: "Best practices when integrating with Stellar: always poll or stream Horizon for \
transaction confirmations instead of assuming finality; implement sequence number management to \
avoid stale transactions; build in retry with exponential backoff for network failures; fund \
accounts with more than the minimum reserve to cover operations; use fee-bump or sponsored \
operations for user-facing apps; and validate transaction results and memo fields before \
crediting users. Keep secret keys encrypted at rest and never log sensitive data.",
    },
    SeedDoc {
        kind: SourceKind::Community,
        title: "Soroban Development Resources",
        url: "https://soroban.stellar.org",
        content: "The Soroban developer community provides tutorials, example contracts (hello \
world, token, NFT, DEX), and forums. The Stellar developer discord and the Stellar Stack Exchange \
are good places to ask questions. The official Soroban docs include getting started guides, the \
SDK reference (rustdoc), and the Soroban CLI (soroban contract deploy). Community best practices \
recommend starting with the hello world tutorial and the token example before building complex \
contracts.",
    },
];

/// Seed the index with curated documentation for Stellar, Soroban, and StarForge.
fn builtin_knowledge_base() -> Vec<DocChunk> {
    BUILTIN_DOCS
        .iter()
        .enumerate()
        .flat_map(|(doc_idx, doc)| {
            chunk_text(doc.content, 900, 120)
                .into_iter()
                .enumerate()
                .map(move |(chunk_idx, content)| DocChunk {
                    id: format!("builtin:{}:{}", doc_idx, chunk_idx),
                    source: doc.title.to_string(),
                    kind: doc.kind,
                    title: doc.title.to_string(),
                    url: Some(doc.url.to_string()),
                    content,
                    language: QaLanguage::English,
                    chunk_index: chunk_idx,
                })
        })
        .collect()
}

// ─── Answer generation ───────────────────────────────────────────────────────

/// A citation linking an answer back to the indexed documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub source: String,
    pub title: String,
    pub url: Option<String>,
    pub kind: SourceKind,
    pub snippet: String,
}

/// A complete answer produced by the Q&A engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaAnswer {
    pub question: String,
    pub answer: String,
    pub citations: Vec<Citation>,
    pub language: QaLanguage,
    pub confidence: f64,
    pub mode: AnswerMode,
    pub follow_up_suggestions: Vec<String>,
    pub latency_ms: u128,
}

/// How the answer was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnswerMode {
    /// Grounded, generated by an LLM from retrieved documentation context.
    Generated,
    /// Extractive fallback: the top documentation excerpts returned verbatim.
    Extractive,
}

/// Follow-up conversation messages, persisted across invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaMessage {
    pub role: QaMessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QaMessageRole {
    User,
    Assistant,
    System,
}

/// A documentation Q&A session (enables follow-up questions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaSession {
    pub session_id: String,
    pub messages: Vec<QaMessage>,
    pub preferred_language: QaLanguage,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
}

impl Default for QaSession {
    fn default() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            messages: Vec::new(),
            preferred_language: QaLanguage::English,
            created_at: Utc::now(),
            last_updated: Utc::now(),
        }
    }
}

/// Persistent session store backing follow-up question support.
#[derive(Debug, Default)]
pub struct QaSessionStore {
    sessions: HashMap<String, QaSession>,
}

impl QaSessionStore {
    fn store_path() -> Result<PathBuf> {
        let dir = config::get_data_dir()?.join("ai_doc_qa");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("sessions.json"))
    }

    pub fn load() -> Self {
        let path = match Self::store_path() {
            Ok(p) => p,
            Err(_) => return QaSessionStore::default(),
        };
        match fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<QaSession>>(&raw).ok())
        {
            Some(sessions) => {
                let mut map = HashMap::new();
                for s in sessions {
                    map.insert(s.session_id.clone(), s);
                }
                QaSessionStore { sessions: map }
            }
            None => QaSessionStore::default(),
        }
    }

    pub fn save(&self) {
        let Ok(path) = Self::store_path() else {
            return;
        };
        let mut sessions: Vec<QaSession> = self.sessions.values().cloned().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_updated));
        if let Ok(json) = serde_json::to_string_pretty(&sessions) {
            let _ = fs::write(path, json);
        }
    }

    pub fn create(&mut self, language: QaLanguage) -> QaSession {
        let mut session = QaSession {
            preferred_language: language,
            ..QaSession::default()
        };
        session.messages.push(QaMessage {
            role: QaMessageRole::System,
            content: format!(
                "Documentation Q&A session. Preferred language: {}.",
                language.display()
            ),
            timestamp: Utc::now(),
        });
        self.sessions
            .insert(session.session_id.clone(), session.clone());
        session
    }

    pub fn get(&self, session_id: &str) -> Option<QaSession> {
        self.sessions.get(session_id).cloned()
    }

    pub fn update(&mut self, session: &QaSession) {
        self.sessions
            .insert(session.session_id.clone(), session.clone());
    }

    pub fn delete(&mut self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    pub fn list(&self) -> Vec<QaSession> {
        let mut sessions: Vec<QaSession> = self.sessions.values().cloned().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_updated));
        sessions
    }
}

/// The top-level Q&A engine.
pub struct DocQaEngine {
    pub index: DocIndex,
    pub store: QaSessionStore,
}

impl DocQaEngine {
    pub fn new(index: DocIndex) -> Self {
        DocQaEngine {
            index,
            store: QaSessionStore::load(),
        }
    }

    /// Ask a single question (optionally within a follow-up session).
    ///
    /// When `session_id` is provided, recent conversation history is included in
    /// the LLM prompt so follow-up questions resolve against earlier context.
    pub async fn ask(
        &mut self,
        question: &str,
        session_id: Option<&str>,
        language: Option<QaLanguage>,
    ) -> Result<QaAnswer> {
        let started = std::time::Instant::now();
        let question = question.trim();
        if question.is_empty() {
            anyhow::bail!("Question must not be empty");
        }

        let analysis = analyze_question(question);
        let answer_language = language.unwrap_or_else(|| analysis.language);

        let tokens = analysis.tokens.clone();
        let mut hits = self.index.retrieve(&tokens, 6, 1.0);

        // If the question has no usable tokens, fall back to topic-based search.
        if hits.is_empty() {
            for topic in &analysis.topics {
                let topic_tokens = tokenize(topic);
                hits.extend(self.index.retrieve(&topic_tokens, 3, 0.0));
            }
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hits.dedup_by(|a, b| a.chunk.id == b.chunk.id);
            hits.truncate(6);
        }

        // Assemble the context block from the top hits.
        let mut context_blocks = Vec::new();
        for (i, hit) in hits.iter().enumerate() {
            context_blocks.push(format!(
                "[{}] Source: {} ({}) | Title: {}{}\n{}",
                i + 1,
                hit.chunk.source,
                hit.chunk.kind.as_str(),
                hit.chunk.title,
                hit.chunk
                    .url
                    .as_ref()
                    .map(|u| format!(" | URL: {}", u))
                    .unwrap_or_default(),
                hit.chunk.content
            ));
        }

        let context = context_blocks.join("\n\n---\n\n");

        let answer = if ollama::is_ollama_running().await {
            let prompt = build_qa_prompt(
                question,
                &context,
                answer_language,
                session_id.and_then(|id| self.store.get(id)),
                analysis.intent,
            );
            match ollama::generate_cached(
                ollama::DEFAULT_MODEL,
                &prompt,
                Some(ollama::GenerateOptions {
                    temperature: Some(0.2),
                    num_predict: Some(800),
                    num_ctx: Some(8192),
                }),
                Some(ai_cache::DEFAULT_CACHE_TTL_SECONDS),
                "doc-qa",
            )
            .await
            {
                Ok(response) => QaAnswer {
                    question: question.to_string(),
                    answer: response.response.trim().to_string(),
                    citations: citations_from_hits(&hits),
                    language: answer_language,
                    confidence: estimate_confidence(&hits, &analysis),
                    mode: AnswerMode::Generated,
                    follow_up_suggestions: follow_up_suggestions(&question, &analysis),
                    latency_ms: started.elapsed().as_millis(),
                },
                Err(_) => extractive_answer(question, &hits, answer_language, started),
            }
        } else {
            extractive_answer(question, &hits, answer_language, started)
        };

        // Persist the exchange so follow-up questions work.
        if let Some(id) = session_id {
            if let Some(mut session) = self.store.get(id) {
                session.messages.push(QaMessage {
                    role: QaMessageRole::User,
                    content: question.to_string(),
                    timestamp: Utc::now(),
                });
                session.messages.push(QaMessage {
                    role: QaMessageRole::Assistant,
                    content: answer.answer.clone(),
                    timestamp: Utc::now(),
                });
                if session.messages.len() > 40 {
                    session.messages = session.messages[..40].to_vec();
                }
                session.last_updated = Utc::now();
                self.store.update(&session);
                self.store.save();
            }
        }

        Ok(answer)
    }

    /// Create a new follow-up session.
    pub fn create_session(&mut self, language: QaLanguage) -> QaSession {
        let session = self.store.create(language);
        self.store.save();
        session
    }
}

/// Build the LLM prompt with retrieved context and conversation history.
fn build_qa_prompt(
    question: &str,
    context: &str,
    language: QaLanguage,
    session: Option<QaSession>,
    intent: QuestionIntent,
) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "You are StarForge's documentation assistant, answering questions about StarForge, the \
Stellar network, and Soroban smart contracts.\n",
    );
    prompt.push_str(&format!("Respond in {}.\n", language.display()));
    prompt.push_str(&format!(
        "The question is a \"{}\" type question. Answer it directly and practically.\n\n",
        intent.as_str()
    ));

    if let Some(session) = session {
        let recent: Vec<&QaMessage> = session
            .messages
            .iter()
            .filter(|m| m.role != QaMessageRole::System)
            .rev()
            .take(8)
            .collect();
        if !recent.is_empty() {
            prompt.push_str("Conversation history (for follow-up questions):\n");
            for msg in recent.iter().rev() {
                let role = match msg.role {
                    QaMessageRole::User => "User",
                    QaMessageRole::Assistant => "Assistant",
                    QaMessageRole::System => "System",
                };
                prompt.push_str(&format!("{}: {}\n", role, msg.content));
            }
            prompt.push('\n');
        }
    }

    if context.is_empty() {
        prompt.push_str(
            "No documentation context was found for this question. Answer from general \
Stellar/Soroban knowledge and state that you could not find specific documentation.\n\n",
        );
    } else {
        prompt.push_str(
            "Use ONLY the following documentation context to answer. Cite the relevant numbered \
source inline like [1], [2], etc. If the context does not contain the answer, say so clearly.\n\n",
        );
        prompt.push_str(&format!("Documentation context:\n{}\n\n", context));
    }

    prompt.push_str(&format!("Question: {}\n", question));
    prompt.push_str("\nAnswer (with inline citations like [1]):\n");
    prompt
}

/// Build citations from the retrieved hits used in the prompt.
fn citations_from_hits(hits: &[SearchHit]) -> Vec<Citation> {
    hits.iter()
        .map(|hit| Citation {
            source: hit.chunk.source.clone(),
            title: hit.chunk.title.clone(),
            url: hit.chunk.url.clone(),
            kind: hit.chunk.kind,
            snippet: truncate_snippet(&hit.chunk.content),
        })
        .collect()
}

/// Extractive fallback answer used when the LLM is unavailable.
fn extractive_answer(
    question: &str,
    hits: &[SearchHit],
    language: QaLanguage,
    started: std::time::Instant,
) -> QaAnswer {
    let analysis = analyze_question(question);
    let answer = if hits.is_empty() {
        "No relevant documentation was found for this question. Try rephrasing it, or index more \
documentation with `starforge ai-doc-qa index --dir <path>`."
            .to_string()
    } else {
        let mut out = String::new();
        out.push_str("Based on the documentation, the most relevant excerpts are:\n\n");
        for (i, hit) in hits.iter().take(3).enumerate() {
            out.push_str(&format!(
                "{}: {}\n\n",
                i + 1,
                truncate_snippet(&hit.chunk.content)
            ));
        }
        out.push_str(
            "This is an extractive answer because the local LLM is not available. Start Ollama \
for full generated answers.",
        );
        out
    };
    QaAnswer {
        question: question.to_string(),
        answer,
        citations: citations_from_hits(hits),
        language,
        confidence: estimate_confidence(hits, &analysis),
        mode: AnswerMode::Extractive,
        follow_up_suggestions: follow_up_suggestions(question, &analysis),
        latency_ms: started.elapsed().as_millis(),
    }
}

/// Heuristic confidence based on retrieval scores and topic overlap.
fn estimate_confidence(hits: &[SearchHit], analysis: &QuestionAnalysis) -> f64 {
    if hits.is_empty() {
        return 0.0;
    }
    let max_score = hits.iter().map(|h| h.score).fold(0.0, f64::max);
    let hit_score = (max_score / 8.0).clamp(0.0, 0.9);
    let topic_overlap = hits
        .iter()
        .filter(|h| {
            analysis
                .topics
                .iter()
                .any(|t| h.chunk.content.to_lowercase().contains(t))
        })
        .count() as f64
        / hits.len() as f64;
    ((hit_score * 0.6) + (topic_overlap * 0.4) * 0.6).clamp(0.0, 0.98)
}

/// Suggest follow-up questions based on the current one.
fn follow_up_suggestions(question: &str, analysis: &QuestionAnalysis) -> Vec<String> {
    let mut suggestions = Vec::new();
    let lower = question.to_lowercase();
    for topic in &analysis.topics {
        match topic.as_str() {
            "deployment" => {
                suggestions.push("How do I deploy on mainnet?".to_string());
                suggestions.push("What are the deployment prerequisites?".to_string());
            }
            "wallet" => suggestions.push("How do I fund my wallet?".to_string()),
            "gas" => suggestions.push("How can I reduce gas costs?".to_string()),
            "authentication" => {
                suggestions.push("How does Soroban authorization work?".to_string())
            }
            "token" => suggestions.push("How do I mint a custom token?".to_string()),
            "storage" => suggestions.push("How does storage TTL work?".to_string()),
            "testing" => suggestions.push("How do I write contract tests?".to_string()),
            "security" => suggestions.push("What security checks should I run?".to_string()),
            "network" => {
                suggestions.push("What is the difference between testnet and mainnet?".to_string())
            }
            _ => {}
        }
        if suggestions.len() >= 2 {
            break;
        }
    }
    if lower.contains("testnet") {
        suggestions.push("What about on mainnet?".to_string());
    } else if lower.contains("mainnet") {
        suggestions.push("How do I test before deploying to mainnet?".to_string());
    }
    if suggestions.len() < 2 {
        suggestions.push("Where can I find more StarForge documentation?".to_string());
    }
    suggestions.truncate(3);
    suggestions
}

/// Truncate a snippet for citation display.
fn truncate_snippet(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.len() <= 220 {
        trimmed.to_string()
    } else {
        let mut end = 220;
        if let Some(rel) = trimmed[..end].rfind(' ') {
            end = rel;
        }
        format!("{}...", &trimmed[..end])
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(
            QaLanguage::detect("How do I deploy a contract?"),
            QaLanguage::English
        );
        assert_eq!(QaLanguage::detect("如何部署合约？"), QaLanguage::Chinese);
        assert_eq!(
            QaLanguage::detect("¿Cómo despliego un contrato?"),
            QaLanguage::Spanish
        );
        assert_eq!(
            QaLanguage::detect("Как развернуть контракт?"),
            QaLanguage::Russian
        );
        assert_eq!(QaLanguage::detect("كيف أنشر عقداً؟"), QaLanguage::Arabic);
    }

    #[test]
    fn test_language_parse() {
        assert_eq!(QaLanguage::parse("es"), Some(QaLanguage::Spanish));
        assert_eq!(QaLanguage::parse("Japanese"), Some(QaLanguage::Japanese));
        assert_eq!(QaLanguage::parse("xx"), None);
        assert_eq!(QaLanguage::all().len(), 10);
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("How do I deploy a Soroban contract on testnet?");
        assert!(tokens.contains(&"deploy".to_string()));
        assert!(tokens.contains(&"soroban".to_string()));
        assert!(!tokens.contains(&"how".to_string()));
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn test_question_analysis() {
        let analysis = analyze_question("How do I deploy a contract on testnet?");
        assert_eq!(analysis.intent, QuestionIntent::HowTo);
        assert!(analysis.topics.contains(&"deployment".to_string()));
        assert_eq!(analysis.language, QaLanguage::English);

        let err = analyze_question("Why is my transaction failing with a sequence number error?");
        assert_eq!(err.intent, QuestionIntent::Troubleshooting);
    }

    #[test]
    fn test_builtin_knowledge_base() {
        let chunks = builtin_knowledge_base();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.url.is_some()));
        assert!(chunks.iter().any(|c| c.kind == SourceKind::Stellar));
        assert!(chunks.iter().any(|c| c.kind == SourceKind::SorobanSdk));
        assert!(chunks.iter().any(|c| c.kind == SourceKind::BestPractices));
    }

    #[test]
    fn test_retrieval_finds_relevant_chunk() {
        let index = DocIndex::new();
        let chunks = builtin_knowledge_base();
        let index = DocIndex {
            chunks,
            built_at: Utc::now(),
        };
        let tokens = tokenize("How do I deploy a contract?");
        let hits = index.retrieve(&tokens, 3, 1.0);
        assert!(!hits.is_empty());
        assert!(hits[0].score > 0.0);
        // Deployment-related chunks should be ranked first.
        assert!(hits[0].chunk.content.to_lowercase().contains("deploy"));
    }

    #[test]
    fn test_chunking() {
        let text = "Paragraph one about deployment.\n\nParagraph two about wallets.\n\nParagraph three about gas.";
        let chunks = chunk_text(text, 40, 5);
        assert!(chunks.len() >= 2);
        let all_joined = chunks.join(" ");
        assert!(all_joined.contains("deployment"));
    }

    #[test]
    fn test_index_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("sample.md");
        fs::write(
            &file,
            "# Gas\n\nOptimizing gas costs is important for Soroban contracts.",
        )
        .unwrap();
        let options = IndexOptions::default();
        let chunks = index_file(&file, SourceKind::StarForge, &options).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].title, "sample");
        assert_eq!(chunks[0].kind, SourceKind::StarForge);
    }

    #[test]
    fn test_session_store() {
        let mut store = QaSessionStore::default();
        let session = store.create(QaLanguage::Spanish);
        assert_eq!(session.preferred_language, QaLanguage::Spanish);
        let sessions = store.list();
        assert_eq!(sessions.len(), 1);
        assert!(store.delete(&session.session_id));
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_follow_up_suggestions() {
        let analysis = analyze_question("How do I deploy?");
        let suggestions = follow_up_suggestions("How do I deploy?", &analysis);
        assert!(!suggestions.is_empty());
        assert!(suggestions[0].to_lowercase().contains("deploy"));
    }

    #[test]
    fn test_confidence_bounds() {
        let analysis = analyze_question("What is Soroban?");
        assert!((0.0..=1.0).contains(&estimate_confidence(&[], &analysis)));
    }
}
