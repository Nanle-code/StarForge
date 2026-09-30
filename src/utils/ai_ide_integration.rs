//! AI-powered IDE integration for Soroban development.
//!
//! Generates the configuration an editor needs to surface starforge's AI
//! assistance in-place, and answers editor requests over a small,
//! transport-agnostic protocol:
//!
//! - **Scaffolding** — emit ready-to-commit config for VS Code, IntelliJ,
//!   Neovim, and Zed (tasks, launch targets, an LSP hook, and snippets).
//! - **Requests** — respond to `hover`, `completion`, `diagnostics`, `codeAction`
//!   and `explain` with structured payloads an extension can render directly.
//!
//! The request layer is deliberately independent of any wire format: it takes a
//! typed [`IdeRequest`] and returns a typed [`IdeResponse`], so the same logic
//! backs an LSP server, a CLI pipe, or an editor plug-in without change.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Editors that starforge can generate integration files for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ide {
    VsCode,
    IntelliJ,
    Neovim,
    Zed,
}

impl Ide {
    pub fn parse(value: &str) -> Option<Self> {
        match value
            .trim()
            .to_lowercase()
            .replace(['-', '_', ' '], "")
            .as_str()
        {
            "vscode" | "code" | "vsc" => Some(Ide::VsCode),
            "intellij" | "idea" | "jetbrains" | "clion" | "rustrover" => Some(Ide::IntelliJ),
            "neovim" | "nvim" | "vim" => Some(Ide::Neovim),
            "zed" => Some(Ide::Zed),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Ide::VsCode => "vscode",
            Ide::IntelliJ => "intellij",
            Ide::Neovim => "neovim",
            Ide::Zed => "zed",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Ide::VsCode => "Visual Studio Code",
            Ide::IntelliJ => "IntelliJ IDEA / RustRover",
            Ide::Neovim => "Neovim",
            Ide::Zed => "Zed",
        }
    }

    /// Every editor starforge knows how to configure.
    pub fn all() -> Vec<Ide> {
        vec![Ide::VsCode, Ide::IntelliJ, Ide::Neovim, Ide::Zed]
    }
}

impl std::fmt::Display for Ide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

/// A file the integration wants to write into the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedFile {
    /// Path relative to the project root.
    pub relative_path: String,
    pub contents: String,
    /// What this file gives the developer.
    pub purpose: String,
}

/// The full set of files for one editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeIntegration {
    pub ide: String,
    pub display_name: String,
    pub files: Vec<GeneratedFile>,
    /// Steps the developer still has to perform by hand.
    pub manual_steps: Vec<String>,
}

/// Kinds of request an editor can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdeRequestKind {
    Hover,
    Completion,
    Diagnostics,
    CodeAction,
    Explain,
}

impl IdeRequestKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().replace(['-', '_'], "").as_str() {
            "hover" => Some(IdeRequestKind::Hover),
            "completion" | "complete" => Some(IdeRequestKind::Completion),
            "diagnostics" | "diagnostic" => Some(IdeRequestKind::Diagnostics),
            "codeaction" | "action" | "fix" => Some(IdeRequestKind::CodeAction),
            "explain" => Some(IdeRequestKind::Explain),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            IdeRequestKind::Hover => "hover",
            IdeRequestKind::Completion => "completion",
            IdeRequestKind::Diagnostics => "diagnostics",
            IdeRequestKind::CodeAction => "codeAction",
            IdeRequestKind::Explain => "explain",
        }
    }
}

/// A request from the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeRequest {
    pub kind: IdeRequestKind,
    /// Full text of the buffer.
    pub source: String,
    /// 1-based cursor line.
    pub line: usize,
    /// 0-based cursor column.
    pub column: usize,
    /// Buffer path, used only for reporting.
    #[serde(default)]
    pub file: Option<String>,
}

/// A completion candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    /// Text to insert; may contain `$0` for the final cursor position.
    pub insert_text: String,
    pub kind: String,
}

