//! AI-assisted documentation generation for Soroban contracts.
//!
//! Builds comprehensive Markdown documentation from Rust source by combining
//! rustdoc comment extraction with heuristic enrichment for architecture,
//! storage layout, security considerations, and multi-language usage examples.
//!
//! When `STARFORGE_AI_API_KEY` is set, prose sections can optionally be refined
//! via an OpenAI-compatible chat completions endpoint.

use crate::utils::doc_generator::{DocCommentExtractor, ExtractedDocs, ExtractedFn, Visibility};
use crate::utils::docs::{DocEntry, DocSection, EventDoc, FunctionDoc, ParamDoc, StorageDoc};
use crate::utils::http_client;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Languages supported for usage-guide examples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocLanguage {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl DocLanguage {
    pub fn parse_list(raw: &str) -> Vec<Self> {
        raw.split(',')
            .filter_map(|part| match part.trim().to_ascii_lowercase().as_str() {
                "rust" | "rs" => Some(Self::Rust),
                "typescript" | "ts" | "js" | "javascript" => Some(Self::TypeScript),
                "python" | "py" => Some(Self::Python),
                "go" | "golang" => Some(Self::Go),
                _ => None,
            })
            .collect()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
        }
    }
}

/// Options for AI documentation generation.
#[derive(Debug, Clone)]
pub struct AiDocsOptions {
    pub contract_id: String,
    pub name: String,
    pub description: Option<String>,
    pub network: String,
    pub version: String,
    pub languages: Vec<DocLanguage>,
    /// When true, attempt optional LLM enrichment if an API key is configured.
    pub use_llm: bool,
}

impl Default for AiDocsOptions {
    fn default() -> Self {
        Self {
            contract_id: "contract".to_string(),
            name: "Contract".to_string(),
            description: None,
            network: "testnet".to_string(),
            version: "1.0.0".to_string(),
            languages: vec![
                DocLanguage::Rust,
                DocLanguage::TypeScript,
                DocLanguage::Python,
            ],
            use_llm: false,
        }
    }
}

/// Full result of AI documentation generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiGeneratedDocs {
    pub entry: DocEntry,
    pub markdown: String,
    pub rustdoc_stubs: String,
    pub enrichment_mode: String,
}

/// Generate comprehensive documentation from a Soroban contract source file.
pub fn generate_from_source(source: &Path, options: &AiDocsOptions) -> Result<AiGeneratedDocs> {
    let extracted = DocCommentExtractor::extract_from_file(source)?;
    let source_text = fs::read_to_string(source)
        .with_context(|| format!("Failed to read {}", source.display()))?;
    generate_from_extracted(&extracted, &source_text, options)
}

/// Generate documentation from already-extracted rustdoc data plus raw source.
pub fn generate_from_extracted(
    extracted: &ExtractedDocs,
    source_text: &str,
    options: &AiDocsOptions,
) -> Result<AiGeneratedDocs> {
    let description = options
        .description
        .clone()
        .or_else(|| first_sentence(&extracted.module_doc))
        .unwrap_or_else(|| {
            format!(
                "Soroban smart contract `{}` with {} public function(s).",
                options.name,
                public_functions(extracted).len()
            )
        });

    let mut functions = build_function_docs(extracted, &options.languages);
    let events = infer_events(source_text, extracted);
    let storage = infer_storage_layout(source_text, extracted);
    let mut sections = build_sections(
        extracted,
        source_text,
        &options.name,
        &options.network,
        &options.version,
        &description,
        &storage,
        &options.languages,
    );

    let mut enrichment_mode = "heuristic".to_string();
    if options.use_llm {
        if let Ok(Some(refined)) = try_llm_enrichment(extracted, &description, &functions) {
            if let Some(overview) = refined.architecture {
                if let Some(section) = sections.iter_mut().find(|s| s.title == "Architecture") {
                    section.content = overview;
                }
            }
            if let Some(security) = refined.security {
                if let Some(section) = sections
                    .iter_mut()
                    .find(|s| s.title == "Security Considerations")
                {
                    section.content = security;
                }
            }
            for enriched_fn in refined.functions {
                if let Some(func) = functions.iter_mut().find(|f| f.name == enriched_fn.name) {
                    if !enriched_fn.description.trim().is_empty() {
                        func.description = enriched_fn.description;
                    }
                    if !enriched_fn.examples.is_empty() {
                        func.examples = enriched_fn.examples;
                    }
                }
            }
            enrichment_mode = "llm".to_string();
        }
    }

    let entry = DocEntry {
        contract_id: options.contract_id.clone(),
        name: options.name.clone(),
        description: description.clone(),
        version: options.version.clone(),
        network: options.network.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        sections: sections.clone(),
        api: crate::utils::docs::ApiDocumentation {
            functions: functions.clone(),
            events: events.clone(),
            storage: storage.clone(),
        },
    };

    let markdown = render_comprehensive_markdown(&entry, extracted, &options.languages);
    let rustdoc_stubs = render_rustdoc_stubs(extracted);

    Ok(AiGeneratedDocs {
        entry,
        markdown,
        rustdoc_stubs,
        enrichment_mode,
    })
}

/// Persist generated docs into the docs store and optionally write Markdown/rustdoc files.
pub fn persist_generated(
    generated: &AiGeneratedDocs,
    markdown_out: Option<&Path>,
    rustdoc_out: Option<&Path>,
) -> Result<DocEntry> {
    let entry = crate::utils::docs::generate_documentation(
        &generated.entry.contract_id,
        &generated.entry.name,
        &generated.entry.description,
        &generated.entry.network,
        &generated.entry.version,
        generated.entry.api.functions.clone(),
        generated.entry.api.events.clone(),
        generated.entry.api.storage.clone(),
        generated.entry.sections.clone(),
    )?;

    if let Some(path) = markdown_out {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &generated.markdown)
            .with_context(|| format!("Failed to write markdown to {}", path.display()))?;
    }

    if let Some(path) = rustdoc_out {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &generated.rustdoc_stubs)
            .with_context(|| format!("Failed to write rustdoc stubs to {}", path.display()))?;
    }

    Ok(entry)
}

