//! Offline, deterministic code navigation for Rust/Soroban projects.
//!
//! The index deliberately uses lightweight source analysis instead of requiring
//! rust-analyzer, which makes it suitable for CI and partially compiling trees.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Module,
    Constant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub line: usize,
    pub public: bool,
    pub signature: String,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub symbol: String,
    pub file: PathBuf,
    pub line: usize,
    pub context: String,
    pub is_definition: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub source: String,
    pub target: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeGraph {
    pub root: PathBuf,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub calls: Vec<CallEdge>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub symbol: Symbol,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationContext {
    pub symbol: Symbol,
    pub definitions: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub callers: Vec<CallEdge>,
    pub callees: Vec<CallEdge>,
    pub related: Vec<Symbol>,
}

pub fn index_project(root: &Path) -> Result<CodeGraph> {
    let root = root
        .canonicalize()
        .with_context(|| format!("Project directory does not exist: {}", root.display()))?;
    let files = rust_files(&root)?;
    let mut graph = CodeGraph {
        root: root.clone(),
        ..CodeGraph::default()
    };
    let mut contents = BTreeMap::new();

    for file in files {
        let content = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read {}", file.display()))?;
        graph.symbols.extend(parse_symbols(&root, &file, &content));
        graph
            .dependencies
            .extend(parse_dependencies(&root, &file, &content));
        contents.insert(file, content);
    }

    let names: BTreeSet<String> = graph.symbols.iter().map(|s| s.name.clone()).collect();
    for (file, content) in &contents {
        graph.references.extend(parse_references(
            &root,
            file,
            content,
            &graph.symbols,
            &names,
        ));
        graph
            .calls
            .extend(parse_calls(&root, file, content, &graph.symbols, &names));
    }
    graph.symbols.sort_by(|a, b| {
        a.qualified_name
            .cmp(&b.qualified_name)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    Ok(graph)
}

pub fn definitions(graph: &CodeGraph, name: &str) -> Vec<Symbol> {
    graph
        .symbols
        .iter()
        .filter(|s| s.name == name || s.qualified_name == name)
        .cloned()
        .collect()
}

pub fn find_references(graph: &CodeGraph, name: &str, include_definition: bool) -> Vec<Reference> {
    graph
        .references
        .iter()
        .filter(|r| r.symbol == name && (include_definition || !r.is_definition))
        .cloned()
        .collect()
}

pub fn smart_search(graph: &CodeGraph, query: &str, limit: usize) -> Vec<SearchHit> {
    let query_lower = query.to_lowercase();
    let tokens: Vec<&str> = query_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .collect();
    let mut hits = Vec::new();

    for symbol in &graph.symbols {
        let name = symbol.name.to_lowercase();
        let signature = symbol.signature.to_lowercase();
        let docs = symbol
            .documentation
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        let mut score = 0.0_f64;
        let mut reasons = Vec::new();
        if name == query_lower {
            score += 1.0;
            reasons.push("exact symbol name");
        } else if name.contains(&query_lower) || query_lower.contains(&name) {
            score += 0.7;
            reasons.push("partial symbol name");
        }
        for token in &tokens {
            if name.contains(token) {
                score += 0.25;
            }
            if signature.contains(token) {
                score += 0.12;
            }
            if docs.contains(token) {
                score += 0.08;
            }
        }
        if score > 0.0 {
            hits.push(SearchHit {
                symbol: symbol.clone(),
                score: score.min(1.0),
                reason: if reasons.is_empty() {
                    "signature or documentation context".into()
                } else {
                    reasons.join(", ")
                },
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.symbol.qualified_name.cmp(&b.symbol.qualified_name))
    });
    hits.truncate(limit);
    hits
}

pub fn context(graph: &CodeGraph, name: &str) -> Option<NavigationContext> {
    let symbol = definitions(graph, name).into_iter().next()?;
    let callers: Vec<_> = graph
        .calls
        .iter()
        .filter(|c| c.callee == symbol.name)
        .cloned()
        .collect();
    let callees: Vec<_> = graph
        .calls
        .iter()
        .filter(|c| c.caller == symbol.name)
        .cloned()
        .collect();
    let related_names: BTreeSet<_> = callers
        .iter()
        .map(|c| c.caller.as_str())
        .chain(callees.iter().map(|c| c.callee.as_str()))
        .collect();
    let related = graph
        .symbols
        .iter()
        .filter(|s| related_names.contains(s.name.as_str()))
        .cloned()
        .collect();

    Some(NavigationContext {
        definitions: definitions(graph, name),
        references: find_references(graph, &symbol.name, false),
        callers,
        callees,
        related,
        symbol,
    })
}

pub fn render_dependency_tree(graph: &CodeGraph) -> String {
    let mut grouped: BTreeMap<&str, Vec<&Dependency>> = BTreeMap::new();
    for dependency in &graph.dependencies {
        grouped
            .entry(&dependency.source)
            .or_default()
            .push(dependency);
    }
    let mut out = format!("{}\n", graph.root.display());
    for (source, dependencies) in grouped {
        out.push_str(&format!("├── {}\n", source));
        for (index, dependency) in dependencies.iter().enumerate() {
            let branch = if index + 1 == dependencies.len() {
                "└──"
            } else {
                "├──"
            };
            out.push_str(&format!(
                "│   {} {} ({})\n",
                branch, dependency.target, dependency.kind
            ));
        }
    }
    out
}

pub fn render_call_hierarchy(graph: &CodeGraph, entry: &str, max_depth: usize) -> String {
    fn walk(
        graph: &CodeGraph,
        current: &str,
        depth: usize,
        max_depth: usize,
        seen: &mut BTreeSet<String>,
        out: &mut String,
    ) {
        out.push_str(&format!("{}{}\n", "  ".repeat(depth), current));
        if depth >= max_depth || !seen.insert(current.to_string()) {
            return;
        }
        let mut children: Vec<_> = graph
            .calls
            .iter()
            .filter(|edge| edge.caller == current)
            .map(|edge| edge.callee.clone())
            .collect();
        children.sort();
        children.dedup();
        for child in children {
            walk(graph, &child, depth + 1, max_depth, seen, out);
        }
        seen.remove(current);
    }

    let mut out = String::new();
    walk(graph, entry, 0, max_depth, &mut BTreeSet::new(), &mut out);
    out
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(name, "target" | ".git" | "node_modules" | "vendor") {
                    visit(&path, files)?;
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn parse_symbols(root: &Path, file: &Path, source: &str) -> Vec<Symbol> {
    let relative = file.strip_prefix(root).unwrap_or(file).to_path_buf();
    let module = relative
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("::");
    let mut docs = Vec::new();
    let mut symbols = Vec::new();

    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(doc) = trimmed.strip_prefix("///") {
            docs.push(doc.trim().to_string());
            continue;
        }
        if trimmed.starts_with("#[") || trimmed.is_empty() {
            continue;
        }
        if let Some((kind, name, public)) = declaration(trimmed) {
            symbols.push(Symbol {
                qualified_name: format!("{}::{}", module, name),
                name,
                kind,
                file: relative.clone(),
                line: index + 1,
                public,
                signature: trimmed.to_string(),
                documentation: (!docs.is_empty()).then(|| docs.join(" ")),
            });
        }
        docs.clear();
    }
    symbols
}

fn declaration(line: &str) -> Option<(SymbolKind, String, bool)> {
    let public = line.starts_with("pub ");
    let normalized = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line)
        .trim_start_matches("async ")
        .trim_start_matches("unsafe ");
    let declarations = [
        ("fn ", SymbolKind::Function),
        ("struct ", SymbolKind::Struct),
        ("enum ", SymbolKind::Enum),
        ("trait ", SymbolKind::Trait),
        ("mod ", SymbolKind::Module),
        ("const ", SymbolKind::Constant),
        ("static ", SymbolKind::Constant),
    ];
    for (prefix, kind) in declarations {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let name = rest
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                return Some((kind, name.to_string(), public));
            }
        }
    }
    None
}

fn parse_references(
    root: &Path,
    file: &Path,
    source: &str,
    symbols: &[Symbol],
    names: &BTreeSet<String>,
) -> Vec<Reference> {
    let relative = file.strip_prefix(root).unwrap_or(file).to_path_buf();
    let definitions: HashMap<(usize, &str), bool> = symbols
        .iter()
        .filter(|s| s.file == relative)
        .map(|s| ((s.line, s.name.as_str()), true))
        .collect();
    let mut references = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let tokens = identifiers(line);
        for name in tokens.into_iter().filter(|name| names.contains(*name)) {
            references.push(Reference {
                symbol: name.to_string(),
                file: relative.clone(),
                line: index + 1,
                context: line.trim().to_string(),
                is_definition: definitions.contains_key(&(index + 1, name)),
            });
        }
    }
    references
}

fn parse_calls(
    root: &Path,
    file: &Path,
    source: &str,
    symbols: &[Symbol],
    names: &BTreeSet<String>,
) -> Vec<CallEdge> {
    let relative = file.strip_prefix(root).unwrap_or(file).to_path_buf();
    let mut function_lines: Vec<_> = symbols
        .iter()
        .filter(|s| s.file == relative && s.kind == SymbolKind::Function)
        .map(|s| (s.line, s.name.clone()))
        .collect();
    function_lines.sort();
    let mut calls = Vec::new();
    let mut current = None::<String>;
    let mut function_depth = 0_i32;
    let mut function_opened = false;

    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        if let Some((_, function)) = function_lines.iter().find(|(line, _)| *line == line_number) {
            current = Some(function.clone());
            function_depth = 0;
            function_opened = false;
        }
        if let Some(caller) = &current {
            for token in identifiers_followed_by_paren(line) {
                if names.contains(token.as_str())
                    && token != *caller
                    && !matches!(token.as_str(), "if" | "while" | "for" | "match")
                {
                    calls.push(CallEdge {
                        caller: caller.clone(),
                        callee: token,
                        file: relative.clone(),
                        line: line_number,
                    });
                }
            }
            let opens = line.matches('{').count() as i32;
            let closes = line.matches('}').count() as i32;
            function_opened |= opens > 0;
            function_depth += opens - closes;
            if function_opened && function_depth <= 0 {
                current = None;
            }
        }
    }
    calls.sort_by(|a, b| {
        a.caller
            .cmp(&b.caller)
            .then(a.callee.cmp(&b.callee))
            .then(a.line.cmp(&b.line))
    });
    calls.dedup_by(|a, b| {
        a.caller == b.caller && a.callee == b.callee && a.file == b.file && a.line == b.line
    });
    calls
}