/// A diagnostic to render in the editor gutter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdeDiagnostic {
    pub line: usize,
    pub severity: String,
    pub message: String,
    pub code: String,
}

/// A quick-fix the editor can offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeAction {
    pub title: String,
    pub line: usize,
    /// Line content to replace the original with.
    pub replacement: String,
}

/// Structured answer to an [`IdeRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IdeResponse {
    Hover {
        symbol: String,
        markdown: String,
    },
    Completion {
        items: Vec<CompletionItem>,
    },
    Diagnostics {
        diagnostics: Vec<IdeDiagnostic>,
    },
    CodeActions {
        actions: Vec<CodeAction>,
    },
    Explanation {
        summary: String,
        details: Vec<String>,
    },
}

/// Soroban symbols the hover provider knows about.
const SYMBOL_DOCS: &[(&str, &str)] = &[
    (
        "require_auth",
        "Asserts that the address authorised this invocation.\n\nCall it **before** any state change or transfer; without it any caller can \
         invoke the entry point.",
    ),
    (
        "storage",
        "Access to contract storage.\n\n`instance()` for small config, `persistent()` for user data that must outlive \
         the instance, `temporary()` for cheap short-lived entries.",
    ),
    (
        "extend_ttl",
        "Extends the time-to-live of a ledger entry.\n\nPersistent entries are archived once their TTL lapses; extend it whenever you \
         read an entry you intend to keep.",
    ),
    (
        "Env",
        "The host environment handle.\n\nEvery contract function takes it as the first parameter; it is the gateway to \
         storage, events, ledger info, and cryptography.",
    ),
    (
        "symbol_short",
        "Builds a `Symbol` from up to 9 characters at compile time.\n\nCheaper than `Symbol::new(&env, ..)` because it avoids a host allocation.",
    ),
    (
        "contracttype",
        "Derives the Soroban wire encoding for a struct or enum.\n\nAdding a variant changes discriminants; never reorder existing ones.",
    ),
];

/// Snippet completions offered inside a Soroban source file.
fn soroban_completions() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "sf-contract".to_string(),
            detail: "Soroban contract skeleton".to_string(),
            insert_text: "#[contract]\npub struct ${1:MyContract};\n\n#[contractimpl]\nimpl ${1:MyContract} {\n    $0\n}"
                .to_string(),
            kind: "snippet".to_string(),
        },
        CompletionItem {
            label: "sf-fn-auth".to_string(),
            detail: "Authorised entry point".to_string(),
            insert_text:
                "pub fn ${1:name}(env: Env, caller: Address) {\n    caller.require_auth();\n    $0\n}"
                    .to_string(),
            kind: "snippet".to_string(),
        },
        CompletionItem {
            label: "sf-storage-get".to_string(),
            detail: "Read a persistent entry with a default".to_string(),
            insert_text:
                "let ${1:value} = env.storage().persistent().get(&${2:key}).unwrap_or(${3:0});\n$0"
                    .to_string(),
            kind: "snippet".to_string(),
        },
        CompletionItem {
            label: "sf-storage-set".to_string(),
            detail: "Write a persistent entry".to_string(),
            insert_text: "env.storage().persistent().set(&${1:key}, &${2:value});\n$0".to_string(),
            kind: "snippet".to_string(),
        },
        CompletionItem {
            label: "sf-event".to_string(),
            detail: "Publish a contract event".to_string(),
            insert_text:
                "env.events().publish((symbol_short!(\"${1:topic}\"),), ${2:payload});\n$0"
                    .to_string(),
            kind: "snippet".to_string(),
        },
        CompletionItem {
            label: "sf-test".to_string(),
            detail: "Contract unit test".to_string(),
            insert_text: "#[test]\nfn ${1:test_name}() {\n    let env = Env::default();\n    $0\n}"
                .to_string(),
            kind: "snippet".to_string(),
        },
    ]
}