fn public_functions(extracted: &ExtractedDocs) -> Vec<&ExtractedFn> {
    extracted
        .functions
        .iter()
        .filter(|f| f.visibility == Visibility::Public)
        .collect()
}

fn build_function_docs(extracted: &ExtractedDocs, languages: &[DocLanguage]) -> Vec<FunctionDoc> {
    public_functions(extracted)
        .into_iter()
        .map(|f| {
            let description = if f.doc_comment.trim().is_empty() {
                infer_function_description(f)
            } else {
                first_paragraph(&f.doc_comment)
            };

            let parameters = f
                .params
                .iter()
                .filter(|p| p.name != "env" && p.name != "self")
                .map(|p| ParamDoc {
                    name: p.name.clone(),
                    ty: p.ty.clone(),
                    description: infer_param_description(&p.name, &p.ty),
                    required: true,
                })
                .collect::<Vec<_>>();

            let mut examples = f
                .examples
                .iter()
                .map(|ex| {
                    if ex.lang.is_empty() {
                        ex.code.clone()
                    } else {
                        format!("// lang: {}\n{}", ex.lang, ex.code)
                    }
                })
                .collect::<Vec<_>>();

            if examples.is_empty() {
                examples.extend(generate_usage_examples(&f.name, &parameters, languages));
            }

            FunctionDoc {
                name: f.name.clone(),
                description,
                parameters,
                returns: f.return_type.clone(),
                examples,
            }
        })
        .collect()
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn build_sections(
    extracted: &ExtractedDocs,
    source_text: &str,
    name: &str,
    network: &str,
    version: &str,
    description: &str,
    storage: &[StorageDoc],
    languages: &[DocLanguage],
) -> Vec<DocSection> {
    let mut sections = Vec::new();

    sections.push(DocSection {
        title: "Overview".to_string(),
        content: format!(
            "{}\n\n`{}` is a Soroban contract intended for the `{}` network. \
             Documentation was generated from rustdoc comments and source analysis.",
            description, name, network
        ),
        order: 0,
    });

    sections.push(DocSection {
        title: "Architecture".to_string(),
        content: explain_architecture(extracted, source_text, name),
        order: 1,
    });

    if !extracted.structs.is_empty() || !extracted.enums.is_empty() {
        sections.push(DocSection {
            title: "Types".to_string(),
            content: document_types(extracted),
            order: 2,
        });
    }

    sections.push(DocSection {
        title: "Storage Layout".to_string(),
        content: document_storage_section(storage, source_text),
        order: 3,
    });

    sections.push(DocSection {
        title: "Configuration Reference".to_string(),
        content: document_configuration(extracted, network, version),
        order: 4,
    });

    sections.push(DocSection {
        title: "Security Considerations".to_string(),
        content: analyze_security(source_text, extracted),
        order: 5,
    });

    sections.push(DocSection {
        title: "Usage Guides".to_string(),
        content: build_usage_guides(extracted, languages),
        order: 6,
    });

    sections.push(DocSection {
        title: "Getting Started".to_string(),
        content: format!(
            "1. Build the contract WASM with `cargo build --target wasm32v1-none --release`.\n\
             2. Deploy with `starforge deploy --wasm <path> --network {network}`.\n\
             3. Call public entrypoints via `starforge contract invoke` or generated bindings.\n\
             4. Keep rustdoc comments (`///`, `//!`) in sync — re-run `starforge docs generate --source` after API changes."
        ),
        order: 7,
    });

    sections.push(DocSection {
        title: "Troubleshooting".to_string(),
        content: build_troubleshooting(source_text, extracted, storage),
        order: 8,
    });

    sections
}

/// Documents contract-level constants, the target network/version, and any
/// environment configuration a deployer needs to know about.
fn document_configuration(extracted: &ExtractedDocs, network: &str, version: &str) -> String {
    let mut out = format!(
        "| Setting | Value |\n|---|---|\n| Network | `{}` |\n| Version | `{}` |\n",
        network, version
    );

    if extracted.constants.is_empty() {
        out.push_str(
            "\nNo module-level constants were found in source. Configuration is driven \
             entirely by constructor/`initialize` arguments at deploy time.\n",
        );
    } else {
        out.push_str(
            "\n### Constants\n\n| Name | Type | Value | Description |\n|---|---|---|---|\n",
        );
        for c in &extracted.constants {
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` | {} |\n",
                c.name,
                c.ty,
                c.value,
                first_paragraph(&c.doc_comment)
            ));
        }
    }

    out
}

/// Heuristic troubleshooting guide covering the most common failure modes
/// for the detected contract shape (auth, storage, arithmetic).
fn build_troubleshooting(
    source: &str,
    extracted: &ExtractedDocs,
    storage: &[StorageDoc],
) -> String {
    let mut tips: Vec<String> = Vec::new();

    if source.contains("require_auth") {
        tips.push(
            "**`Error(Auth, InvalidAction)`** — the transaction was not signed by the address \
             passed to a `require_auth()` call. Make sure the invoking key matches the address \
             argument exactly."
                .to_string(),
        );
    }

    if !storage.is_empty() {
        tips.push(
            "**Reads return `None`/default values** — storage is only populated after the \
             relevant `initialize`/setter function has been invoked at least once. Confirm \
             the contract was initialized on the network you're querying."
                .to_string(),
        );
    }

    if source.contains(".unwrap()") || source.contains(".expect(") {
        tips.push(
            "**Host panics with a generic `UnreachableCodeReached`/`Error(Contract, #0)`** — \
             one of the contract's `unwrap()`/`expect()` calls hit an unexpected `None`/`Err`. \
             Check the preconditions of the function you called (e.g. an account must exist, \
             a balance must be sufficient)."
                .to_string(),
        );
    }

    if extracted.functions.iter().any(|f| {
        f.params
            .iter()
            .any(|p| p.ty.contains("i128") || p.ty.contains("u32") || p.ty.contains("u64"))
    }) {
        tips.push(
            "**`Error(Contract, #...)` on large amounts** — numeric parameters use fixed-width \
             integer types; verify inputs stay within range before calling to avoid overflow \
             panics."
                .to_string(),
        );
    }

    tips.push(
        "**`HostError: not found`** — the contract ID or network passed to \
         `starforge contract invoke` doesn't match where the contract was deployed. Re-check \
         with `starforge deployments list`."
            .to_string(),
    );
    tips.push(
        "**Build fails on `wasm32v1-none`** — ensure the target is installed with \
         `rustup target add wasm32v1-none` and that `#![no_std]` is present for on-chain builds."
            .to_string(),
    );

    tips.join("\n\n")
}

fn explain_architecture(extracted: &ExtractedDocs, source: &str, name: &str) -> String {
    let public_fns = public_functions(extracted);
    let mut parts = Vec::new();

    parts.push(format!(
        "`{}` exposes **{}** public function(s)",
        name,
        public_fns.len()
    ));
    if !extracted.structs.is_empty() {
        parts.push(format!("**{}** struct type(s)", extracted.structs.len()));
    }
    if !extracted.enums.is_empty() {
        parts.push(format!("**{}** enum type(s)", extracted.enums.len()));
    }

    let mut body = format!("{}.\n\n", parts.join(", "));

    if source.contains("#[contractimpl]") || source.contains("#[contract]") {
        body.push_str(
            "The contract follows the standard Soroban `#[contract]` / `#[contractimpl]` pattern.\n\n",
        );
    }

    let storage_kinds = [
        ("instance()", "instance storage (TTL-bound contract state)"),
        (
            "persistent()",
            "persistent storage (long-lived ledger entries)",
        ),
        ("temporary()", "temporary storage (short-lived cache)"),
    ];
    let used: Vec<&str> = storage_kinds
        .iter()
        .filter(|(needle, _)| source.contains(needle))
        .map(|(_, label)| *label)
        .collect();
    if !used.is_empty() {
        body.push_str("**Storage tiers in use:**\n");
        for label in used {
            body.push_str(&format!("- {}\n", label));
        }
        body.push('\n');
    }

    if !public_fns.is_empty() {
        body.push_str("**Public entrypoints:**\n");
        for func in &public_fns {
            let summary = if func.doc_comment.trim().is_empty() {
                infer_function_description(func)
            } else {
                first_sentence(&func.doc_comment)
                    .unwrap_or_else(|| infer_function_description(func))
            };
            body.push_str(&format!("- `{}` — {}\n", func.name, summary));
        }
    }

    if !extracted.module_doc.trim().is_empty() {
        body.push_str("\n**Module documentation (from rustdoc):**\n\n");
        body.push_str(extracted.module_doc.trim());
        body.push('\n');
    }

    body
}

fn document_types(extracted: &ExtractedDocs) -> String {
    let mut md = String::new();

    for s in &extracted.structs {
        md.push_str(&format!("### `{}`\n\n", s.name));
        if s.doc_comment.trim().is_empty() {
            md.push_str(&format!(
                "Struct used by the contract API (`{}`).\n\n",
                s.name
            ));
        } else {
            md.push_str(&format!("{}\n\n", s.doc_comment.trim()));
        }
        if !s.fields.is_empty() {
            md.push_str("| Field | Type | Description |\n| --- | --- | --- |\n");
            for field in &s.fields {
                let desc = if field.doc_comment.trim().is_empty() {
                    infer_param_description(&field.name, &field.ty)
                } else {
                    field.doc_comment.trim().to_string()
                };
                md.push_str(&format!(
                    "| `{}` | `{}` | {} |\n",
                    field.name, field.ty, desc
                ));
            }
            md.push('\n');
        }
    }

    for e in &extracted.enums {
        md.push_str(&format!("### `{}`\n\n", e.name));
        if e.doc_comment.trim().is_empty() {
            md.push_str(&format!(
                "Enumeration used by the contract API (`{}`).\n\n",
                e.name
            ));
        } else {
            md.push_str(&format!("{}\n\n", e.doc_comment.trim()));
        }
        if !e.variants.is_empty() {
            md.push_str("**Variants:**\n\n");
            for variant in &e.variants {
                let desc = if variant.doc_comment.trim().is_empty() {
                    format!("`{}` variant", variant.name)
                } else {
                    variant.doc_comment.trim().to_string()
                };
                md.push_str(&format!("- `{}` — {}\n", variant.name, desc));
            }
            md.push('\n');
        }
    }

    md
}

fn infer_storage_layout(source: &str, extracted: &ExtractedDocs) -> Vec<StorageDoc> {
    let mut storage = Vec::new();

    // Prefer explicit StorageKey / DataKey enums.
    for e in &extracted.enums {
        let lower = e.name.to_ascii_lowercase();
        if lower.contains("storage") || lower.contains("datakey") || lower == "key" {
            for variant in &e.variants {
                storage.push(StorageDoc {
                    key: variant.name.clone(),
                    ty: e.name.clone(),
                    description: if variant.doc_comment.trim().is_empty() {
                        format!("Storage key variant of `{}`", e.name)
                    } else {
                        variant.doc_comment.trim().to_string()
                    },
                });
            }
        }
    }

    // Symbol constants used as keys: `const FOO: Symbol = symbol_short!("FOO");`
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("const ") && trimmed.contains("Symbol") {
            if let Some(name) = trimmed
                .strip_prefix("const ")
                .and_then(|rest| rest.split(':').next())
                .map(str::trim)
            {
                if !storage.iter().any(|s| s.key == name) {
                    storage.push(StorageDoc {
                        key: name.to_string(),
                        ty: "Symbol".to_string(),
                        description: format!("Ledger key constant `{}`", name),
                    });
                }
            }
        }
    }

    // Heuristic: common storage API usage mentions.
    let heuristics = [
        (
            "balances",
            "Map<Address, i128>",
            "Token or account balances",
        ),
        ("admin", "Address", "Contract administrator address"),
        ("owner", "Address", "Asset or resource owner"),
        (
            "allowance",
            "Map<(Address, Address), i128>",
            "Spend allowances",
        ),
        ("total_supply", "i128", "Total token supply"),
    ];
    for (key, ty, desc) in heuristics {
        if source.contains(key) && !storage.iter().any(|s| s.key.eq_ignore_ascii_case(key)) {
            // Only include if it looks like a storage key / field, not a random comment hit.
            if source.contains(&format!("\"{}\"", key))
                || source.contains(&format!("symbol_short!(\"{}\"", key.to_ascii_uppercase()))
                || source.contains(&format!("{}:", key))
                || source.contains(&format!("{} =", key))
            {
                storage.push(StorageDoc {
                    key: key.to_string(),
                    ty: ty.to_string(),
                    description: desc.to_string(),
                });
            }
        }
    }

    storage
}

fn document_storage_section(storage: &[StorageDoc], source: &str) -> String {
    let mut md = String::new();
    if storage.is_empty() {
        md.push_str(
            "No explicit storage key enum or symbol constants were detected. \
             Review `env.storage()` usage in the source for ledger layout details.\n",
        );
    } else {
        md.push_str("| Key | Type | Description |\n| --- | --- | --- |\n");
        for item in storage {
            md.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                item.key, item.ty, item.description
            ));
        }
        md.push('\n');
    }

    let tiers = [
        ("instance()", "Instance"),
        ("persistent()", "Persistent"),
        ("temporary()", "Temporary"),
    ];
    let used: Vec<&str> = tiers
        .iter()
        .filter(|(needle, _)| source.contains(needle))
        .map(|(_, label)| *label)
        .collect();
    if !used.is_empty() {
        md.push_str(&format!("**Active storage tiers:** {}\n", used.join(", ")));
    }
    md
}

