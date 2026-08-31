//! Documentation template system for the StarForge contract doc generator.
//!
//! Templates are thin wrappers around static string layouts that accept a
//! [`TemplateContext`] and produce rendered output (HTML or Markdown).
//! Custom templates can be loaded from disk; built-in templates are compiled
//! into the binary.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

// ──────────────────────────────────────────────────────────────────────────────
// Template context
// ──────────────────────────────────────────────────────────────────────────────

/// All variables available for template rendering.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateContext {
    /// Contract display name.
    pub name: String,
    /// On-chain contract ID.
    pub contract_id: String,
    /// Short description.
    pub description: String,
    /// Semantic version string.
    pub version: String,
    /// Author or deployer address.
    pub author: String,
    /// Network name (testnet / mainnet).
    pub network: String,
    /// ISO-8601 timestamp of generation.
    pub generated_at: String,
    /// Rendered HTML / Markdown for each section (key = section title).
    /// Uses `BTreeMap` so that iteration order is deterministic (sorted by key).
    pub sections: BTreeMap<String, String>,
    /// Rendered function docs blocks.
    pub functions_html: String,
    /// Rendered event docs blocks.
    pub events_html: String,
    /// Rendered storage docs blocks.
    pub storage_html: String,
    /// Rendered usage example blocks.
    pub examples_html: String,
    /// Optional URL of a deployed documentation site.
    pub site_url: Option<String>,
}