/// Returns the identifier under the cursor, if any.
///
/// `line` is 1-based (as editors report it) and `column` is 0-based.
pub fn symbol_at(source: &str, line: usize, column: usize) -> Option<String> {
    let target = source.lines().nth(line.checked_sub(1)?)?;
    let bytes = target.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    // Clamp so a cursor resting just past the last character still resolves.
    let mut start = column.min(bytes.len().saturating_sub(1));
    if !is_ident(bytes[start]) {
        // Fall back to the identifier immediately to the left of the cursor.
        start = start.checked_sub(1)?;
        if !is_ident(bytes[start]) {
            return None;
        }
    }

    let mut end = start;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    while end + 1 < bytes.len() && is_ident(bytes[end + 1]) {
        end += 1;
    }

    Some(target[start..=end].to_string())
}

/// Documentation for `symbol`, if starforge knows it.
pub fn lookup_symbol_doc(symbol: &str) -> Option<&'static str> {
    SYMBOL_DOCS
        .iter()
        .find(|(name, _)| *name == symbol)
        .map(|(_, doc)| *doc)
}

/// Lints the buffer and returns editor diagnostics.
///
/// Reuses the same static security checks the audit command runs, so what a
/// developer sees inline matches what CI will report.
pub fn diagnostics_for(source: &str) -> Vec<IdeDiagnostic> {
    let mut diagnostics = Vec::new();

    for finding in crate::utils::security::run_static_checks(source) {
        for line in &finding.line_numbers {
            diagnostics.push(IdeDiagnostic {
                line: *line,
                severity: finding.severity.clone(),
                message: finding.description.clone(),
                code: finding.pattern_name.clone(),
            });
        }
    }

    // Editor-only hints that are too noisy for an audit report but useful inline.
    for (index, raw) in source.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or(raw);
        if line.contains(".unwrap()") {
            diagnostics.push(IdeDiagnostic {
                line: index + 1,
                severity: "low".to_string(),
                message: "`unwrap()` panics on failure; prefer `?` or an explicit error"
                    .to_string(),
                code: "unwrap_in_contract".to_string(),
            });
        }
        if line.contains("todo!()") || line.contains("unimplemented!()") {
            diagnostics.push(IdeDiagnostic {
                line: index + 1,
                severity: "medium".to_string(),
                message: "Unimplemented body will panic if reached".to_string(),
                code: "unimplemented_body".to_string(),
            });
        }
    }

    diagnostics.sort_by_key(|d| (d.line, d.code.clone()));
    diagnostics
}

/// Derives quick-fixes from the diagnostics found in `source`.
pub fn code_actions_for(source: &str) -> Vec<CodeAction> {
    let lines: Vec<&str> = source.lines().collect();
    let mut actions = Vec::new();

    for diagnostic in diagnostics_for(source) {
        let Some(original) = lines.get(diagnostic.line.saturating_sub(1)) else {
            continue;
        };

        match diagnostic.code.as_str() {
            "unwrap_in_contract" => actions.push(CodeAction {
                title: "Replace `unwrap()` with `?`".to_string(),
                line: diagnostic.line,
                replacement: original.replace(".unwrap()", "?"),
            }),
            "missing_auth" => actions.push(CodeAction {
                title: "Insert `require_auth()` guard".to_string(),
                line: diagnostic.line,
                replacement: format!("{original}\n    caller.require_auth();"),
            }),
            "unchecked_arithmetic" => actions.push(CodeAction {
                title: "Use checked arithmetic".to_string(),
                line: diagnostic.line,
                replacement: original.replacen(" + ", ".checked_add(", 1).replacen(
                    " - ",
                    ".checked_sub(",
                    1,
                ),
            }),
            _ => {}
        }
    }

    actions
}