fn analyze_security(source: &str, extracted: &ExtractedDocs) -> String {
    let mut notes = Vec::new();

    let has_auth = source.contains("require_auth") || source.contains("require_auth_for_args");
    if has_auth {
        notes.push(
            "Authorization checks (`require_auth`) are present — verify every state-mutating \
             entrypoint authenticates the correct address."
                .to_string(),
        );
    } else {
        notes.push(
            "**Warning:** No `require_auth` calls were detected. State-changing public functions \
             may be callable by anyone unless authorization is enforced elsewhere."
                .to_string(),
        );
    }

    let mutating = [
        "transfer",
        "mint",
        "burn",
        "withdraw",
        "upgrade",
        "set_admin",
        "reset",
    ];
    for func in public_functions(extracted) {
        if mutating.iter().any(|m| func.name.contains(m)) {
            notes.push(format!(
                "Entrypoint `{}` looks state-mutating — confirm caller authorization and input validation.",
                func.name
            ));
        }
    }

    if source.contains("upgrade") || source.contains("set_admin") {
        notes.push(
            "Admin / upgrade paths detected — restrict to a trusted admin and document the upgrade process."
                .to_string(),
        );
    }

    if source.contains("i128") || source.contains("u128") {
        notes.push(
            "Large integer types are used — prefer checked arithmetic for amounts to avoid overflow/underflow."
                .to_string(),
        );
    }

    if source.contains("persistent()") {
        notes.push(
            "Persistent storage is used — ensure TTL/bump policies keep critical ledger entries alive."
                .to_string(),
        );
    }

    notes.push(
        "Re-run `starforge security audit` before mainnet deployment for a fuller vulnerability scan."
            .to_string(),
    );

    notes
        .into_iter()
        .enumerate()
        .map(|(i, n)| format!("{}. {}", i + 1, n))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_usage_guides(extracted: &ExtractedDocs, languages: &[DocLanguage]) -> String {
    let mut md = String::new();
    let funcs = public_functions(extracted);
    if funcs.is_empty() {
        return "No public functions found to generate usage guides.".to_string();
    }

    let primary = funcs[0];
    let params: Vec<ParamDoc> = primary
        .params
        .iter()
        .filter(|p| p.name != "env" && p.name != "self")
        .map(|p| ParamDoc {
            name: p.name.clone(),
            ty: p.ty.clone(),
            description: String::new(),
            required: true,
        })
        .collect();

    for lang in languages {
        md.push_str(&format!("### {}\n\n", lang.as_str()));
        for example in generate_usage_examples(&primary.name, &params, &[*lang]) {
            md.push_str(&format!("```{}\n{}\n```\n\n", lang.as_str(), example));
        }
    }
    md
}

fn generate_usage_examples(
    fn_name: &str,
    params: &[ParamDoc],
    languages: &[DocLanguage],
) -> Vec<String> {
    languages
        .iter()
        .map(|lang| match lang {
            DocLanguage::Rust => {
                let args = params
                    .iter()
                    .map(|p| format!("&{}", sample_value_rust(&p.name, &p.ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "// Rust (Soroban client)\nlet result = client.{}({});",
                    fn_name, args
                )
            }
            DocLanguage::TypeScript => {
                let args = params
                    .iter()
                    .map(|p| sample_value_ts(&p.name, &p.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "// TypeScript\nconst result = await contract.{}({});",
                    to_camel(fn_name),
                    args
                )
            }
            DocLanguage::Python => {
                let args = params
                    .iter()
                    .map(|p| sample_value_py(&p.name, &p.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("# Python\nresult = contract.{}({})", fn_name, args)
            }
            DocLanguage::Go => {
                let args = params
                    .iter()
                    .map(|p| sample_value_go(&p.name, &p.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "// Go\nresult, err := client.{}({})",
                    to_pascal(fn_name),
                    args
                )
            }
        })
        .collect()
}

fn infer_events(source: &str, extracted: &ExtractedDocs) -> Vec<EventDoc> {
    let mut events = Vec::new();

    // Events often appear as structs ending in Event, or publish calls.
    for s in &extracted.structs {
        if s.name.ends_with("Event") || s.name.ends_with("Evt") {
            events.push(EventDoc {
                name: s.name.clone(),
                description: if s.doc_comment.trim().is_empty() {
                    format!("Contract event emitted as `{}`", s.name)
                } else {
                    first_paragraph(&s.doc_comment)
                },
                topics: s
                    .fields
                    .iter()
                    .map(|f| crate::utils::docs::TopicDoc {
                        name: f.name.clone(),
                        ty: f.ty.clone(),
                        description: if f.doc_comment.trim().is_empty() {
                            infer_param_description(&f.name, &f.ty)
                        } else {
                            f.doc_comment.trim().to_string()
                        },
                    })
                    .collect(),
            });
        }
    }

    if events.is_empty() && source.contains(".publish(") {
        events.push(EventDoc {
            name: "ContractEvent".to_string(),
            description: "One or more events are published via `env.events().publish(...)`."
                .to_string(),
            topics: vec![],
        });
    }

    events
}

fn infer_function_description(func: &ExtractedFn) -> String {
    let name = func.name.as_str();
    let params: Vec<String> = func
        .params
        .iter()
        .filter(|p| p.name != "env" && p.name != "self")
        .map(|p| format!("`{}: {}`", p.name, p.ty))
        .collect();

    let base = match name {
        "initialize" | "init" => "Initialize the contract state".to_string(),
        "transfer" => "Transfer value between accounts".to_string(),
        "mint" => "Mint new tokens or assets".to_string(),
        "burn" => "Burn tokens or assets".to_string(),
        "balance" | "get_balance" => "Read an account balance".to_string(),
        "approve" => "Approve a spender allowance".to_string(),
        "upgrade" => "Upgrade the contract WASM".to_string(),
        "reset" => "Reset contract state".to_string(),
        "increment" => "Increment a stored counter".to_string(),
        "get_count" | "count" => "Return the current counter value".to_string(),
        other if other.starts_with("get_") || other.starts_with("read_") => {
            format!(
                "Read `{}` from contract storage",
                other.trim_start_matches("get_").trim_start_matches("read_")
            )
        }
        other if other.starts_with("set_") => {
            format!(
                "Update `{}` in contract storage",
                other.trim_start_matches("set_")
            )
        }
        other => format!("Invoke the `{}` contract entrypoint", other),
    };

    if params.is_empty() {
        format!("{}.", base)
    } else {
        format!("{} (parameters: {}).", base, params.join(", "))
    }
}

fn infer_param_description(name: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    match name {
        "admin" => "Administrator address".to_string(),
        "from" | "sender" => "Source address".to_string(),
        "to" | "recipient" => "Destination address".to_string(),
        "amount" => "Amount to transfer or update".to_string(),
        "value" if t.contains("i128") || t.contains("u128") => {
            "Numeric amount or balance value".to_string()
        }
        "value" => "Associated value".to_string(),
        "spender" => "Address allowed to spend on behalf of the owner".to_string(),
        "owner" => "Token or resource owner address".to_string(),
        "id" | "token_id" => "Identifier of the resource".to_string(),
        other => format!("Parameter `{}` of type `{}`", other, ty),
    }
}

fn render_comprehensive_markdown(
    entry: &DocEntry,
    extracted: &ExtractedDocs,
    _languages: &[DocLanguage],
) -> String {
    let mut md = String::new();
    md.push_str(&format!("# {} Documentation\n\n", entry.name));
    md.push_str(&format!("**Contract:** `{}`  \n", entry.contract_id));
    md.push_str(&format!("**Network:** {}  \n", entry.network));
    md.push_str(&format!("**Version:** {}  \n", entry.version));
    md.push_str(&format!(
        "**Generated:** {}  \n\n",
        &entry.generated_at[..10]
    ));
    md.push_str(&format!("{}\n\n", entry.description));

    let mut sections = entry.sections.clone();
    sections.sort_by_key(|s| s.order);
    for section in &sections {
        md.push_str(&format!("## {}\n\n{}\n\n", section.title, section.content));
    }

    md.push_str("## API Reference\n\n");
    md.push_str("### Functions\n\n");
    for func in &entry.api.functions {
        md.push_str(&format!("#### `{}`\n\n", func.name));
        md.push_str(&format!("{}\n\n", func.description));
        if !func.parameters.is_empty() {
            md.push_str("| Name | Type | Required | Description |\n| --- | --- | --- | --- |\n");
            for param in &func.parameters {
                md.push_str(&format!(
                    "| `{}` | `{}` | {} | {} |\n",
                    param.name,
                    param.ty,
                    if param.required { "yes" } else { "no" },
                    param.description
                ));
            }
            md.push('\n');
        }
        if let Some(ret) = &func.returns {
            md.push_str(&format!("**Returns:** `{}`\n\n", ret));
        }
        if !func.examples.is_empty() {
            md.push_str("**Examples:**\n\n");
            for example in &func.examples {
                let (lang, code) = split_lang_example(example);
                md.push_str(&format!("```{}\n{}\n```\n\n", lang, code));
            }
        }
    }

    if !entry.api.events.is_empty() {
        md.push_str("### Events\n\n");
        for event in &entry.api.events {
            md.push_str(&format!(
                "#### `{}`\n\n{}\n\n",
                event.name, event.description
            ));
            if !event.topics.is_empty() {
                md.push_str("| Topic | Type | Description |\n| --- | --- | --- |\n");
                for topic in &event.topics {
                    md.push_str(&format!(
                        "| `{}` | `{}` | {} |\n",
                        topic.name, topic.ty, topic.description
                    ));
                }
                md.push('\n');
            }
        }
    }

    if !entry.api.storage.is_empty() {
        md.push_str("### Storage\n\n");
        md.push_str("| Key | Type | Description |\n| --- | --- | --- |\n");
        for s in &entry.api.storage {
            md.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                s.key, s.ty, s.description
            ));
        }
        md.push('\n');
    }

    // Type appendix from rustdoc extraction (may duplicate Types section — keep concise).
    if (!extracted.structs.is_empty() || !extracted.enums.is_empty())
        && !entry.sections.iter().any(|s| s.title == "Types")
    {
        md.push_str("## Type Documentation\n\n");
        md.push_str(&document_types(extracted));
    }

    md.push_str("---\n\n");
    md.push_str("*Generated by StarForge AI Documentation Generation. Source of truth: rustdoc comments in contract source.*\n");
    md
}

/// Emit rustdoc-ready stub comments for functions missing documentation.
fn render_rustdoc_stubs(extracted: &ExtractedDocs) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated rustdoc stubs for undocumented items.\n");
    out.push_str("// Paste above the corresponding definitions, then refine.\n\n");

    if extracted.module_doc.trim().is_empty() {
        out.push_str("//! Soroban smart contract module.\n\n");
    }

    for func in public_functions(extracted) {
        if !func.doc_comment.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("/// {}\n", infer_function_description(func)));
        for param in func
            .params
            .iter()
            .filter(|p| p.name != "env" && p.name != "self")
        {
            out.push_str(&format!(
                "///\n/// # Arguments\n/// * `{}` - {}\n",
                param.name,
                infer_param_description(&param.name, &param.ty)
            ));
        }
        if let Some(ret) = &func.return_type {
            out.push_str(&format!("///\n/// # Returns\n/// `{}`\n", ret));
        }
        out.push_str(&format!("// fn {}(...)\n\n", func.name));
    }

    for s in &extracted.structs {
        if s.doc_comment.trim().is_empty() {
            out.push_str(&format!(
                "/// Struct `{}` used by the contract API.\n",
                s.name
            ));
            out.push_str(&format!("// struct {}\n\n", s.name));
        }
    }

    for e in &extracted.enums {
        if e.doc_comment.trim().is_empty() {
            out.push_str(&format!(
                "/// Enum `{}` used by the contract API.\n",
                e.name
            ));
            out.push_str(&format!("// enum {}\n\n", e.name));
        }
    }

    out
}

#[derive(Debug, Deserialize)]
struct LlmEnrichment {
    architecture: Option<String>,
    security: Option<String>,
    functions: Vec<LlmFunctionEnrichment>,
}

#[derive(Debug, Deserialize)]
struct LlmFunctionEnrichment {
    name: String,
    description: String,
    #[serde(default)]
    examples: Vec<String>,
}

fn try_llm_enrichment(
    extracted: &ExtractedDocs,
    description: &str,
    functions: &[FunctionDoc],
) -> Result<Option<LlmEnrichment>> {
    let api_key = match std::env::var("STARFORGE_AI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => return Ok(None),
    };

    let base_url = std::env::var("STARFORGE_AI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = std::env::var("STARFORGE_AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let model_for_telemetry = model.clone();

    let summary = serde_json::json!({
        "description": description,
        "module_doc": extracted.module_doc,
        "functions": functions.iter().map(|f| {
            serde_json::json!({
                "name": f.name,
                "description": f.description,
                "parameters": f.parameters,
                "returns": f.returns,
            })
        }).collect::<Vec<_>>(),
    });

    let body = serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": "You enrich Soroban smart-contract documentation. \
    Return JSON with keys: architecture (string), security (string), \
    functions (array of {name, description, examples}). \
    Do not invent ABI members that are not provided. Keep examples accurate."
            },
            {
                "role": "user",
                "content": format!(
                    "Enrich this contract documentation context:\n{}",
                    serde_json::to_string_pretty(&summary)?
                )
            }
        ]
    });

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    // Blocking call via reqwest runtime is awkward in sync context; use ureq-less
    // approach with reqwest blocking feature if available. Fall back to skipping
    // when the async client cannot be blocked safely.
    // Run the async HTTP call on a dedicated runtime thread so this works both
    // inside and outside an existing tokio context without nested block_on panics.
    let start = std::time::Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("Failed to create tokio runtime for LLM enrichment")?;
            rt.block_on(async {
                let resp = http_client::get_client()
                    .post(&url)
                    .bearer_auth(api_key)
                    .json(&body)
                    .send()
                    .await
                    .context("LLM request failed")?
                    .error_for_status()
                    .context("LLM API returned an error status")?
                    .json::<serde_json::Value>()
                    .await
                    .context("Failed to parse LLM response")?;
                let tokens_in = resp
                    .pointer("/usage/prompt_tokens")
                    .and_then(|v| v.as_u64());
                let tokens_out = resp
                    .pointer("/usage/completion_tokens")
                    .and_then(|v| v.as_u64());
                let enrichment = parse_llm_content(resp);
                Ok::<_, anyhow::Error>((enrichment, tokens_in, tokens_out))
            })
        })();
        let _ = tx.send(result);
    });

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let outcome = rx
        .recv()
        .context("LLM enrichment worker exited unexpectedly")?;

    match outcome {
        Ok((enrichment, tokens_in, tokens_out)) => {
            crate::utils::ai_telemetry::record_call(
                "openai",
                &model_for_telemetry,
                "docs-generate",
                tokens_in,
                tokens_out,
                elapsed_ms,
                true,
                None,
            );
            enrichment
        }
        Err(e) => {
            crate::utils::ai_telemetry::record_call(
                "openai",
                &model_for_telemetry,
                "docs-generate",
                None,
                None,
                elapsed_ms,
                false,
                Some(classify_docs_error(&e)),
            );
            Err(e)
        }
    }
}

