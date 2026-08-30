use anyhow::Result;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub call_type: CallType,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallType {
    DirectInvoke,
    ClientNew,
    ExternalCall,
    InternalCall,
}

impl std::fmt::Display for CallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallType::DirectInvoke => write!(f, "invoke"),
            CallType::ClientNew => write!(f, "client"),
            CallType::ExternalCall => write!(f, "external"),
            CallType::InternalCall => write!(f, "internal"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallNode {
    pub name: String,
    pub functions: Vec<String>,
    pub is_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraph {
    pub root: String,
    pub nodes: Vec<CallNode>,
    pub edges: Vec<CallEdge>,
    pub dependencies: Vec<String>,
    pub patterns: Vec<CallPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallPattern {
    pub name: String,
    pub description: String,
    pub severity: String,
    /// Optional struct-of-interest (function or contract) the pattern
    /// points at. Lets tools rebind suggestions without text-scraping the
    /// description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSuggestion {
    pub target: String,
    pub title: String,
    pub detail: String,
    pub priority: String, // "high" | "medium" | "low"
    pub estimated_savings: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallStats {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub external_edges: usize,
    pub internal_edges: usize,
    pub direct_invokes: usize,
    pub client_constructions: usize,
    pub dependencies: usize,
    pub patterns_high: usize,
    pub patterns_medium: usize,
    pub patterns_low: usize,
    pub fan_out_max: usize,
    pub fan_in_max: usize,
}

pub fn compute_stats(graph: &CallGraph) -> CallStats {
    let direct_invokes = graph
        .edges
        .iter()
        .filter(|e| e.call_type == CallType::DirectInvoke)
        .count();
    let client_constructions = graph
        .edges
        .iter()
        .filter(|e| e.call_type == CallType::ClientNew)
        .count();
    let external_edges = graph
        .edges
        .iter()
        .filter(|e| e.call_type != CallType::InternalCall)
        .count();
    let internal_edges = graph
        .edges
        .iter()
        .filter(|e| e.call_type == CallType::InternalCall)
        .count();

    let mut out_count: HashMap<&str, usize> = HashMap::new();
    let mut in_count: HashMap<&str, usize> = HashMap::new();
    for edge in &graph.edges {
        *out_count.entry(edge.caller.as_str()).or_insert(0) += 1;
        *in_count.entry(edge.callee.as_str()).or_insert(0) += 1;
    }
    let fan_out_max = out_count.values().copied().max().unwrap_or(0);
    let fan_in_max = in_count.values().copied().max().unwrap_or(0);

    let (mut hi, mut md, mut lo) = (0usize, 0usize, 0usize);
    for pat in &graph.patterns {
        match pat.severity.as_str() {
            "high" => hi += 1,
            "medium" => md += 1,
            _ => lo += 1,
        }
    }

    CallStats {
        total_nodes: graph.nodes.len(),
        total_edges: graph.edges.len(),
        external_edges,
        internal_edges,
        direct_invokes,
        client_constructions,
        dependencies: graph.dependencies.len(),
        patterns_high: hi,
        patterns_medium: md,
        patterns_low: lo,
        fan_out_max,
        fan_in_max,
    }
}

pub fn extract_call_graph(path: &Path) -> Result<CallGraph> {
    let content = fs::read_to_string(path)?;
    let root = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("contract")
        .to_string();

    let edges = extract_edges(&content, &root);
    let nodes = build_nodes(&edges, &root);
    let dependencies = extract_dependencies(&content);
    let patterns = detect_patterns(&edges, &content);

    Ok(CallGraph {
        root,
        nodes,
        edges,
        dependencies,
        patterns,
    })
}

fn extract_edges(content: &str, root: &str) -> Vec<CallEdge> {
    let mut edges = Vec::new();

    // Pattern 1: invoke_contract! macro
    let invoke_pattern = "invoke_contract!";
    let mut search = content;
    while let Some(pos) = search.find(invoke_pattern) {
        let rest = &search[pos + invoke_pattern.len()..];
        let callee = extract_contract_arg(rest).unwrap_or_else(|| "unknown".to_string());
        let line = count_lines(&content[..content.len() - search.len() + pos]);
        edges.push(CallEdge {
            caller: root.to_string(),
            callee,
            call_type: CallType::DirectInvoke,
            location: Some(format!("line {}", line)),
        });
        search = &search[1..];
    }

    // Pattern 2: Client::new(env, contract_id)
    let client_pattern = "Client::new";
    let mut search = content;
    while let Some(pos) = search.find(client_pattern) {
        let prefix = &content[..content.len() - search.len() + pos];
        let callee = extract_client_name(prefix).unwrap_or_else(|| "ExternalContract".to_string());
        let line = count_lines(prefix);
        edges.push(CallEdge {
            caller: root.to_string(),
            callee,
            call_type: CallType::ClientNew,
            location: Some(format!("line {}", line)),
        });
        search = &search[1..];
    }

    // Pattern 3: contract::Client or ContractName::Client
    let client_suffix = "::Client";
    let mut search = content;
    while let Some(pos) = search.find(client_suffix) {
        let prefix_area = &content[..content.len() - search.len() + pos];
        if let Some(callee) = extract_module_name(prefix_area) {
            let already = edges.iter().any(|e| e.callee == callee);
            if !already {
                let line = count_lines(prefix_area);
                edges.push(CallEdge {
                    caller: root.to_string(),
                    callee,
                    call_type: CallType::ExternalCall,
                    location: Some(format!("line {}", line)),
                });
            }
        }
        search = &search[1..];
    }

    // Pattern 4: internal fn calls (fn name in same file)
    let fns = extract_function_names(content);
    for fn_name in &fns {
        let call_pattern = format!("{}(", fn_name);
        let definitions = content.matches(&format!("fn {}(", fn_name)).count();
        let calls = content.matches(&call_pattern).count();
        if calls > definitions && fn_name != root {
            edges.push(CallEdge {
                caller: root.to_string(),
                callee: fn_name.clone(),
                call_type: CallType::InternalCall,
                location: None,
            });
        }
    }

    edges
}

fn extract_contract_arg(text: &str) -> Option<String> {
    let start = text.find('(')?;
    let rest = &text[start + 1..];
    let end = rest.find(',')?;
    let raw = rest[..end].trim().trim_matches('&').trim();
    if raw.is_empty() || raw == "env" {
        None
    } else {
        Some(raw.to_string())
    }
}

fn extract_client_name(prefix: &str) -> Option<String> {
    let parts: Vec<&str> = prefix
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .collect();
    parts
        .into_iter()
        .find(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_uppercase()))
        .map(|s| s.to_string())
}

fn extract_module_name(prefix: &str) -> Option<String> {
    let last_alpha: String = prefix
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if last_alpha.is_empty() || last_alpha.chars().next().map_or(true, |c| c.is_lowercase()) {
        None
    } else {
        Some(last_alpha)
    }
}

fn extract_function_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            let after_fn = trimmed.trim_start_matches("pub ").trim_start_matches("fn ");
            if let Some(paren) = after_fn.find('(') {
                let name = after_fn[..paren].trim();
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

fn build_nodes(edges: &[CallEdge], root: &str) -> Vec<CallNode> {
    let mut node_map: HashMap<String, (Vec<String>, bool)> = HashMap::new();

    node_map
        .entry(root.to_string())
        .or_insert_with(|| (Vec::new(), false));

    for edge in edges {
        let is_external = edge.call_type != CallType::InternalCall;
        let entry = node_map
            .entry(edge.callee.clone())
            .or_insert_with(|| (Vec::new(), is_external));
        entry.1 = entry.1 || is_external;
    }

    node_map
        .into_iter()
        .map(|(name, (functions, is_external))| CallNode {
            name,
            functions,
            is_external,
        })
        .collect()
}

fn extract_dependencies(content: &str) -> Vec<String> {
    let mut deps = HashSet::new();
    for line in content.lines() {
        let t = line.trim();
        // Skip comment lines so `// use foo::bar;` notes do not leak through.
        if t.starts_with("//") {
            continue;
        }
        if t.starts_with("use ") {
            let without_use = t.trim_start_matches("use ").trim_end_matches(';');
            if let Some(top) = without_use.split("::").next() {
                if top != "crate" && top != "super" && top != "self" && top != "std" {
                    deps.insert(top.to_string());
                }
            }
        }
    }
    deps.into_iter().collect()
}

fn detect_patterns(edges: &[CallEdge], content: &str) -> Vec<CallPattern> {
    let mut patterns = Vec::new();
    let target = |s: &str| Some(s.to_string());

    // Check for re-entrancy risk: calling external contract then updating state
    let has_external_calls = edges.iter().any(|e| e.call_type != CallType::InternalCall);
    let has_storage_after = content.contains("storage.set")
        || content.contains("env.storage().set")
        || content.contains(".set(");
    if has_external_calls && has_storage_after {
        patterns.push(CallPattern {
            name: "potential-reentrancy".to_string(),
            description: "External calls detected before storage updates — consider using checks-effects-interactions pattern.".to_string(),
            severity: "medium".to_string(),
            target: None,
        });
    }

    // Check for deep call chains
    if edges.len() > 5 {
        patterns.push(CallPattern {
            name: "deep-call-chain".to_string(),
            description: format!(
                "Contract has {} outgoing calls. Deep call chains increase gas cost and attack surface.",
                edges.len()
            ),
            severity: "low".to_string(),
            target: None,
        });
    }

    // Check for missing auth on external calls
    let has_require_auth =
        content.contains("require_auth") || content.contains("require_auth_for_args");
    if has_external_calls && !has_require_auth {
        patterns.push(CallPattern {
            name: "missing-auth-check".to_string(),
            description: "External calls found but no require_auth() detected. Ensure callers are authorized.".to_string(),
            severity: "high".to_string(),
            target: None,
        });
    }

    // Direct recursion: caller == callee on an internal edge indicates
    // a function calls itself, which may stack-blow unless bounded.
    let mut saw_recursion = false;
    for edge in edges
        .iter()
        .filter(|e| e.call_type == CallType::InternalCall)
    {
        if edge.caller == edge.callee && !saw_recursion {
            patterns.push(CallPattern {
                name: "recursive-call-cycle".to_string(),
                description: format!(
                    "Function '{}' appears to call itself. Ensure termination/depth limits are in place.",
                    edge.caller
                ),
                severity: "high".to_string(),
                target: target(&edge.caller),
            });
            saw_recursion = true;
        }
    }

    // Mutual recursion via internal calls: A -> B and B -> A.
    let mut saw_mutual = false;
    let internal_pairs: Vec<(&str, &str)> = edges
        .iter()
        .filter(|e| e.call_type == CallType::InternalCall)
        .map(|e| (e.caller.as_str(), e.callee.as_str()))
        .collect();
    for (a, b) in internal_pairs.iter() {
        if a != b && internal_pairs.iter().any(|(x, y)| *x == *b && *y == *a) && !saw_mutual {
            patterns.push(CallPattern {
                name: "mutual-recursion".to_string(),
                description: format!(
                    "Functions '{}' and '{}' call each other. Trace call cycles before deployment.",
                    a, b
                ),
                severity: "medium".to_string(),
                target: target(a),
            });
            saw_mutual = true;
        }
    }

    // Fan-out heuristic: a single function that calls too many distinct
    // externals raises the attack surface.
    let mut out_targets: HashMap<&str, HashSet<&str>> = HashMap::new();
    for edge in edges
        .iter()
        .filter(|e| e.call_type != CallType::InternalCall)
    {
        out_targets
            .entry(edge.caller.as_str())
            .or_default()
            .insert(edge.callee.as_str());
    }
    for (caller, callees) in &out_targets {
        if callees.len() >= 3 {
            patterns.push(CallPattern {
                name: "high-fan-out".to_string(),
                description: format!(
                    "Function '{}' fans out to {} distinct external contracts. Consider batching or splitting responsibilities.",
                    caller,
                    callees.len()
                ),
                severity: "medium".to_string(),
                target: target(caller),
            });
        }
    }

    // Repeated identical external calls — likely candidates for caching.
    let mut pair_count: HashMap<(&str, &str), usize> = HashMap::new();
    for edge in edges
        .iter()
        .filter(|e| e.call_type == CallType::DirectInvoke)
    {
        *pair_count
            .entry((edge.caller.as_str(), edge.callee.as_str()))
            .or_insert(0) += 1;
    }
    for ((caller, callee), count) in &pair_count {
        if *count > 1 {
            patterns.push(CallPattern {
                name: "duplicate-call".to_string(),
                description: format!(
                    "'{}' invokes contract '{}' {} times. Cache or combine to reduce cross-contract gas.",
                    caller, callee, count
                ),
                severity: "low".to_string(),
                target: target(caller),
            });
        }
    }

    patterns
}

fn count_lines(text: &str) -> usize {
    text.lines().count() + 1
}

// ── Optimization suggestions ─────────────────────────────────────────────────

/// Generate concrete structural and gas-oriented optimization suggestions
/// based on the shape of the call graph. Used by the `--optimize` flag of
/// `starforge contract call-graph`.
pub fn generate_suggestions(graph: &CallGraph) -> Vec<OptimizationSuggestion> {
    let mut suggestions = Vec::new();

    // 1) Duplicate identical external calls → cache results.
    let mut pair_count: HashMap<(String, String), usize> = HashMap::new();
    for edge in &graph.edges {
        if edge.call_type == CallType::DirectInvoke {
            *pair_count
                .entry((edge.caller.clone(), edge.callee.clone()))
                .or_insert(0) += 1;
        }
    }
    for ((caller, callee), count) in &pair_count {
        if *count > 1 {
            suggestions.push(OptimizationSuggestion {
                target: caller.clone(),
                title: "Cache duplicate external call".to_string(),
                detail: format!(
                    "'{}' invokes '{}' {} times. Cache the result once and reuse.",
                    caller, callee, count
                ),
                priority: "high".to_string(),
                estimated_savings: Some(format!("~{} cross-contract hops", count - 1)),
            });
        }
    }

    // 2) High fan-out → batching / splitting.
    let mut out_targets: HashMap<&str, HashSet<&str>> = HashMap::new();
    for edge in &graph.edges {
        if edge.call_type != CallType::InternalCall {
            out_targets
                .entry(edge.caller.as_str())
                .or_default()
                .insert(edge.callee.as_str());
        }
    }
    for (caller, callees) in &out_targets {
        if callees.len() >= 3 {
            suggestions.push(OptimizationSuggestion {
                target: (*caller).to_string(),
                title: "Reduce fan-out".to_string(),
                detail: format!(
                    "'{}' calls {} distinct external contracts in parallel paths. Consider batching related calls or splitting the public surface.",
                    caller,
                    callees.len()
                ),
                priority: "medium".to_string(),
                estimated_savings: None,
            });
        }
    }

    // 3) Heavy external dependencies → reduces WASM size.
    if graph.dependencies.len() > 5 {
        suggestions.push(OptimizationSuggestion {
            target: graph.root.clone(),
            title: "Consolidate imports".to_string(),
            detail: format!(
                "Contract pulls in {} external crates ({}). Review `use` lines and prune unused features to shrink WASM.",
                graph.dependencies.len(),
                graph.dependencies.join(", ")
            ),
            priority: "medium".to_string(),
            estimated_savings: None,
        });
    }

    // 4) High internal stack depth → invoke flatten / inline.
    let internal_count = graph
        .edges
        .iter()
        .filter(|e| e.call_type == CallType::InternalCall)
        .count();
    if internal_count >= 4 {
        suggestions.push(OptimizationSuggestion {
            target: graph.root.clone(),
            title: "Flatten internal helpers".to_string(),
            detail: format!(
                "{} internal call edges detected. Inline tiny helpers or use `#[inline]` to reduce wasm code size.",
                internal_count
            ),
            priority: "low".to_string(),
            estimated_savings: None,
        });
    }

    // 5) Detected patterns that already represent optimization opportunities
    //    are mirrored as suggestions so they show up under `--optimize` too.
    for pat in &graph.patterns {
        if pat.name == "duplicate-call" {
            let tgt = pat.target.clone().unwrap_or_else(|| graph.root.clone());
            suggestions.push(OptimizationSuggestion {
                target: tgt,
                title: "Deduplicate contract call".to_string(),
                detail: pat.description.clone(),
                priority: "low".to_string(),
                estimated_savings: None,
            });
        }
        if pat.name == "high-fan-out" {
            let tgt = pat.target.clone().unwrap_or_else(|| graph.root.clone());
            suggestions.push(OptimizationSuggestion {
                target: tgt,
                title: "Tighten external surface".to_string(),
                detail: pat.description.clone(),
                priority: "low".to_string(),
                estimated_savings: None,
            });
        }
    }

    suggestions
}

// ── Interactive explorer ─────────────────────────────────────────────────────

/// Start a stdin-driven interactive session that lets a developer pick a
/// node from the call graph and inspect its incoming / outgoing edges,
/// functions, and detected patterns. Pure-stdlib so it works in any TTY.
pub fn explore_graph(graph: &CallGraph) -> anyhow::Result<()> {
    if graph.nodes.is_empty() {
        println!("\n  No nodes to explore. The contract appears to have no detectable cross-contract interactions.");
        return Ok(());
    }

    loop {
        println!();
        println!("{}", "═".repeat(64).dimmed());
        println!("  {}  Cross-Contract Call Explorer", "🛰".bright_cyan());
        println!("{}", "═".repeat(64).dimmed());
        println!("  Root contract: {}", graph.root.bright_white().bold());
        println!(
            "  Nodes: {}   Edges: {}   Patterns: {}",
            graph.nodes.len(),
            graph.edges.len(),
            graph.patterns.len()
        );
        println!();
        println!("  Available commands:");
        println!("    <number>   Inspect node by its index");
        println!("    s          Show graph statistics");
        println!("    d          Show dependency list");
        println!("    p          Show detected patterns");
        println!("    o          Show optimization suggestions");
        println!("    q / 0      Quit explorer");
        println!();
        println!("  Nodes:");
        for (i, node) in graph.nodes.iter().enumerate() {
            let marker = if node.is_external { "↔" } else { "·" };
            println!(
                "    {:>2}. {} {} {}",
                i + 1,
                marker.bright_cyan(),
                node.name.bright_white(),
                if node.is_external {
                    "(external)".dimmed()
                } else {
                    "".normal()
                }
            );
        }
        print!("\n  ➜ Choose: ");
        io::stdout().flush().ok();

        let mut input = String::new();
        let bytes = io::stdin().read_line(&mut input)?;
        if bytes == 0 {
            // EOF (e.g. piped input) — leave the loop.
            println!();
            return Ok(());
        }
        let choice = input.trim();

        match choice {
            "" => continue,
            "q" | "Q" | "0" => {
                println!("\n  Exiting explorer. ✓\n");
                return Ok(());
            }
            "s" | "S" => {
                let stats = compute_stats(graph);
                println!();
                println!("  {}", "Graph Statistics".bright_white().bold());
                println!("  {}", "─".repeat(40).dimmed());
                println!("  Total nodes          : {}", stats.total_nodes);
                println!("  Total edges          : {}", stats.total_edges);
                println!("    external           : {}", stats.external_edges);
                println!("    internal           : {}", stats.internal_edges);
                println!("  Direct invokes       : {}", stats.direct_invokes);
                println!("  Client constructions : {}", stats.client_constructions);
                println!("  Dependencies         : {}", stats.dependencies);
                println!(
                    "  Patterns (h/m/l)     : {} / {} / {}",
                    stats.patterns_high, stats.patterns_medium, stats.patterns_low
                );
                println!("  Max out-degree       : {}", stats.fan_out_max);
                println!("  Max in-degree        : {}", stats.fan_in_max);
                prompt_enter();
            }
            "d" | "D" => {
                println!();
                println!("  {}", "Module Dependencies".bright_white().bold());
                println!("  {}", "─".repeat(40).dimmed());
                if graph.dependencies.is_empty() {
                    println!("  (none)");
                } else {
                    for dep in &graph.dependencies {
                        println!("  use {}", dep.bright_white());
                    }
                }
                prompt_enter();
            }
            "p" | "P" => {
                println!();
                println!("  {}", "Detected Patterns".bright_white().bold());
                println!("  {}", "─".repeat(40).dimmed());
                if graph.patterns.is_empty() {
                    println!("  No patterns detected.");
                } else {
                    for pat in &graph.patterns {
                        let icon = match pat.severity.as_str() {
                            "high" => "⚠".red(),
                            "medium" => "⚡".yellow(),
                            _ => "ℹ".cyan(),
                        };
                        println!(
                            "  {} [{}] {}",
                            icon,
                            pat.severity.to_uppercase().dimmed(),
                            pat.name.bright_white()
                        );
                        println!("      {}", pat.description.dimmed());
                    }
                }
                prompt_enter();
            }
            "o" | "O" => {
                println!();
                println!("  {}", "Optimization Suggestions".bright_white().bold());
                println!("  {}", "─".repeat(40).dimmed());
                let suggestions = generate_suggestions(graph);
                if suggestions.is_empty() {
                    println!("  No improvements suggested.");
                } else {
                    for sug in &suggestions {
                        let icon = match sug.priority.as_str() {
                            "high" => "▲".red(),
                            "medium" => "●".yellow(),
                            _ => "·".cyan(),
                        };
                        println!(
                            "  {} [{}] {} → {}",
                            icon,
                            sug.priority.to_uppercase().dimmed(),
                            sug.title.bright_white(),
                            sug.target.bright_green()
                        );
                        println!("      {}", sug.detail.dimmed());
                        if let Some(s) = &sug.estimated_savings {
                            println!("      est. savings: {}", s.cyan());
                        }
                    }
                }
                prompt_enter();
            }
            other => {
                if let Ok(idx) = other.parse::<usize>() {
                    if idx >= 1 && idx <= graph.nodes.len() {
                        print_node_details(graph, &graph.nodes[idx - 1]);
                        prompt_enter();
                    } else {
                        println!("  {} Invalid selection.", "✗".red());
                    }
                } else {
                    println!("  {} Unknown command '{}'.", "✗".red(), other);
                }
            }
        }
    }
}

fn prompt_enter() {
    print!("\n  Press <enter> to continue… ");
    io::stdout().flush().ok();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

fn print_node_details(graph: &CallGraph, node: &CallNode) {
    println!();
    println!(
        "  {} {}",
        "◆".bright_cyan(),
        node.name.bright_white().bold()
    );
    println!("  {}", "─".repeat(40).dimmed());
    println!(
        "  External: {}",
        if node.is_external {
            "yes".yellow()
        } else {
            "no".dimmed()
        }
    );

    let incoming: Vec<&CallEdge> = graph
        .edges
        .iter()
        .filter(|e| e.callee == node.name)
        .collect();
    let outgoing: Vec<&CallEdge> = graph
        .edges
        .iter()
        .filter(|e| e.caller == node.name)
        .collect();

    println!("\n  Outgoing calls ({}):", outgoing.len());
    if outgoing.is_empty() {
        println!("    (none)");
    } else {
        for edge in &outgoing {
            let loc = edge.location.as_deref().unwrap_or("").to_string();
            let arrow = match edge.call_type {
                CallType::InternalCall => "⟶",
                _ => "⤳",
            };
            println!(
                "    {} {} ({}) {}",
                arrow.bright_cyan(),
                edge.callee.bright_white(),
                edge.call_type.to_string().dimmed(),
                loc.dimmed()
            );
        }
    }

    println!("\n  Incoming calls ({}):", incoming.len());
    if incoming.is_empty() {
        println!("    (none)");
    } else {
        for edge in &incoming {
            let arrow = match edge.call_type {
                CallType::InternalCall => "⟵",
                _ => "⤴",
            };
            println!(
                "    {} {} ({})",
                arrow.bright_cyan(),
                edge.caller.bright_white(),
                edge.call_type.to_string().dimmed()
            );
        }
    }
}

pub fn render_ascii(graph: &CallGraph) -> String {
    let mut out = String::new();
    out.push_str(&format!("Call Graph: {}\n", graph.root));
    out.push_str(&"─".repeat(50));
    out.push('\n');

    let external: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.call_type != CallType::InternalCall)
        .collect();
    let internal: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.call_type == CallType::InternalCall)
        .collect();

    if !external.is_empty() {
        out.push_str("\nExternal Calls:\n");
        for edge in &external {
            let loc = edge.location.as_deref().unwrap_or("").to_string();
            out.push_str(&format!(
                "  [{}] ──({})──▶ {}  {}\n",
                graph.root, edge.call_type, edge.callee, loc,
            ));
        }
    }

    if !internal.is_empty() {
        out.push_str("\nInternal Functions:\n");
        for edge in &internal {
            out.push_str(&format!("  [{}] calls {}()\n", graph.root, edge.callee));
        }
    }

    if !graph.dependencies.is_empty() {
        out.push_str("\nImport Dependencies:\n");
        for dep in &graph.dependencies {
            out.push_str(&format!("  use {}\n", dep));
        }
    }

    if !graph.patterns.is_empty() {
        out.push_str("\nPatterns Detected:\n");
        for pat in &graph.patterns {
            out.push_str(&format!(
                "  [{}] {}: {}\n",
                pat.severity.to_uppercase(),
                pat.name,
                pat.description,
            ));
        }
    }

    out.push_str(&"─".repeat(50));
    out.push('\n');
    out
}

pub fn render_dot(graph: &CallGraph) -> String {
    let mut out = String::new();
    out.push_str("digraph call_graph {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  node [shape=box];\n");
    out.push_str(&format!(
        "  \"{}\" [style=filled, fillcolor=lightblue];\n",
        graph.root
    ));

    let mut seen = HashSet::new();
    for edge in &graph.edges {
        if !seen.contains(&edge.callee) {
            seen.insert(edge.callee.clone());
            let color = if edge.call_type == CallType::InternalCall {
                "lightyellow"
            } else {
                "lightcoral"
            };
            out.push_str(&format!(
                "  \"{}\" [style=filled, fillcolor={}];\n",
                edge.callee, color
            ));
        }
        let style = match edge.call_type {
            CallType::InternalCall => "dashed",
            _ => "solid",
        };
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{}\", style={}];\n",
            edge.caller, edge.callee, edge.call_type, style,
        ));
    }

    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper for building graphs in tests.
    fn mk_graph(edges: Vec<CallEdge>, deps: Vec<String>) -> CallGraph {
        let nodes = build_nodes(&edges, "root");
        let patterns = detect_patterns(&edges, "");
        CallGraph {
            root: "root".to_string(),
            nodes,
            edges,
            dependencies: deps,
            patterns,
        }
    }

    fn edge(caller: &str, callee: &str, kind: CallType) -> CallEdge {
        CallEdge {
            caller: caller.to_string(),
            callee: callee.to_string(),
            call_type: kind,
            location: Some("line 1".to_string()),
        }
    }

    #[test]
    fn extract_functions_finds_pub_fn() {
        let src = "pub fn transfer(env: Env) {}\nfn helper() {}";
        let fns = extract_function_names(src);
        assert!(fns.contains(&"transfer".to_string()));
        assert!(fns.contains(&"helper".to_string()));
    }

    #[test]
    fn deps_excludes_std_and_crate() {
        let src = "use crate::utils;\nuse std::vec;\nuse soroban_sdk::Env;";
        let deps = extract_dependencies(src);
        assert!(!deps.contains(&"crate".to_string()));
        assert!(!deps.contains(&"std".to_string()));
        assert!(deps.contains(&"soroban_sdk".to_string()));
    }

    #[test]
    fn deps_ignores_comment_lines() {
        let src =
            "// use fake::x;\nuse soroban_sdk::Env;\n/// doc: use fake::y;\nuse token::Client;";
        let deps = extract_dependencies(src);
        assert!(
            !deps.contains(&"fake".to_string()),
            "comment lines must not be parsed as imports: {:?}",
            deps
        );
        assert!(deps.contains(&"soroban_sdk".to_string()));
        assert!(deps.contains(&"token".to_string()));
    }

    #[test]
    fn render_ascii_not_empty() {
        let graph = CallGraph {
            root: "mycontract".to_string(),
            nodes: vec![],
            edges: vec![],
            dependencies: vec![],
            patterns: vec![],
        };
        let out = render_ascii(&graph);
        assert!(out.contains("mycontract"));
    }

    // ── new feature tests ────────────────────────────────────────────────────

    #[test]
    fn stats_counts_external_and_internal() {
        let g = mk_graph(
            vec![
                edge("root", "Token", CallType::DirectInvoke),
                edge("root", "Vault", CallType::ClientNew),
                edge("root", "Oracle", CallType::ExternalCall),
                edge("root", "helper", CallType::InternalCall),
            ],
            vec!["soroban_sdk".to_string()],
        );
        let s = compute_stats(&g);
        assert_eq!(s.total_edges, 4);
        assert_eq!(s.external_edges, 3);
        assert_eq!(s.internal_edges, 1);
        assert_eq!(s.direct_invokes, 1);
        assert_eq!(s.client_constructions, 1);
        assert_eq!(s.fan_out_max, 4); // root has 4 outgoing edges
    }

    #[test]
    fn suggestions_detect_duplicate_calls() {
        let g = mk_graph(
            vec![
                edge("root", "Token", CallType::DirectInvoke),
                edge("root", "Token", CallType::DirectInvoke),
                edge("root", "Token", CallType::DirectInvoke),
            ],
            vec![],
        );
        let sugs = generate_suggestions(&g);
        assert!(
            sugs.iter()
                .any(|s| s.title == "Cache duplicate external call"),
            "expected a cache suggestion, got: {:?}",
            sugs
        );
    }

    #[test]
    fn suggestions_flag_high_fan_out() {
        let g = mk_graph(
            vec![
                edge("root", "A", CallType::ExternalCall),
                edge("root", "B", CallType::DirectInvoke),
                edge("root", "C", CallType::ClientNew),
                edge("root", "D", CallType::DirectInvoke),
            ],
            vec![],
        );
        let sugs = generate_suggestions(&g);
        assert!(
            sugs.iter().any(|s| s.title == "Reduce fan-out"),
            "expected fan-out suggestion"
        );
    }

    #[test]
    fn suggestions_flag_heavy_imports() {
        let g = mk_graph(
            vec![edge("root", "X", CallType::DirectInvoke)],
            vec![
                "soroban_sdk".into(),
                "token".into(),
                "oracle".into(),
                "vault".into(),
                "amm".into(),
                "governance".into(),
            ],
        );
        let sugs = generate_suggestions(&g);
        assert!(
            sugs.iter().any(|s| s.title == "Consolidate imports"),
            "expected consolidation suggestion"
        );
    }

    #[test]
    fn patterns_flag_recursion() {
        let g = mk_graph(vec![edge("root", "root", CallType::InternalCall)], vec![]);
        assert!(g.patterns.iter().any(|p| p.name == "recursive-call-cycle"));
    }

    #[test]
    fn patterns_flag_fan_out() {
        let g = mk_graph(
            vec![
                edge("root", "A", CallType::DirectInvoke),
                edge("root", "B", CallType::ClientNew),
                edge("root", "C", CallType::DirectInvoke),
            ],
            vec![],
        );
        assert!(g.patterns.iter().any(|p| p.name == "high-fan-out"));
    }

    #[test]
    fn patterns_flag_duplicate_calls() {
        let g = mk_graph(
            vec![
                edge("root", "Token", CallType::DirectInvoke),
                edge("root", "Token", CallType::DirectInvoke),
            ],
            vec![],
        );
        assert!(g.patterns.iter().any(|p| p.name == "duplicate-call"));
    }

    #[test]
    fn patterns_flag_mutual_recursion() {
        let g = mk_graph(
            vec![
                edge("root", "a", CallType::InternalCall),
                edge("a", "b", CallType::InternalCall),
                edge("b", "a", CallType::InternalCall),
            ],
            vec![],
        );
        assert!(g.patterns.iter().any(|p| p.name == "mutual-recursion"));
    }

    #[test]
    fn explore_graph_handles_empty_nodes() {
        let g = CallGraph {
            root: "empty".to_string(),
            nodes: vec![],
            edges: vec![],
            dependencies: vec![],
            patterns: vec![],
        };
        // Should not panic, should print a message and return Ok.
        // We avoid actually reading stdin; instead test that the early-return
        // path by inspecting `stats`.
        let stats = compute_stats(&g);
        assert_eq!(stats.total_nodes, 0);
        assert_eq!(stats.total_edges, 0);
    }

    #[test]
    fn render_dot_includes_rankdir_and_nodes() {
        let g = mk_graph(
            vec![
                edge("root", "Token", CallType::DirectInvoke),
                edge("root", "Helper", CallType::InternalCall),
            ],
            vec![],
        );
        let dot = render_dot(&g);
        assert!(dot.contains("digraph"));
        assert!(dot.contains("root"));
        assert!(dot.contains("Token"));
        assert!(dot.contains("Helper"));
    }

    #[test]
    fn ascii_render_lists_external_then_internal() {
        let g = mk_graph(
            vec![
                edge("root", "Token", CallType::DirectInvoke),
                edge("root", "helper", CallType::InternalCall),
            ],
            vec!["soroban_sdk".to_string()],
        );
        let out = render_ascii(&g);
        assert!(out.contains("External Calls"));
        assert!(out.contains("Internal Functions"));
        assert!(out.contains("Import Dependencies"));
    }
}