/// Answers an editor request.
pub fn handle_request(request: &IdeRequest) -> IdeResponse {
    match request.kind {
        IdeRequestKind::Hover => {
            let symbol = symbol_at(&request.source, request.line, request.column)
                .unwrap_or_else(|| "(none)".to_string());
            let markdown = lookup_symbol_doc(&symbol)
                .map(|doc| format!("### `{symbol}`\n\n{doc}"))
                .unwrap_or_else(|| {
                    format!("### `{symbol}`\n\nNo starforge documentation for this symbol.")
                });
            IdeResponse::Hover { symbol, markdown }
        }
        IdeRequestKind::Completion => IdeResponse::Completion {
            items: soroban_completions(),
        },
        IdeRequestKind::Diagnostics => IdeResponse::Diagnostics {
            diagnostics: diagnostics_for(&request.source),
        },
        IdeRequestKind::CodeAction => IdeResponse::CodeActions {
            actions: code_actions_for(&request.source),
        },
        IdeRequestKind::Explain => {
            let diagnostics = diagnostics_for(&request.source);
            let summary = if diagnostics.is_empty() {
                "No issues detected in this buffer.".to_string()
            } else {
                format!(
                    "{} issue(s) detected across {} line(s).",
                    diagnostics.len(),
                    request.source.lines().count()
                )
            };
            IdeResponse::Explanation {
                summary,
                details: diagnostics
                    .iter()
                    .map(|d| format!("line {}: [{}] {}", d.line, d.code, d.message))
                    .collect(),
            }
        }
    }
}

/// Builds the integration files for `ide`.
pub fn build_integration(ide: Ide) -> IdeIntegration {
    let files = match ide {
        Ide::VsCode => vec![
            GeneratedFile {
                relative_path: ".vscode/tasks.json".to_string(),
                contents: VSCODE_TASKS.to_string(),
                purpose: "Build, test, audit, and profile tasks".to_string(),
            },
            GeneratedFile {
                relative_path: ".vscode/settings.json".to_string(),
                contents: VSCODE_SETTINGS.to_string(),
                purpose: "rust-analyzer and starforge assistant settings".to_string(),
            },
            GeneratedFile {
                relative_path: ".vscode/starforge.code-snippets".to_string(),
                contents: build_vscode_snippets(),
                purpose: "Soroban snippets backed by the AI completion engine".to_string(),
            },
        ],
        Ide::IntelliJ => vec![
            GeneratedFile {
                relative_path: ".idea/runConfigurations/Starforge_Audit.xml".to_string(),
                contents: INTELLIJ_AUDIT_RUN_CONFIG.to_string(),
                purpose: "Run configuration for `starforge ai security-audit`".to_string(),
            },
            GeneratedFile {
                relative_path: ".idea/runConfigurations/Starforge_Profile.xml".to_string(),
                contents: INTELLIJ_PROFILE_RUN_CONFIG.to_string(),
                purpose: "Run configuration for `starforge ai profiling run`".to_string(),
            },
        ],
        Ide::Neovim => vec![GeneratedFile {
            relative_path: ".nvim/starforge.lua".to_string(),
            contents: NEOVIM_PLUGIN.to_string(),
            purpose: "Buffer-local commands wired to the starforge IDE bridge".to_string(),
        }],
        Ide::Zed => vec![GeneratedFile {
            relative_path: ".zed/tasks.json".to_string(),
            contents: ZED_TASKS.to_string(),
            purpose: "Zed task definitions for audit and profiling".to_string(),
        }],
    };

    let manual_steps = match ide {
        Ide::VsCode => vec![
            "Install the rust-analyzer extension if it is not already present.".to_string(),
            "Run the task 'starforge: AI audit' from the command palette to verify.".to_string(),
        ],
        Ide::IntelliJ => vec![
            "Reload the project so IntelliJ picks up the new run configurations.".to_string(),
            "Set the Rust toolchain under Settings → Languages & Frameworks → Rust.".to_string(),
        ],
        Ide::Neovim => vec![
            "Source the file from your config: `require('starforge')`.".to_string(),
            "Requires Neovim 0.9+ for `vim.system`.".to_string(),
        ],
        Ide::Zed => vec!["Open the task panel and run 'starforge: AI audit'.".to_string()],
    };

    IdeIntegration {
        ide: ide.slug().to_string(),
        display_name: ide.display_name().to_string(),
        files,
        manual_steps,
    }
}