fn parse_dependencies(root: &Path, file: &Path, source: &str) -> Vec<Dependency> {
    let relative = file
        .strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string();
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (kind, rest) = if let Some(rest) = trimmed.strip_prefix("use ") {
                ("use", rest)
            } else {
                let rest = trimmed.strip_prefix("mod ")?;
                ("module", rest)
            };
            let target = rest
                .trim_end_matches(';')
                .split("::{")
                .next()
                .unwrap_or(rest)
                .to_string();
            Some(Dependency {
                source: relative.clone(),
                target,
                kind: kind.into(),
            })
        })
        .collect()
}

fn identifiers(line: &str) -> Vec<&str> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .collect()
}

fn identifiers_followed_by_paren(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut results = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '(' {
            continue;
        }
        let mut end = index;
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        if start < end {
            results.push(chars[start..end].iter().collect());
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_definitions_references_and_calls() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("lib.rs");
        fs::write(
            &source,
            "/// Adds one.\npub fn add(x: u32) -> u32 { helper(x) }\nfn helper(x: u32) -> u32 { x + 1 }\n",
        )
        .unwrap();
        let graph = index_project(temp.path()).unwrap();
        assert_eq!(definitions(&graph, "add").len(), 1);
        assert!(find_references(&graph, "helper", false).len() >= 1);
        assert!(graph
            .calls
            .iter()
            .any(|edge| edge.caller == "add" && edge.callee == "helper"));
        assert_eq!(smart_search(&graph, "adds one", 5)[0].symbol.name, "add");
    }

    #[test]
    fn renders_call_hierarchy_without_looping_on_cycles() {
        let graph = CodeGraph {
            calls: vec![
                CallEdge {
                    caller: "a".into(),
                    callee: "b".into(),
                    file: "lib.rs".into(),
                    line: 1,
                },
                CallEdge {
                    caller: "b".into(),
                    callee: "a".into(),
                    file: "lib.rs".into(),
                    line: 2,
                },
            ],
            ..CodeGraph::default()
        };
        let rendered = render_call_hierarchy(&graph, "a", 5);
        assert_eq!(rendered.lines().count(), 3);
    }
}
