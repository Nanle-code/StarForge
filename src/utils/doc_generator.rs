use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ── Extracted source types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedDocs {
    pub module_doc: String,
    pub functions: Vec<ExtractedFn>,
    pub structs: Vec<ExtractedStruct>,
    pub enums: Vec<ExtractedEnum>,
    pub constants: Vec<ExtractedConst>,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFn {
    pub name: String,
    pub doc_comment: String,
    pub signature: String,
    pub visibility: Visibility,
    pub params: Vec<ExtractedParam>,
    pub return_type: Option<String>,
    pub examples: Vec<CodeExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedStruct {
    pub name: String,
    pub doc_comment: String,
    pub fields: Vec<ExtractedField>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEnum {
    pub name: String,
    pub doc_comment: String,
    pub variants: Vec<ExtractedVariant>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedConst {
    pub name: String,
    pub doc_comment: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedParam {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedField {
    pub name: String,
    pub ty: String,
    pub doc_comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedVariant {
    pub name: String,
    pub doc_comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExample {
    pub lang: String,
    pub code: String,
}

// ── Doc comment extractor ─────────────────────────────────────────────────────

pub struct DocCommentExtractor;

impl DocCommentExtractor {
    pub fn extract_from_file(path: &Path) -> Result<ExtractedDocs> {
        let source =
            fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))?;
        let mut docs = Self::extract_from_source(&source);
        docs.source_path = path.display().to_string();
        Ok(docs)
    }

    pub fn extract_from_source(source: &str) -> ExtractedDocs {
        let mut docs = ExtractedDocs {
            module_doc: String::new(),
            functions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            constants: Vec::new(),
            source_path: String::new(),
        };

        let lines: Vec<&str> = source.lines().collect();
        let mut pending_doc: Vec<String> = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Module-level doc comment `//!`
            if line.starts_with("//!") {
                docs.module_doc.push_str(&strip_prefix(line, "//!"));
                docs.module_doc.push('\n');
                i += 1;
                continue;
            }

            // Item doc comment `///`
            if line.starts_with("///") {
                pending_doc.push(strip_prefix(line, "///"));
                i += 1;
                continue;
            }

            // Attributes (#[...]) — skip but don't discard pending_doc
            if line.starts_with("#[") || line.starts_with("#![") {
                i += 1;
                continue;
            }

            // Function definition
            if let Some(fn_name) = parse_fn_name(line) {
                let doc_text = pending_doc.join("\n");
                let examples = ExampleExtractor::extract_from_comment(&doc_text);
                let (params, return_type) = parse_fn_signature(line);
                let vis = if line.contains("pub ") {
                    Visibility::Public
                } else {
                    Visibility::Private
                };
                docs.functions.push(ExtractedFn {
                    name: fn_name,
                    doc_comment: doc_text,
                    signature: line.to_string(),
                    visibility: vis,
                    params,
                    return_type,
                    examples,
                });
                pending_doc.clear();
                i += 1;
                continue;
            }

            // Struct definition
            if let Some(struct_name) = parse_struct_name(line) {
                let doc_text = pending_doc.join("\n");
                let vis = if line.contains("pub ") {
                    Visibility::Public
                } else {
                    Visibility::Private
                };
                let fields = extract_struct_fields(&lines, i);
                docs.structs.push(ExtractedStruct {
                    name: struct_name,
                    doc_comment: doc_text,
                    fields,
                    visibility: vis,
                });
                pending_doc.clear();
                i += 1;
                continue;
            }

            // Enum definition
            if let Some(enum_name) = parse_enum_name(line) {
                let doc_text = pending_doc.join("\n");
                let vis = if line.contains("pub ") {
                    Visibility::Public
                } else {
                    Visibility::Private
                };
                let variants = extract_enum_variants(&lines, i);
                docs.enums.push(ExtractedEnum {
                    name: enum_name,
                    doc_comment: doc_text,
                    variants,
                    visibility: vis,
                });
                pending_doc.clear();
                i += 1;
                continue;
            }

            // Const / static
            if let Some((const_name, const_ty, const_val)) = parse_const(line) {
                let doc_text = pending_doc.join("\n");
                docs.constants.push(ExtractedConst {
                    name: const_name,
                    doc_comment: doc_text,
                    ty: const_ty,
                    value: const_val,
                });
                pending_doc.clear();
                i += 1;
                continue;
            }

            // Any other non-blank line resets pending doc
            if !line.is_empty() {
                pending_doc.clear();
            }

            i += 1;
        }

        docs
    }
}

fn strip_prefix(line: &str, prefix: &str) -> String {
    line.strip_prefix(prefix)
        .map(|s| s.trim_start().to_string())
        .unwrap_or_default()
}

fn parse_fn_name(line: &str) -> Option<String> {
    let stripped = line
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ")
        .trim_start_matches("async ")
        .trim_start_matches("unsafe ");
    if let Some(rest) = stripped.strip_prefix("fn ") {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn parse_struct_name(line: &str) -> Option<String> {
    let stripped = line
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ");
    if let Some(rest) = stripped.strip_prefix("struct ") {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn parse_enum_name(line: &str) -> Option<String> {
    let stripped = line
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ");
    if let Some(rest) = stripped.strip_prefix("enum ") {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn parse_const(line: &str) -> Option<(String, String, String)> {
    let stripped = line
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ");
    if stripped.starts_with("const ") || stripped.starts_with("static ") {
        // const NAME: Type = value;
        if let Some(colon_pos) = stripped.find(':') {
            let name_part = &stripped[stripped.find(' ').unwrap_or(0) + 1..colon_pos];
            let name = name_part.trim().to_string();
            let rest = &stripped[colon_pos + 1..];
            if let Some(eq_pos) = rest.find('=') {
                let ty = rest[..eq_pos].trim().to_string();
                let val = rest[eq_pos + 1..].trim().trim_end_matches(';').to_string();
                if !name.is_empty() {
                    return Some((name, ty, val));
                }
            }
        }
    }
    None
}

fn parse_fn_signature(line: &str) -> (Vec<ExtractedParam>, Option<String>) {
    let mut params = Vec::new();

    // Extract return type after `->`
    let return_type = if let Some(arrow_pos) = line.find("->") {
        let ret = line[arrow_pos + 2..]
            .split('{')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !ret.is_empty() {
            Some(ret)
        } else {
            None
        }
    } else {
        None
    };

    // Extract params between `(` and `)`
    if let (Some(open), Some(close)) = (line.find('('), line.rfind(')')) {
        if open < close {
            let param_str = &line[open + 1..close];
            for part in param_str.split(',') {
                let trimmed = part.trim();
                if trimmed.is_empty()
                    || trimmed == "&self"
                    || trimmed == "&mut self"
                    || trimmed == "self"
                {
                    continue;
                }
                if let Some(colon_pos) = trimmed.find(':') {
                    let name = trimmed[..colon_pos]
                        .trim()
                        .trim_start_matches('&')
                        .trim_start_matches("mut ")
                        .to_string();
                    let ty = trimmed[colon_pos + 1..].trim().to_string();
                    if !name.is_empty() {
                        params.push(ExtractedParam { name, ty });
                    }
                }
            }
        }
    }

    (params, return_type)
}

fn extract_struct_fields(lines: &[&str], start: usize) -> Vec<ExtractedField> {
    let mut fields = Vec::new();
    let mut in_struct = false;
    let mut depth = 0;
    let mut pending_field_doc = Vec::new();

    // Unit structs (`struct Foo;`) and tuple structs without a brace body
    // must not scan subsequent `impl` blocks for fake fields.
    if let Some(header) = lines.get(start) {
        let trimmed = header.trim();
        if trimmed.contains(';') && !trimmed.contains('{') {
            return fields;
        }
    }

    for line in &lines[start..] {
        let trimmed = line.trim();

        if !in_struct {
            if trimmed.contains('{') {
                in_struct = true;
                depth += trimmed.chars().filter(|&c| c == '{').count();
                depth -= trimmed.chars().filter(|&c| c == '}').count();
                if depth == 0 {
                    break;
                }
            } else if trimmed.ends_with(';') {
                // Reached another item without opening a struct body.
                break;
            }
            continue;
        }

        depth += trimmed.chars().filter(|&c| c == '{').count();
        depth -= trimmed.chars().filter(|&c| c == '}').count();

        if depth == 0 {
            break;
        }

        if trimmed.starts_with("///") {
            pending_field_doc.push(strip_prefix(trimmed, "///"));
            continue;
        }

        // Skip nested items / methods accidentally encountered.
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("pub impl ")
        {
            pending_field_doc.clear();
            continue;
        }

        // field: Type,
        if let Some(colon_pos) = trimmed.find(':') {
            let field_part = trimmed[..colon_pos]
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ")
                .trim();
            let type_part = trimmed[colon_pos + 1..]
                .trim()
                .trim_end_matches(',')
                .to_string();
            if !field_part.is_empty()
                && !field_part.starts_with("//")
                && !field_part.contains('(')
                && field_part
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                && field_part.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                fields.push(ExtractedField {
                    name: field_part.to_string(),
                    ty: type_part,
                    doc_comment: pending_field_doc.join("\n"),
                });
            }
        }
        if !trimmed.starts_with("///") {
            pending_field_doc.clear();
        }
    }

    fields
}

fn extract_enum_variants(lines: &[&str], start: usize) -> Vec<ExtractedVariant> {
    let mut variants = Vec::new();
    let mut in_enum = false;
    let mut depth = 0;
    let mut pending_variant_doc = Vec::new();

    for line in &lines[start..] {
        let trimmed = line.trim();

        if !in_enum {
            if trimmed.contains('{') {
                in_enum = true;
                depth += trimmed.chars().filter(|&c| c == '{').count();
                depth -= trimmed.chars().filter(|&c| c == '}').count();
            }
            continue;
        }

        depth += trimmed.chars().filter(|&c| c == '{').count();
        depth -= trimmed.chars().filter(|&c| c == '}').count();

        if depth == 0 {
            break;
        }

        if trimmed.starts_with("///") {
            pending_variant_doc.push(strip_prefix(trimmed, "///"));
            continue;
        }

        // Variant name (possibly followed by `(...)`, `{ ... }`, or just `,`)
        let variant_name: String = trimmed
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !variant_name.is_empty()
            && variant_name
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase())
        {
            variants.push(ExtractedVariant {
                name: variant_name,
                doc_comment: pending_variant_doc.join("\n"),
            });
        }
        if !trimmed.starts_with("///") {
            pending_variant_doc.clear();
        }
    }

    variants
}

// ── Example extractor ─────────────────────────────────────────────────────────

pub struct ExampleExtractor;

impl ExampleExtractor {
    pub fn extract_from_comment(comment: &str) -> Vec<CodeExample> {
        let mut examples = Vec::new();
        let mut in_fence = false;
        let mut lang = String::new();
        let mut code_lines: Vec<String> = Vec::new();

        for line in comment.lines() {
            let trimmed = line.trim();
            if !in_fence {
                if let Some(stripped) = trimmed.strip_prefix("```") {
                    lang = stripped.trim().to_string();
                    if lang.is_empty() {
                        lang = "rust".to_string();
                    }
                    in_fence = true;
                    code_lines.clear();
                }
            } else if trimmed == "```" {
                examples.push(CodeExample {
                    lang: lang.clone(),
                    code: code_lines.join("\n"),
                });
                in_fence = false;
                code_lines.clear();
            } else {
                code_lines.push(line.to_string());
            }
        }

        examples
    }

    pub fn extract_from_file(path: &Path) -> Result<Vec<CodeExample>> {
        let source = fs::read_to_string(path)?;
        let mut examples = Vec::new();

        // Extract all doc comments from the file and pull examples out
        for chunk in source.split("///") {
            let e = Self::extract_from_comment(chunk);
            examples.extend(e);
        }

        Ok(examples)
    }
}

// ── Template engine ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DocTemplate {
    pub name: String,
    content: String,
}

impl DocTemplate {
    pub fn new(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            content: content.to_string(),
        }
    }

    pub fn render(&self, ctx: &HashMap<String, String>) -> String {
        let mut out = self.content.clone();
        for (key, value) in ctx {
            out = out.replace(&format!("{{{{{}}}}}", key), value);
        }
        out
    }
}

pub struct TemplateEngine {
    templates: HashMap<String, DocTemplate>,
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            templates: HashMap::new(),
        };
        engine.load_builtin_templates();
        engine
    }

    fn load_builtin_templates(&mut self) {
        self.templates
            .insert("base".to_string(), DocTemplate::new("base", BASE_TEMPLATE));
        self.templates.insert(
            "contract_page".to_string(),
            DocTemplate::new("contract_page", CONTRACT_PAGE_TEMPLATE),
        );
        self.templates.insert(
            "api_reference".to_string(),
            DocTemplate::new("api_reference", API_REFERENCE_TEMPLATE),
        );
        self.templates.insert(
            "index".to_string(),
            DocTemplate::new("index", INDEX_TEMPLATE),
        );
    }

    pub fn load_from_dir(&mut self, dir: &Path) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("html") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let content = fs::read_to_string(&path)?;
                self.templates
                    .insert(name.clone(), DocTemplate::new(&name, &content));
            }
        }
        Ok(())
    }

    pub fn render(&self, template_name: &str, ctx: &HashMap<String, String>) -> Result<String> {
        let tmpl = self
            .templates
            .get(template_name)
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found", template_name))?;
        Ok(tmpl.render(ctx))
    }
}

// ── HTML documentation generator ─────────────────────────────────────────────

pub struct HtmlDocGenerator {
    engine: TemplateEngine,
}

impl Default for HtmlDocGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlDocGenerator {
    pub fn new() -> Self {
        Self {
            engine: TemplateEngine::new(),
        }
    }

    pub fn with_template_dir(mut self, dir: &Path) -> Result<Self> {
        self.engine.load_from_dir(dir)?;
        Ok(self)
    }

    pub fn generate_site(
        &self,
        docs: &ExtractedDocs,
        contract_name: &str,
        contract_id: &str,
        output_dir: &Path,
    ) -> Result<()> {
        fs::create_dir_all(output_dir)?;

        // Write contract page
        let contract_html = self.generate_contract_page(docs, contract_name, contract_id)?;
        fs::write(output_dir.join("index.html"), &contract_html)?;

        // Write API reference
        let api_html = self.generate_api_reference(docs, contract_name, contract_id)?;
        fs::write(output_dir.join("api.html"), &api_html)?;

        // Write assets (inline CSS)
        fs::write(output_dir.join("style.css"), STYLESHEET)?;

        Ok(())
    }

    pub fn generate_contract_page(
        &self,
        docs: &ExtractedDocs,
        contract_name: &str,
        contract_id: &str,
    ) -> Result<String> {
        let functions_html = docs
            .functions
            .iter()
            .filter(|f| f.visibility == Visibility::Public)
            .map(render_function_card)
            .collect::<Vec<_>>()
            .join("\n");

        let structs_html = docs
            .structs
            .iter()
            .map(render_struct_card)
            .collect::<Vec<_>>()
            .join("\n");

        let enums_html = docs
            .enums
            .iter()
            .map(render_enum_card)
            .collect::<Vec<_>>()
            .join("\n");

        let toc_items: String = docs
            .functions
            .iter()
            .filter(|f| f.visibility == Visibility::Public)
            .map(|f| {
                format!(
                    r##"<li><a href="#fn-{}">{}</a></li>"##,
                    html_id(&f.name),
                    f.name
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let mut ctx = HashMap::new();
        ctx.insert(
            "title".to_string(),
            format!("{} — StarForge Docs", contract_name),
        );
        ctx.insert("contract_name".to_string(), contract_name.to_string());
        ctx.insert("contract_id".to_string(), contract_id.to_string());
        ctx.insert("module_doc".to_string(), escape_html(&docs.module_doc));
        ctx.insert("functions_html".to_string(), functions_html);
        ctx.insert("structs_html".to_string(), structs_html);
        ctx.insert("enums_html".to_string(), enums_html);
        ctx.insert("toc_items".to_string(), toc_items);
        ctx.insert("source_path".to_string(), docs.source_path.clone());

        self.engine.render("contract_page", &ctx)
    }

    pub fn generate_api_reference(
        &self,
        docs: &ExtractedDocs,
        contract_name: &str,
        contract_id: &str,
    ) -> Result<String> {
        let rows: String = docs
            .functions
            .iter()
            .filter(|f| f.visibility == Visibility::Public)
            .map(|f| {
                let param_list = f
                    .params
                    .iter()
                    .map(|p| {
                        format!(
                            "<code>{}: {}</code>",
                            escape_html(&p.name),
                            escape_html(&p.ty)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = f
                    .return_type
                    .as_deref()
                    .map(|r| format!("<code>{}</code>", escape_html(r)))
                    .unwrap_or_else(|| "()".to_string());
                format!(
                    r#"<tr>
  <td><code><a href="index.html#fn-{anchor}">{name}</a></code></td>
  <td>{params}</td>
  <td>{ret}</td>
  <td>{desc}</td>
</tr>"#,
                    anchor = html_id(&f.name),
                    name = escape_html(&f.name),
                    params = param_list,
                    ret = ret,
                    desc = escape_html(f.doc_comment.lines().next().unwrap_or("")),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let mut ctx = HashMap::new();
        ctx.insert(
            "title".to_string(),
            format!("{} API Reference — StarForge Docs", contract_name),
        );
        ctx.insert("contract_name".to_string(), contract_name.to_string());
        ctx.insert("contract_id".to_string(), contract_id.to_string());
        ctx.insert("rows".to_string(), rows);

        self.engine.render("api_reference", &ctx)
    }

    pub fn generate_multi_contract_index(
        &self,
        contracts: &[ContractSummary],
        output_path: &Path,
    ) -> Result<()> {
        let cards: String = contracts
            .iter()
            .map(|c| {
                format!(
                    r#"<div class="card">
  <h3><a href="{dir}/index.html">{name}</a></h3>
  <div class="contract-id">{id}</div>
  <p>{desc}</p>
  <span class="badge">{network}</span>
</div>"#,
                    dir = html_id(&c.contract_id),
                    name = escape_html(&c.name),
                    id = escape_html(&c.contract_id),
                    desc = escape_html(&c.description),
                    network = escape_html(&c.network),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let mut ctx = HashMap::new();
        ctx.insert(
            "title".to_string(),
            "StarForge Contract Documentation".to_string(),
        );
        ctx.insert("cards".to_string(), cards);
        ctx.insert("count".to_string(), contracts.len().to_string());

        let html = self.engine.render("index", &ctx)?;
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output_path, html)?;
        Ok(())
    }
}

// ── API reference types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSummary {
    pub contract_id: String,
    pub name: String,
    pub description: String,
    pub network: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiReference {
    pub contract_id: String,
    pub contract_name: String,
    pub version: String,
    pub functions: Vec<ApiFunction>,
    pub events: Vec<ApiEvent>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFunction {
    pub name: String,
    pub description: String,
    pub params: Vec<ExtractedParam>,
    pub return_type: Option<String>,
    pub examples: Vec<CodeExample>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEvent {
    pub name: String,
    pub description: String,
}

pub struct ApiReferenceGenerator;

impl ApiReferenceGenerator {
    pub fn from_extracted(
        docs: &ExtractedDocs,
        contract_id: &str,
        contract_name: &str,
        version: &str,
    ) -> ApiReference {
        let functions = docs
            .functions
            .iter()
            .map(|f| ApiFunction {
                name: f.name.clone(),
                description: f.doc_comment.clone(),
                params: f.params.clone(),
                return_type: f.return_type.clone(),
                examples: f.examples.clone(),
                visibility: f.visibility.clone(),
            })
            .collect();

        // Soroban events are typically enums; look for enums ending in "Event"
        let events = docs
            .enums
            .iter()
            .filter(|e| e.name.ends_with("Event") || e.name.ends_with("Events"))
            .map(|e| ApiEvent {
                name: e.name.clone(),
                description: e.doc_comment.clone(),
            })
            .collect();

        ApiReference {
            contract_id: contract_id.to_string(),
            contract_name: contract_name.to_string(),
            version: version.to_string(),
            functions,
            events,
            generated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn save_json(api_ref: &ApiReference, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(api_ref)?)?;
        Ok(())
    }

    pub fn render_markdown(api_ref: &ApiReference) -> String {
        let mut md = format!(
            "# {} API Reference\n\n**Contract:** `{}`  \n**Version:** {}  \n**Generated:** {}\n\n",
            api_ref.contract_name,
            api_ref.contract_id,
            api_ref.version,
            &api_ref.generated_at[..10]
        );

        md.push_str("## Functions\n\n");
        for f in &api_ref.functions {
            md.push_str(&format!("### `{}`\n\n", f.name));
            if !f.description.is_empty() {
                md.push_str(&format!("{}\n\n", f.description));
            }
            if !f.params.is_empty() {
                md.push_str("**Parameters:**\n\n");
                for p in &f.params {
                    md.push_str(&format!("- `{}`: `{}`\n", p.name, p.ty));
                }
                md.push('\n');
            }
            if let Some(ref ret) = f.return_type {
                md.push_str(&format!("**Returns:** `{}`\n\n", ret));
            }
            for example in &f.examples {
                md.push_str(&format!("```{}\n{}\n```\n\n", example.lang, example.code));
            }
        }

        if !api_ref.events.is_empty() {
            md.push_str("## Events\n\n");
            for e in &api_ref.events {
                md.push_str(&format!("### `{}`\n\n{}\n\n", e.name, e.description));
            }
        }

        md
    }
}

// ── Doc publisher ─────────────────────────────────────────────────────────────

pub struct DocPublisher;

impl DocPublisher {
    /// Copy a generated docs directory to a destination path.
    pub fn publish_to_dir(source: &Path, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)?;
        Self::copy_dir_recursive(source, dest)?;
        Ok(())
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                fs::create_dir_all(&dst_path)?;
                Self::copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    /// Write a publish manifest file listing all generated files.
    pub fn write_manifest(output_dir: &Path, contract_id: &str, version: &str) -> Result<PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(output_dir)? {
            let entry = entry?;
            files.push(entry.file_name().to_string_lossy().to_string());
        }

        let manifest = serde_json::json!({
            "contract_id": contract_id,
            "version": version,
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "files": files,
        });

        let manifest_path = output_dir.join("manifest.json");
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
        Ok(manifest_path)
    }

    /// Generate a shell script that can be used to upload docs to a static host.
    pub fn generate_deploy_script(output_dir: &Path, endpoint: &str) -> Result<PathBuf> {
        let script = format!(
            "#!/usr/bin/env bash\n\
             # Auto-generated StarForge documentation deploy script\n\
             set -euo pipefail\n\n\
             OUTPUT_DIR=\"{dir}\"\n\
             ENDPOINT=\"{ep}\"\n\n\
             echo \"Deploying docs from $OUTPUT_DIR to $ENDPOINT\"\n\
             rsync -avz --delete \"$OUTPUT_DIR/\" \"$ENDPOINT\"\n\
             echo \"Done.\"\n",
            dir = output_dir.display(),
            ep = endpoint,
        );
        let script_path = output_dir.join("deploy.sh");
        fs::write(&script_path, &script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))?;
        }
        Ok(script_path)
    }
}

// ── HTML rendering helpers ────────────────────────────────────────────────────

fn html_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_function_card(f: &ExtractedFn) -> String {
    let params_html = if f.params.is_empty() {
        String::new()
    } else {
        let rows = f
            .params
            .iter()
            .map(|p| {
                format!(
                    r#"<tr><td><code>{}</code></td><td><code>{}</code></td></tr>"#,
                    escape_html(&p.name),
                    escape_html(&p.ty)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"<div class="params"><h4>Parameters</h4><table><thead><tr><th>Name</th><th>Type</th></tr></thead><tbody>{rows}</tbody></table></div>"#,
        )
    };

    let return_html = f
        .return_type
        .as_deref()
        .map(|r| {
            format!(
                r#"<div class="returns"><h4>Returns</h4><code>{}</code></div>"#,
                escape_html(r)
            )
        })
        .unwrap_or_default();

    let examples_html = f
        .examples
        .iter()
        .map(|ex| {
            format!(
                r#"<pre><code class="lang-{}">{}</code></pre>"#,
                escape_html(&ex.lang),
                escape_html(&ex.code)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let examples_section = if examples_html.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="examples"><h4>Examples</h4>{examples_html}</div>"#)
    };

    let doc_html = f
        .doc_comment
        .lines()
        .filter(|l| !l.starts_with("```"))
        .map(escape_html)
        .collect::<Vec<_>>()
        .join("<br>");

    format!(
        r#"<div class="fn-card" id="fn-{id}">
  <div class="fn-header">
    <span class="fn-name">{name}</span>
    <span class="fn-sig"><code>{sig}</code></span>
  </div>
  <div class="fn-doc">{doc}</div>
  {params}
  {ret}
  {examples}
</div>"#,
        id = html_id(&f.name),
        name = escape_html(&f.name),
        sig = escape_html(&f.signature),
        doc = doc_html,
        params = params_html,
        ret = return_html,
        examples = examples_section,
    )
}

fn render_struct_card(s: &ExtractedStruct) -> String {
    let fields_html = s
        .fields
        .iter()
        .map(|f| {
            format!(
                r#"<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>"#,
                escape_html(&f.name),
                escape_html(&f.ty),
                escape_html(&f.doc_comment),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="struct-card">
  <div class="struct-header"><span class="struct-name">{name}</span></div>
  <div class="struct-doc">{doc}</div>
  <table><thead><tr><th>Field</th><th>Type</th><th>Description</th></tr></thead>
  <tbody>{fields}</tbody></table>
</div>"#,
        name = escape_html(&s.name),
        doc = escape_html(&s.doc_comment),
        fields = fields_html,
    )
}

fn render_enum_card(e: &ExtractedEnum) -> String {
    let variants_html = e
        .variants
        .iter()
        .map(|v| {
            format!(
                r#"<tr><td><code>{}</code></td><td>{}</td></tr>"#,
                escape_html(&v.name),
                escape_html(&v.doc_comment),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="enum-card">
  <div class="enum-header"><span class="enum-name">{name}</span></div>
  <div class="enum-doc">{doc}</div>
  <table><thead><tr><th>Variant</th><th>Description</th></tr></thead>
  <tbody>{variants}</tbody></table>
</div>"#,
        name = escape_html(&e.name),
        doc = escape_html(&e.doc_comment),
        variants = variants_html,
    )
}

// ── Embedded templates ────────────────────────────────────────────────────────

const STYLESHEET: &str = r#"
:root {
  --bg: #0f1117;
  --surface: #1a1d27;
  --border: #2a2d3a;
  --accent: #7c6af7;
  --accent-light: #a89bf7;
  --text: #e2e4ed;
  --text-muted: #8b8fa8;
  --code-bg: #12141f;
  --success: #3ecf8e;
  --warning: #f6a623;
  --font-mono: 'JetBrains Mono', 'Fira Code', 'Courier New', monospace;
  --font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body { background: var(--bg); color: var(--text); font-family: var(--font-sans); line-height: 1.6; }
a { color: var(--accent-light); text-decoration: none; }
a:hover { text-decoration: underline; }
code, pre { font-family: var(--font-mono); }

/* Layout */
.layout { display: grid; grid-template-columns: 260px 1fr; min-height: 100vh; }
.sidebar { background: var(--surface); border-right: 1px solid var(--border); padding: 2rem 1rem; position: sticky; top: 0; height: 100vh; overflow-y: auto; }
.sidebar h2 { font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.1em; color: var(--text-muted); margin-bottom: 0.75rem; margin-top: 1.5rem; }
.sidebar ul { list-style: none; }
.sidebar li { margin: 0.25rem 0; }
.sidebar li a { color: var(--text-muted); font-size: 0.875rem; display: block; padding: 0.25rem 0.5rem; border-radius: 4px; }
.sidebar li a:hover { color: var(--text); background: var(--border); text-decoration: none; }
.main { padding: 2.5rem 3rem; max-width: 960px; }

/* Header */
.page-header { border-bottom: 1px solid var(--border); padding-bottom: 1.5rem; margin-bottom: 2rem; }
.page-header h1 { font-size: 2rem; font-weight: 700; }
.page-header .contract-id { font-family: var(--font-mono); color: var(--text-muted); font-size: 0.875rem; margin-top: 0.25rem; }
.page-header .module-doc { color: var(--text-muted); margin-top: 0.75rem; }

/* Section */
.section { margin: 2.5rem 0; }
.section h2 { font-size: 1.25rem; color: var(--accent-light); border-bottom: 1px solid var(--border); padding-bottom: 0.5rem; margin-bottom: 1.25rem; }

/* Function card */
.fn-card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 1.5rem; margin: 1rem 0; }
.fn-header { display: flex; align-items: baseline; gap: 1rem; margin-bottom: 0.75rem; flex-wrap: wrap; }
.fn-name { font-size: 1.1rem; font-weight: 700; font-family: var(--font-mono); color: var(--accent-light); }
.fn-sig code { font-size: 0.75rem; color: var(--text-muted); }
.fn-doc { color: var(--text-muted); margin-bottom: 1rem; font-size: 0.9rem; }
.params, .returns, .examples { margin-top: 1rem; }
.params h4, .returns h4, .examples h4 { font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.08em; color: var(--text-muted); margin-bottom: 0.5rem; }

/* Tables */
table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
th { text-align: left; color: var(--text-muted); font-weight: 600; padding: 0.4rem 0.75rem; border-bottom: 1px solid var(--border); }
td { padding: 0.4rem 0.75rem; border-bottom: 1px solid var(--border); color: var(--text); }
tr:last-child td { border-bottom: none; }

/* Code blocks */
pre { background: var(--code-bg); border: 1px solid var(--border); border-radius: 6px; padding: 1rem; overflow-x: auto; }
pre code { font-size: 0.85rem; color: var(--text); }
code { background: var(--code-bg); padding: 0.1em 0.3em; border-radius: 3px; font-size: 0.875em; }

/* Cards (index) */
.card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 1.5rem; margin-top: 1.5rem; }
.card { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 1.5rem; transition: border-color 0.2s; }
.card:hover { border-color: var(--accent); }
.card h3 { margin-bottom: 0.5rem; }
.card .contract-id { font-family: var(--font-mono); font-size: 0.75rem; color: var(--text-muted); margin-bottom: 0.75rem; }
.card p { color: var(--text-muted); font-size: 0.875rem; }
.badge { display: inline-block; background: rgba(124,106,247,0.15); color: var(--accent-light); border: 1px solid rgba(124,106,247,0.3); border-radius: 4px; padding: 0.15rem 0.6rem; font-size: 0.75rem; margin-top: 0.75rem; }

/* Struct / enum cards */
.struct-card, .enum-card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 1.25rem; margin: 0.75rem 0; }
.struct-header, .enum-header { margin-bottom: 0.5rem; }
.struct-name, .enum-name { font-family: var(--font-mono); font-weight: 700; color: var(--success); }
.struct-doc, .enum-doc { color: var(--text-muted); font-size: 0.875rem; margin-bottom: 0.75rem; }

/* Search */
.search-bar { margin: 1.5rem 0; }
.search-bar input { width: 100%; background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 0.75rem 1rem; color: var(--text); font-size: 0.95rem; outline: none; }
.search-bar input:focus { border-color: var(--accent); }
"#;

const BASE_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{{title}}</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
{{body}}
</body>
</html>"#;

const CONTRACT_PAGE_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{{title}}</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<div class="layout">
  <nav class="sidebar">
    <div style="margin-bottom:1.5rem;">
      <a href="../../index.html" style="color:var(--text-muted);font-size:0.85rem;">← All Contracts</a>
    </div>
    <div style="font-weight:700;font-size:1rem;color:var(--text);">{{contract_name}}</div>
    <div style="font-family:monospace;font-size:0.7rem;color:var(--text-muted);margin-top:0.25rem;word-break:break-all;">{{contract_id}}</div>
    <h2 style="margin-top:2rem;">Functions</h2>
    <ul>{{toc_items}}</ul>
    <h2>Reference</h2>
    <ul>
      <li><a href="api.html">API Reference</a></li>
    </ul>
  </nav>
  <main class="main">
    <div class="page-header">
      <h1>{{contract_name}}</h1>
      <div class="contract-id">{{contract_id}}</div>
      <div class="module-doc">{{module_doc}}</div>
    </div>
    <div class="section">
      <h2>Functions</h2>
      {{functions_html}}
    </div>
    <div class="section">
      <h2>Types</h2>
      {{structs_html}}
      {{enums_html}}
    </div>
  </main>
</div>
</body>
</html>"#;

const API_REFERENCE_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{{title}}</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<div class="layout">
  <nav class="sidebar">
    <div style="margin-bottom:1.5rem;">
      <a href="index.html" style="color:var(--text-muted);font-size:0.85rem;">← Back to docs</a>
    </div>
    <div style="font-weight:700;font-size:1rem;color:var(--text);">{{contract_name}}</div>
    <div style="font-family:monospace;font-size:0.7rem;color:var(--text-muted);margin-top:0.25rem;word-break:break-all;">{{contract_id}}</div>
  </nav>
  <main class="main">
    <div class="page-header">
      <h1>API Reference — {{contract_name}}</h1>
      <div class="contract-id">{{contract_id}}</div>
    </div>
    <div class="section">
      <table>
        <thead><tr><th>Function</th><th>Parameters</th><th>Returns</th><th>Description</th></tr></thead>
        <tbody>{{rows}}</tbody>
      </table>
    </div>
  </main>
</div>
</body>
</html>"#;

const INDEX_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{{title}}</title>
<link rel="stylesheet" href="style.css">
</head>
<body style="padding:2.5rem 3rem;max-width:1200px;margin:0 auto;">
  <div class="page-header">
    <h1>&#9889; StarForge Contract Docs</h1>
    <p style="color:var(--text-muted);margin-top:0.5rem;">{{count}} contracts documented</p>
  </div>
  <div class="search-bar">
    <input type="text" id="search" placeholder="Search contracts…" oninput="filter()">
  </div>
  <div class="card-grid" id="grid">
    {{cards}}
  </div>
  <script>
    function filter() {
      const q = document.getElementById('search').value.toLowerCase();
      document.querySelectorAll('.card').forEach(c => {
        c.style.display = c.textContent.toLowerCase().includes(q) ? '' : 'none';
      });
    }
  </script>
</body>
</html>"#;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const SAMPLE_SOURCE: &str = r#"
//! Token contract for Stellar Soroban.
//! Implements basic ERC-20-style transfers.

/// Initialise the contract with an admin address.
///
/// # Examples
/// ```rust
/// contract.initialize(&env, &admin);
/// ```
pub fn initialize(env: Env, admin: Address) -> bool {
    true
}

/// Transfer tokens between two accounts.
///
/// # Examples
/// ```rust
/// contract.transfer(&env, &from, &to, 1000_i128);
/// ```
pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> bool {
    true
}

/// Private helper — not exposed in docs.
fn internal_helper(value: u32) {}

/// Storage keys for the contract.
pub enum StorageKey {
    /// Admin address key.
    Admin,
    /// Balance map key.
    Balance,
}

/// Contract configuration.
pub struct Config {
    /// Max tokens in circulation.
    pub max_supply: i128,
    /// Token name.
    pub name: String,
}
"#;

    #[test]
    fn extracts_module_doc() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        assert!(docs.module_doc.contains("Token contract"));
    }

    #[test]
    fn extracts_public_functions() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let names: Vec<&str> = docs.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"initialize"));
        assert!(names.contains(&"transfer"));
        assert!(names.contains(&"internal_helper"));
    }

    #[test]
    fn extracts_visibility() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let init = docs
            .functions
            .iter()
            .find(|f| f.name == "initialize")
            .unwrap();
        assert_eq!(init.visibility, Visibility::Public);
        let helper = docs
            .functions
            .iter()
            .find(|f| f.name == "internal_helper")
            .unwrap();
        assert_eq!(helper.visibility, Visibility::Private);
    }

    #[test]
    fn extracts_params() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let transfer = docs
            .functions
            .iter()
            .find(|f| f.name == "transfer")
            .unwrap();
        let param_names: Vec<&str> = transfer.params.iter().map(|p| p.name.as_str()).collect();
        assert!(param_names.contains(&"from"));
        assert!(param_names.contains(&"to"));
        assert!(param_names.contains(&"amount"));
    }

    #[test]
    fn extracts_code_examples() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let init = docs
            .functions
            .iter()
            .find(|f| f.name == "initialize")
            .unwrap();
        assert_eq!(init.examples.len(), 1);
        assert!(init.examples[0].code.contains("initialize"));
    }

    #[test]
    fn extracts_structs_and_enums() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let struct_names: Vec<&str> = docs.structs.iter().map(|s| s.name.as_str()).collect();
        let enum_names: Vec<&str> = docs.enums.iter().map(|e| e.name.as_str()).collect();
        assert!(struct_names.contains(&"Config"));
        assert!(enum_names.contains(&"StorageKey"));
    }

    #[test]
    fn template_engine_renders() {
        let engine = TemplateEngine::new();
        let mut ctx = HashMap::new();
        ctx.insert("title".to_string(), "My Contract".to_string());
        ctx.insert("contract_name".to_string(), "MyContract".to_string());
        ctx.insert("contract_id".to_string(), "CABC123".to_string());
        ctx.insert("module_doc".to_string(), String::new());
        ctx.insert("functions_html".to_string(), String::new());
        ctx.insert("structs_html".to_string(), String::new());
        ctx.insert("enums_html".to_string(), String::new());
        ctx.insert("toc_items".to_string(), String::new());
        ctx.insert("source_path".to_string(), String::new());
        let html = engine.render("contract_page", &ctx).unwrap();
        assert!(html.contains("MyContract"));
        assert!(html.contains("CABC123"));
    }

    #[test]
    fn html_generator_creates_files() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let tmp = tempdir().unwrap();
        let generator = HtmlDocGenerator::new();
        generator
            .generate_site(&docs, "TestContract", "CABC123", tmp.path())
            .unwrap();
        assert!(tmp.path().join("index.html").exists());
        assert!(tmp.path().join("api.html").exists());
        assert!(tmp.path().join("style.css").exists());
    }

    #[test]
    fn api_reference_generation() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let api = ApiReferenceGenerator::from_extracted(&docs, "CABC123", "TestContract", "1.0.0");
        assert_eq!(api.functions.len(), docs.functions.len());
        let md = ApiReferenceGenerator::render_markdown(&api);
        assert!(md.contains("initialize"));
        assert!(md.contains("transfer"));
    }

    #[test]
    fn publisher_copies_dir() {
        let docs = DocCommentExtractor::extract_from_source(SAMPLE_SOURCE);
        let src_tmp = tempdir().unwrap();
        let dst_tmp = tempdir().unwrap();
        let generator = HtmlDocGenerator::new();
        generator
            .generate_site(&docs, "TestContract", "CABC123", src_tmp.path())
            .unwrap();
        DocPublisher::publish_to_dir(src_tmp.path(), dst_tmp.path()).unwrap();
        assert!(dst_tmp.path().join("index.html").exists());
    }

    #[test]
    fn publisher_writes_manifest() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("index.html"), "<html/>").unwrap();
        let manifest_path = DocPublisher::write_manifest(tmp.path(), "CABC123", "1.0.0").unwrap();
        assert!(manifest_path.exists());
        let content = fs::read_to_string(&manifest_path).unwrap();
        assert!(content.contains("CABC123"));
    }

    #[test]
    fn example_extractor_parses_fenced_blocks() {
        let comment = "Transfer tokens.\n\n```rust\ncontract.transfer();\n```";
        let examples = ExampleExtractor::extract_from_comment(comment);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].lang, "rust");
        assert!(examples[0].code.contains("transfer"));
    }
}