/// Writes an integration into `project_root`.
///
/// Existing files are left untouched unless `force` is set, so re-running the
/// command never silently discards a developer's local edits.
pub fn write_integration(
    integration: &IdeIntegration,
    project_root: &Path,
    force: bool,
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();

    for file in &integration.files {
        let target = project_root.join(&file.relative_path);

        if target.exists() && !force {
            continue;
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        std::fs::write(&target, &file.contents)
            .with_context(|| format!("Failed to write {}", target.display()))?;
        written.push(target);
    }

    Ok(written)
}

/// Renders the snippet catalogue in VS Code's `.code-snippets` format.
fn build_vscode_snippets() -> String {
    let mut entries = Vec::new();

    for item in soroban_completions() {
        let body: Vec<String> = item
            .insert_text
            .split('\n')
            .map(|l| l.to_string())
            .collect();
        entries.push(format!(
            "  {}: {{\n    \"prefix\": {},\n    \"body\": {},\n    \"description\": {}\n  }}",
            serde_json::to_string(&item.label).unwrap_or_default(),
            serde_json::to_string(&item.label).unwrap_or_default(),
            serde_json::to_string(&body).unwrap_or_default(),
            serde_json::to_string(&item.detail).unwrap_or_default(),
        ));
    }

    format!("{{\n{}\n}}\n", entries.join(",\n"))
}

const VSCODE_TASKS: &str = r#"{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "starforge: build contract",
      "type": "shell",
      "command": "stellar contract build",
      "group": "build",
      "problemMatcher": ["$rustc"]
    },
    {
      "label": "starforge: AI audit",
      "type": "shell",
      "command": "starforge ai security-audit ${file}",
      "problemMatcher": []
    },
    {
      "label": "starforge: AI profile",
      "type": "shell",
      "command": "starforge ai profiling run --wasm ${workspaceFolder}/target/wasm32-unknown-unknown/release/contract.wasm",
      "problemMatcher": []
    },
    {
      "label": "starforge: test maintenance",
      "type": "shell",
      "command": "starforge ai test-maintain analyze --source ${workspaceFolder}/src --tests ${workspaceFolder}/tests",
      "problemMatcher": []
    }
  ]
}
"#;

const VSCODE_SETTINGS: &str = r#"{
  "rust-analyzer.cargo.target": "wasm32-unknown-unknown",
  "rust-analyzer.check.command": "clippy",
  "starforge.ai.enabled": true,
  "starforge.ai.diagnosticsOnSave": true,
  "starforge.ai.bridgeCommand": "starforge ai ide request --kind diagnostics --stdin"
}
"#;

const INTELLIJ_AUDIT_RUN_CONFIG: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="Starforge Audit" type="ShConfigurationType">
    <option name="SCRIPT_TEXT" value="starforge ai security-audit src/lib.rs" />
    <option name="INDEPENDENT_SCRIPT_PATH" value="true" />
    <option name="EXECUTE_IN_TERMINAL" value="true" />
    <method v="2" />
  </configuration>
</component>
"#;

const INTELLIJ_PROFILE_RUN_CONFIG: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="Starforge Profile" type="ShConfigurationType">
    <option name="SCRIPT_TEXT" value="starforge ai profiling run --wasm target/wasm32-unknown-unknown/release/contract.wasm" />
    <option name="INDEPENDENT_SCRIPT_PATH" value="true" />
    <option name="EXECUTE_IN_TERMINAL" value="true" />
    <method v="2" />
  </configuration>
</component>
"#;

const NEOVIM_PLUGIN: &str = r#"-- starforge AI integration for Neovim.
-- Source with: require('starforge')
local M = {}

local function bridge(kind)
  local file = vim.api.nvim_buf_get_name(0)
  local out = vim.fn.system({ 'starforge', 'ai-ide', 'request', '--kind', kind, '--file', file })
  vim.notify(out, vim.log.levels.INFO, { title = 'starforge' })
end