impl TemplateContext {
    /// Apply a very small `{{key}}` substitution over a template string.
    ///
    /// Supported keys are the string fields of [`TemplateContext`].  The
    /// `sections`, `functions_html`, etc. pre-rendered blocks are injected
    /// via their explicit field names.
    pub fn render(&self, template: &str) -> String {
        let mut out = template.to_string();

        let replacements: &[(&str, &str)] = &[
            ("{{name}}", &self.name),
            ("{{contract_id}}", &self.contract_id),
            ("{{description}}", &self.description),
            ("{{version}}", &self.version),
            ("{{author}}", &self.author),
            ("{{network}}", &self.network),
            ("{{generated_at}}", &self.generated_at),
            ("{{functions_html}}", &self.functions_html),
            ("{{events_html}}", &self.events_html),
            ("{{storage_html}}", &self.storage_html),
            ("{{examples_html}}", &self.examples_html),
            ("{{site_url}}", self.site_url.as_deref().unwrap_or("#")),
        ];

        for (placeholder, value) in replacements {
            out = out.replace(placeholder, value);
        }

        // Inject custom sections by title.
        for (title, content) in &self.sections {
            let key = format!("{{{{section:{}}}}}", title);
            out = out.replace(&key, content);
        }

        out
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Template variants
// ──────────────────────────────────────────────────────────────────────────────

/// Available built-in template styles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateKind {
    /// Full single-page HTML contract reference.
    HtmlFull,
    /// Minimal HTML card (for embedding in portal index).
    HtmlCard,
    /// GitHub-flavoured Markdown API reference.
    MarkdownFull,
    /// A short Markdown summary suitable for README injection.
    MarkdownSummary,
    /// Custom template loaded from a file path.
    Custom(PathBuf),
}

impl TemplateKind {
    /// Return the template string for this variant.
    pub fn load(&self) -> Result<String> {
        match self {
            TemplateKind::HtmlFull => Ok(HTML_FULL_TEMPLATE.to_string()),
            TemplateKind::HtmlCard => Ok(HTML_CARD_TEMPLATE.to_string()),
            TemplateKind::MarkdownFull => Ok(MARKDOWN_FULL_TEMPLATE.to_string()),
            TemplateKind::MarkdownSummary => Ok(MARKDOWN_SUMMARY_TEMPLATE.to_string()),
            TemplateKind::Custom(path) => fs::read_to_string(path)
                .with_context(|| format!("Failed to load custom template: {}", path.display())),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Template manager
// ──────────────────────────────────────────────────────────────────────────────

/// Manages template resolution, caching, and rendering.
pub struct TemplateManager {
    /// Optional directory containing user-supplied `.html` / `.md` templates.
    custom_dir: Option<PathBuf>,
    /// In-memory cache: template content keyed by a stable name.
    cache: HashMap<String, String>,
}

impl TemplateManager {
    /// Create a manager without a custom template directory.
    pub fn new() -> Self {
        Self {
            custom_dir: None,
            cache: HashMap::new(),
        }
    }

    /// Create a manager that will look for custom templates in `dir`.
    pub fn with_custom_dir(dir: PathBuf) -> Self {
        Self {
            custom_dir: Some(dir),
            cache: HashMap::new(),
        }
    }

    /// Render `kind` with `ctx`, returning the finished document string.
    pub fn render(&mut self, kind: &TemplateKind, ctx: &TemplateContext) -> Result<String> {
        let cache_key = format!("{:?}", kind);
        let template = if let Some(cached) = self.cache.get(&cache_key) {
            cached.clone()
        } else {
            // Try custom dir first for HtmlFull / MarkdownFull.
            let tpl = self
                .try_load_from_custom_dir(kind)
                .unwrap_or_else(|| kind.load().unwrap_or_default());
            self.cache.insert(cache_key, tpl.clone());
            tpl
        };

        Ok(ctx.render(&template))
    }

    /// Try to load a template from the custom directory (best-effort).
    fn try_load_from_custom_dir(&self, kind: &TemplateKind) -> Option<String> {
        let dir = self.custom_dir.as_ref()?;
        let filename = match kind {
            TemplateKind::HtmlFull => "contract.html",
            TemplateKind::HtmlCard => "card.html",
            TemplateKind::MarkdownFull => "contract.md",
            TemplateKind::MarkdownSummary => "summary.md",
            TemplateKind::Custom(_) => return None,
        };
        let path = dir.join(filename);
        fs::read_to_string(path).ok()
    }

    /// List all `.html` and `.md` files found in the custom template directory.
    pub fn list_custom_templates(&self) -> Vec<PathBuf> {
        let Some(dir) = &self.custom_dir else {
            return vec![];
        };
        let Ok(entries) = fs::read_dir(dir) else {
            return vec![];
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("html") | Some("md")
                )
            })
            .collect();
        paths.sort();
        paths
    }
}

impl Default for TemplateManager {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in templates
// ──────────────────────────────────────────────────────────────────────────────

const HTML_FULL_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{{name}} — StarForge Docs</title>
  <style>
    :root {
      --bg: #0d1117; --surface: #161b22; --border: #30363d;
      --accent: #58a6ff; --text: #c9d1d9; --muted: #8b949e;
      --success: #3fb950; --warning: #d29922; --code-bg: #1f2428;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; line-height: 1.6; }
    a { color: var(--accent); text-decoration: none; }
    a:hover { text-decoration: underline; }
    .layout { display: flex; min-height: 100vh; }
    .sidebar { width: 260px; background: var(--surface); border-right: 1px solid var(--border); padding: 24px 16px; position: sticky; top: 0; height: 100vh; overflow-y: auto; flex-shrink: 0; }
    .sidebar h2 { font-size: 14px; text-transform: uppercase; letter-spacing: .08em; color: var(--muted); margin-bottom: 12px; }
    .sidebar ul { list-style: none; }
    .sidebar li a { display: block; padding: 4px 8px; border-radius: 4px; color: var(--text); font-size: 14px; }
    .sidebar li a:hover { background: var(--border); }
    .main { flex: 1; max-width: 900px; padding: 40px 48px; }
    .page-header { border-bottom: 1px solid var(--border); padding-bottom: 24px; margin-bottom: 32px; }
    .page-header h1 { font-size: 28px; color: var(--accent); margin-bottom: 8px; }
    .page-header p { color: var(--muted); font-size: 15px; }
    .meta-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px; margin: 20px 0 32px; }
    .meta-item { background: var(--surface); border: 1px solid var(--border); border-radius: 6px; padding: 10px 14px; }
    .meta-item .label { font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--muted); margin-bottom: 4px; }
    .meta-item .value { font-size: 13px; font-family: monospace; color: var(--text); word-break: break-all; }
    .section { margin-bottom: 40px; }
    .section h2 { font-size: 20px; color: var(--text); border-bottom: 1px solid var(--border); padding-bottom: 8px; margin-bottom: 20px; }
    .fn-card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 20px; margin-bottom: 16px; }
    .fn-card .fn-name { font-size: 16px; font-family: monospace; color: var(--accent); margin-bottom: 6px; }
    .fn-card .fn-desc { color: var(--muted); font-size: 14px; margin-bottom: 12px; }
    .badge { display: inline-block; padding: 2px 8px; border-radius: 20px; font-size: 11px; font-weight: 600; }
    .badge-pub { background: #1f4a20; color: var(--success); }
    .badge-admin { background: #4a2e1f; color: var(--warning); }
    .params-table { width: 100%; border-collapse: collapse; font-size: 13px; margin-top: 8px; }
    .params-table th { text-align: left; padding: 6px 10px; background: var(--code-bg); color: var(--muted); font-weight: 500; }
    .params-table td { padding: 6px 10px; border-top: 1px solid var(--border); }
    .params-table td.type { font-family: monospace; color: var(--accent); }
    .code-block { background: var(--code-bg); border: 1px solid var(--border); border-radius: 6px; padding: 14px; font-family: monospace; font-size: 13px; white-space: pre-wrap; overflow-x: auto; margin-top: 10px; }
    .example-card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 20px; margin-bottom: 16px; }
    .example-card h4 { color: var(--text); margin-bottom: 8px; }
    footer { border-top: 1px solid var(--border); padding: 20px 48px; color: var(--muted); font-size: 12px; }
  </style>
</head>
<body>
<div class="layout">
  <nav class="sidebar">
    <h2>⚡ StarForge</h2>
    <ul>
      <li><a href="#overview">Overview</a></li>
      <li><a href="#functions">Functions</a></li>
      <li><a href="#events">Events</a></li>
      <li><a href="#storage">Storage</a></li>
      <li><a href="#examples">Examples</a></li>
    </ul>
  </nav>
  <div class="main">
    <div class="page-header">
      <h1>{{name}}</h1>
      <p>{{description}}</p>
    </div>

    <div class="meta-grid">
      <div class="meta-item"><div class="label">Contract ID</div><div class="value">{{contract_id}}</div></div>
      <div class="meta-item"><div class="label">Version</div><div class="value">{{version}}</div></div>
      <div class="meta-item"><div class="label">Author</div><div class="value">{{author}}</div></div>
      <div class="meta-item"><div class="label">Network</div><div class="value">{{network}}</div></div>
      <div class="meta-item"><div class="label">Generated</div><div class="value">{{generated_at}}</div></div>
    </div>

    <div class="section" id="functions">
      <h2>Functions</h2>
      {{functions_html}}
    </div>

    <div class="section" id="events">
      <h2>Events</h2>
      {{events_html}}
    </div>

    <div class="section" id="storage">
      <h2>Storage</h2>
      {{storage_html}}
    </div>

    <div class="section" id="examples">
      <h2>Usage Examples</h2>
      {{examples_html}}
    </div>
  </div>
</div>
<footer>
  Generated by <strong>StarForge</strong> Contract Documentation Generator &middot; {{generated_at}}
</footer>
</body>
</html>"##;

const HTML_CARD_TEMPLATE: &str = r#"<div class="contract-card" data-id="{{contract_id}}">
  <h3><a href="{{contract_id}}.html">{{name}}</a></h3>
  <div class="contract-id">{{contract_id}}</div>
  <p class="description">{{description}}</p>
  <span class="badge">{{network}}</span>
  <span class="badge">v{{version}}</span>
</div>"#;

const MARKDOWN_FULL_TEMPLATE: &str = r#"# {{name}}

> {{description}}

| Field | Value |
|-------|-------|
| Contract ID | `{{contract_id}}` |
| Version | `{{version}}` |
| Author | `{{author}}` |
| Network | `{{network}}` |
| Generated | {{generated_at}} |

{{functions_html}}

{{events_html}}

{{storage_html}}

{{examples_html}}

---
*Generated by [StarForge](https://github.com/Nanle-code/StarForge) Contract Documentation Generator*
"#;

const MARKDOWN_SUMMARY_TEMPLATE: &str = r#"## {{name}} (`{{contract_id}}`)

{{description}}

- **Network:** {{network}}
- **Version:** `{{version}}`
- **Author:** `{{author}}`
"#;

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ctx() -> TemplateContext {
        TemplateContext {
            name: "TokenContract".to_string(),
            contract_id: "CABC1234".to_string(),
            description: "A simple token contract".to_string(),
            version: "1.0.0".to_string(),
            author: "GDEMO".to_string(),
            network: "testnet".to_string(),
            generated_at: "2026-01-01".to_string(),
            functions_html: "<p>functions</p>".to_string(),
            events_html: "<p>events</p>".to_string(),
            storage_html: "<p>storage</p>".to_string(),
            examples_html: "<p>examples</p>".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn context_renders_placeholders() {
        let ctx = sample_ctx();
        let out = ctx.render("Name: {{name}} / ID: {{contract_id}}");
        assert_eq!(out, "Name: TokenContract / ID: CABC1234");
    }

    #[test]
    fn html_full_template_renders() {
        let mut mgr = TemplateManager::new();
        let out = mgr.render(&TemplateKind::HtmlFull, &sample_ctx()).unwrap();
        assert!(out.contains("TokenContract"));
        assert!(out.contains("CABC1234"));
        assert!(out.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn markdown_full_template_renders() {
        let mut mgr = TemplateManager::new();
        let out = mgr
            .render(&TemplateKind::MarkdownFull, &sample_ctx())
            .unwrap();
        assert!(out.contains("# TokenContract"));
        assert!(out.contains("`CABC1234`"));
    }

    /// Verify that rendering with BTreeMap-backed sections is deterministic:
    /// the same context always produces the same output regardless of insertion
    /// order.
    #[test]
    fn sections_render_in_deterministic_order() {
        use std::collections::BTreeMap;

        let mut sections_a = BTreeMap::new();
        sections_a.insert("Zebra".to_string(), "<p>z</p>".to_string());
        sections_a.insert("Alpha".to_string(), "<p>a</p>".to_string());
        sections_a.insert("Middle".to_string(), "<p>m</p>".to_string());

        let mut sections_b = BTreeMap::new();
        sections_b.insert("Alpha".to_string(), "<p>a</p>".to_string());
        sections_b.insert("Middle".to_string(), "<p>m</p>".to_string());
        sections_b.insert("Zebra".to_string(), "<p>z</p>".to_string());

        let mut ctx_a = sample_ctx();
        ctx_a.sections = sections_a;
        let mut ctx_b = sample_ctx();
        ctx_b.sections = sections_b;

        let template = "before {{section:Alpha}} mid {{section:Middle}} end {{section:Zebra}}";
        let out_a = ctx_a.render(template);
        let out_b = ctx_b.render(template);

        assert_eq!(
            out_a, out_b,
            "BTreeMap ensures same output regardless of insertion order"
        );
        assert_eq!(out_a, "before <p>a</p> mid <p>m</p> end <p>z</p>");
    }

    /// Verify that list_custom_templates returns files in sorted order.
    #[test]
    fn list_custom_templates_is_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("z_template.html"), "<div>z</div>").unwrap();
        fs::write(dir.join("a_template.md"), "# a").unwrap();
        fs::write(dir.join("m_template.html"), "<div>m</div>").unwrap();
        fs::write(dir.join("not_a_template.txt"), "skip").unwrap();

        let mgr = TemplateManager::with_custom_dir(dir.to_path_buf());
        let paths = mgr.list_custom_templates();
        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();

        assert_eq!(
            names,
            vec!["a_template.md", "m_template.html", "z_template.html"]
        );
    }
}