fn classify_docs_error(err: &anyhow::Error) -> &'static str {
    let msg = err.to_string().to_lowercase();
    if msg.contains("timeout") || msg.contains("timed out") {
        "timeout"
    } else if msg.contains("429") || msg.contains("rate limit") {
        "rate_limit"
    } else if msg.contains("401") || msg.contains("403") || msg.contains("error status") {
        "auth"
    } else if msg.contains("request failed") || msg.contains("connection") {
        "network"
    } else if msg.contains("parse") || msg.contains("json") {
        "invalid_response"
    } else {
        "unknown"
    }
}

fn parse_llm_content(resp: serde_json::Value) -> Result<Option<LlmEnrichment>> {
    let content = resp
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if content.trim().is_empty() {
        return Ok(None);
    }
    let parsed: LlmEnrichment =
        serde_json::from_str(content).context("LLM returned invalid enrichment JSON")?;
    Ok(Some(parsed))
}

fn first_sentence(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let sentence = trimmed.split(['.', '\n']).next().unwrap_or(trimmed).trim();
    if sentence.is_empty() {
        None
    } else if sentence.ends_with('.') {
        Some(sentence.to_string())
    } else {
        Some(format!("{}.", sentence))
    }
}

fn first_paragraph(text: &str) -> String {
    text.split("\n\n")
        .next()
        .unwrap_or(text)
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn split_lang_example(example: &str) -> (&str, String) {
    if let Some(rest) = example.strip_prefix("// lang: ") {
        let mut lines = rest.lines();
        let lang = lines.next().unwrap_or("text");
        return (lang, lines.collect::<Vec<_>>().join("\n"));
    }
    if example.contains("// TypeScript") {
        ("typescript", example.to_string())
    } else if example.contains("# Python") {
        ("python", example.to_string())
    } else if example.contains("// Go") {
        ("go", example.to_string())
    } else {
        ("rust", example.to_string())
    }
}

fn sample_value_rust(name: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    if t.contains("address") {
        name.to_string()
    } else if t.contains("i128") || t.contains("u128") || t.contains("u32") || t.contains("i32") {
        "1_000".to_string()
    } else if t.contains("bool") {
        "true".to_string()
    } else if t.contains("bytes") || t.contains("string") || t.contains("symbol") {
        format!("{}_value", name)
    } else {
        name.to_string()
    }
}

fn sample_value_ts(name: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    if t.contains("address") {
        format!("\"G...{}\"", name)
    } else if t.contains("bool") {
        "true".to_string()
    } else if t.contains("i128") || t.contains("u128") || t.contains("u32") || t.contains("i32") {
        "1000n".to_string()
    } else {
        format!("\"{}\"", name)
    }
}

fn sample_value_py(name: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    if t.contains("address") {
        format!("\"G...{}\"", name)
    } else if t.contains("bool") {
        "True".to_string()
    } else if t.contains("i128") || t.contains("u128") || t.contains("u32") || t.contains("i32") {
        "1000".to_string()
    } else {
        format!("\"{}\"", name)
    }
}

fn sample_value_go(name: &str, ty: &str) -> String {
    let t = ty.to_ascii_lowercase();
    if t.contains("address") {
        format!("\"G...{}\"", name)
    } else if t.contains("bool") {
        "true".to_string()
    } else if t.contains("i128") || t.contains("u128") || t.contains("u32") || t.contains("i32") {
        "1000".to_string()
    } else {
        format!("\"{}\"", name)
    }
}

fn to_camel(name: &str) -> String {
    let mut parts = name.split('_');
    let Some(first) = parts.next() else {
        return name.to_string();
    };
    let mut out = first.to_string();
    for part in parts {
        let mut chars = part.chars();
        if let Some(c) = chars.next() {
            out.push(c.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    out
}

fn to_pascal(name: &str) -> String {
    name.split('_')
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_ascii_uppercase().to_string();
                    s.extend(chars);
                    s
                }
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE_CONTRACT: &str = r#"
//! A simple counter contract for testing documentation generation.

#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

const COUNTER: Symbol = symbol_short!("COUNTER");

#[contracttype]
pub enum DataKey {
    /// Administrator address key
    Admin,
    Count,
}

/// Emitted when the counter changes.
pub struct CounterEvent {
    pub value: u32,
}

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    /// Increment the counter and return the new value.
    ///
    /// # Examples
    ///
    /// ```
    /// let value = client.increment();
    /// ```
    pub fn increment(env: Env) -> u32 {
        let mut count: u32 = env.storage().instance().get(&COUNTER).unwrap_or(0);
        count += 1;
        env.storage().instance().set(&COUNTER, &count);
        count
    }

    pub fn get_count(env: Env) -> u32 {
        env.storage().instance().get(&COUNTER).unwrap_or(0)
    }

    pub fn reset(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&COUNTER, &0u32);
    }

    pub fn set_admin(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }
}
"#;

    #[test]
    fn generates_comprehensive_markdown() {
        let extracted = DocCommentExtractor::extract_from_source(SAMPLE_CONTRACT);
        let options = AiDocsOptions {
            contract_id: "counter".into(),
            name: "Counter".into(),
            description: None,
            network: "testnet".into(),
            version: "1.0.0".into(),
            languages: DocLanguage::parse_list("rust,ts,python"),
            use_llm: false,
        };

        let docs = generate_from_extracted(&extracted, SAMPLE_CONTRACT, &options).unwrap();
        assert_eq!(docs.enrichment_mode, "heuristic");
        assert!(docs.markdown.contains("# Counter Documentation"));
        assert!(docs.markdown.contains("## Architecture"));
        assert!(docs.markdown.contains("## Storage Layout"));
        assert!(docs.markdown.contains("## Configuration Reference"));
        assert!(docs.markdown.contains("## Security Considerations"));
        assert!(docs.markdown.contains("## Usage Guides"));
        assert!(docs.markdown.contains("## Troubleshooting"));
        assert!(docs.markdown.contains("## API Reference"));
        assert!(docs.markdown.contains("increment"));
        assert!(docs.markdown.contains("get_count"));
        assert!(docs.entry.api.functions.len() >= 3);
        assert!(docs
            .entry
            .api
            .storage
            .iter()
            .any(|s| s.key == "COUNTER" || s.key == "Admin"));
        assert!(docs.rustdoc_stubs.contains("get_count") || docs.markdown.contains("get_count"));
        assert!(
            docs.markdown.contains("typescript")
                || docs.markdown.contains("TypeScript")
                || docs.markdown.contains("```typescript")
        );
    }

    #[test]
    fn multi_language_parser() {
        let langs = DocLanguage::parse_list("rust, typescript, py, go");
        assert_eq!(langs.len(), 4);
        assert!(langs.contains(&DocLanguage::Rust));
        assert!(langs.contains(&DocLanguage::Go));
    }

    #[test]
    fn generate_from_temp_file_and_persist_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("lib.rs");
        let mut file = fs::File::create(&source).unwrap();
        write!(file, "{}", SAMPLE_CONTRACT).unwrap();

        let options = AiDocsOptions {
            contract_id: format!("test-counter-{}", uuid::Uuid::new_v4()),
            name: "Counter".into(),
            description: Some("Test counter contract".into()),
            network: "testnet".into(),
            version: "0.1.0".into(),
            languages: vec![DocLanguage::Rust, DocLanguage::TypeScript],
            use_llm: false,
        };

        let generated = generate_from_source(&source, &options).unwrap();
        let md_path = dir.path().join("COUNTER.md");
        let rustdoc_path = dir.path().join("rustdoc_stubs.rs");
        let entry = persist_generated(&generated, Some(&md_path), Some(&rustdoc_path)).unwrap();

        assert_eq!(entry.name, "Counter");
        assert!(md_path.exists());
        let md = fs::read_to_string(md_path).unwrap();
        assert!(md.contains("Security Considerations"));
        assert!(rustdoc_path.exists());
    }

    #[test]
    fn security_warns_without_auth() {
        let source = r#"
#[contractimpl]
impl Token {
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {}
}
"#;
        let extracted = DocCommentExtractor::extract_from_source(source);
        let notes = analyze_security(source, &extracted);
        assert!(notes.contains("require_auth"));
    }

    #[test]
    fn configuration_reference_lists_constants() {
        let extracted = DocCommentExtractor::extract_from_source(SAMPLE_CONTRACT);
        let out = document_configuration(&extracted, "testnet", "1.2.3");
        assert!(out.contains("testnet"));
        assert!(out.contains("1.2.3"));
        assert!(out.contains("COUNTER"));
    }

    #[test]
    fn configuration_reference_handles_no_constants() {
        let extracted = DocCommentExtractor::extract_from_source("pub fn noop() {}");
        let out = document_configuration(&extracted, "testnet", "1.0.0");
        assert!(out.contains("No module-level constants"));
    }

    #[test]
    fn troubleshooting_flags_auth_and_storage() {
        let extracted = DocCommentExtractor::extract_from_source(SAMPLE_CONTRACT);
        let storage = vec![StorageDoc {
            key: "COUNTER".into(),
            ty: "u32".into(),
            description: String::new(),
        }];
        let tips = build_troubleshooting(SAMPLE_CONTRACT, &extracted, &storage);
        assert!(tips.contains("Auth, InvalidAction"));
        assert!(tips.contains("initialize"));
    }

    #[test]
    fn troubleshooting_flags_unwrap_panics() {
        let source = "pub fn f(x: Option<u32>) -> u32 { x.unwrap() }";
        let extracted = DocCommentExtractor::extract_from_source(source);
        let tips = build_troubleshooting(source, &extracted, &[]);
        assert!(tips.to_lowercase().contains("unwrap"));
    }
}