function M.diagnostics() bridge('diagnostics') end
function M.explain() bridge('explain') end
function M.actions() bridge('codeAction') end

vim.api.nvim_create_user_command('StarforgeDiagnostics', M.diagnostics, {})
vim.api.nvim_create_user_command('StarforgeExplain', M.explain, {})
vim.api.nvim_create_user_command('StarforgeActions', M.actions, {})

return M
"#;

const ZED_TASKS: &str = r#"[
  {
    "label": "starforge: AI audit",
    "command": "starforge",
    "args": ["ai", "security-audit", "$ZED_FILE"]
  },
  {
    "label": "starforge: AI profile",
    "command": "starforge",
    "args": ["ai", "profiling", "run", "--wasm", "target/wasm32-unknown-unknown/release/contract.wasm"]
  }
]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
pub fn withdraw(env: Env, amount: i128) {
    let balance = env.storage().persistent().get(&KEY).unwrap();
    env.storage().persistent().set(&KEY, balance - amount);
}
"#;

    #[test]
    fn ide_names_parse_with_aliases() {
        assert_eq!(Ide::parse("vscode"), Some(Ide::VsCode));
        assert_eq!(Ide::parse("VS-Code"), Some(Ide::VsCode));
        assert_eq!(Ide::parse("RustRover"), Some(Ide::IntelliJ));
        assert_eq!(Ide::parse("nvim"), Some(Ide::Neovim));
        assert_eq!(Ide::parse("emacs"), None);
    }

    #[test]
    fn every_known_ide_produces_files() {
        for ide in Ide::all() {
            let integration = build_integration(ide);
            assert!(
                !integration.files.is_empty(),
                "{} produced no files",
                ide.slug()
            );
            assert!(integration.files.iter().all(|f| !f.contents.is_empty()));
        }
    }

    #[test]
    fn generated_vscode_json_is_valid() {
        let integration = build_integration(Ide::VsCode);
        for file in &integration.files {
            assert!(
                serde_json::from_str::<serde_json::Value>(&file.contents).is_ok(),
                "{} is not valid JSON",
                file.relative_path
            );
        }
    }

    #[test]
    fn generated_zed_tasks_are_valid_json() {
        let integration = build_integration(Ide::Zed);
        for file in &integration.files {
            assert!(serde_json::from_str::<serde_json::Value>(&file.contents).is_ok());
        }
    }

    #[test]
    fn symbol_at_resolves_identifier_under_cursor() {
        let source = "caller.require_auth();";
        assert_eq!(
            symbol_at(source, 1, 10).as_deref(),
            Some("require_auth"),
            "cursor inside the identifier"
        );
        assert_eq!(symbol_at(source, 1, 0).as_deref(), Some("caller"));
    }

    #[test]
    fn symbol_at_handles_out_of_range_positions() {
        assert_eq!(symbol_at("abc", 9, 0), None);
        assert_eq!(symbol_at("", 1, 0), None);
        assert_eq!(symbol_at("  ", 1, 1), None);
    }

    #[test]
    fn hover_returns_documentation_for_known_symbols() {
        let request = IdeRequest {
            kind: IdeRequestKind::Hover,
            source: "caller.require_auth();".to_string(),
            line: 1,
            column: 10,
            file: None,
        };
        match handle_request(&request) {
            IdeResponse::Hover { symbol, markdown } => {
                assert_eq!(symbol, "require_auth");
                assert!(markdown.contains("authorised"));
            }
            other => panic!("expected hover, got {other:?}"),
        }
    }

    #[test]
    fn hover_degrades_gracefully_for_unknown_symbols() {
        let request = IdeRequest {
            kind: IdeRequestKind::Hover,
            source: "let banana = 1;".to_string(),
            line: 1,
            column: 4,
            file: None,
        };
        match handle_request(&request) {
            IdeResponse::Hover { symbol, markdown } => {
                assert_eq!(symbol, "banana");
                assert!(markdown.contains("No starforge documentation"));
            }
            other => panic!("expected hover, got {other:?}"),
        }
    }

    #[test]
    fn completion_offers_soroban_snippets() {
        let request = IdeRequest {
            kind: IdeRequestKind::Completion,
            source: String::new(),
            line: 1,
            column: 0,
            file: None,
        };
        match handle_request(&request) {
            IdeResponse::Completion { items } => {
                assert!(items.iter().any(|i| i.label == "sf-contract"));
                assert!(items.iter().all(|i| !i.insert_text.is_empty()));
            }
            other => panic!("expected completion, got {other:?}"),
        }
    }

    #[test]
    fn diagnostics_flag_unwrap_and_report_line_numbers() {
        let diagnostics = diagnostics_for(SAMPLE);
        assert!(diagnostics.iter().any(|d| d.code == "unwrap_in_contract"));
        assert!(
            diagnostics.iter().all(|d| d.line >= 1),
            "editor lines are 1-based"
        );
    }

    #[test]
    fn diagnostics_ignore_commented_out_code() {
        let source = "// let x = y.unwrap();\nlet z = 1;";
        assert!(!diagnostics_for(source)
            .iter()
            .any(|d| d.code == "unwrap_in_contract"));
    }

    #[test]
    fn diagnostics_are_sorted_by_line() {
        let diagnostics = diagnostics_for(SAMPLE);
        for pair in diagnostics.windows(2) {
            assert!(pair[0].line <= pair[1].line);
        }
    }

    #[test]
    fn code_actions_rewrite_unwrap() {
        let actions = code_actions_for("let a = b.unwrap();");
        let fix = actions
            .iter()
            .find(|a| a.title.contains("unwrap"))
            .expect("expected an unwrap quick-fix");
        assert_eq!(fix.replacement, "let a = b?;");
    }

    #[test]
    fn explain_summarises_a_clean_buffer() {
        let request = IdeRequest {
            kind: IdeRequestKind::Explain,
            source: "pub fn noop() {}".to_string(),
            line: 1,
            column: 0,
            file: None,
        };
        match handle_request(&request) {
            IdeResponse::Explanation { summary, details } => {
                assert!(summary.contains("No issues"));
                assert!(details.is_empty());
            }
            other => panic!("expected explanation, got {other:?}"),
        }
    }

    #[test]
    fn request_kinds_parse_with_aliases() {
        assert_eq!(
            IdeRequestKind::parse("code-action"),
            Some(IdeRequestKind::CodeAction)
        );
        assert_eq!(
            IdeRequestKind::parse("Diagnostics"),
            Some(IdeRequestKind::Diagnostics)
        );
        assert_eq!(IdeRequestKind::parse("nonsense"), None);
    }

    #[test]
    fn writing_an_integration_creates_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let integration = build_integration(Ide::VsCode);
        let written = write_integration(&integration, dir.path(), false).unwrap();

        assert_eq!(written.len(), integration.files.len());
        for file in &integration.files {
            assert!(dir.path().join(&file.relative_path).exists());
        }
    }

    #[test]
    fn existing_files_are_preserved_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let integration = build_integration(Ide::VsCode);
        write_integration(&integration, dir.path(), false).unwrap();

        let target = dir.path().join(&integration.files[0].relative_path);
        std::fs::write(&target, "CUSTOM").unwrap();

        let written = write_integration(&integration, dir.path(), false).unwrap();
        assert!(!written.contains(&target), "must not overwrite by default");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "CUSTOM");

        let forced = write_integration(&integration, dir.path(), true).unwrap();
        assert!(forced.contains(&target));
        assert_ne!(std::fs::read_to_string(&target).unwrap(), "CUSTOM");
    }

    #[test]
    fn responses_round_trip_through_json() {
        let request = IdeRequest {
            kind: IdeRequestKind::Diagnostics,
            source: SAMPLE.to_string(),
            line: 1,
            column: 0,
            file: Some("lib.rs".to_string()),
        };
        let response = handle_request(&request);
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(serde_json::from_str::<IdeResponse>(&encoded).is_ok());
    }
}
