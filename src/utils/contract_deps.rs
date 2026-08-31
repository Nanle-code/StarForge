use anyhow::{Context, Result};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContractDependencies {
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DependencySource {
    Version(String),
    Detailed {
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        git: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
}

impl DependencySource {
    pub fn get_version_req(&self) -> Result<Option<VersionReq>> {
        match self {
            Self::Version(v) => {
                let req = VersionReq::parse(v).context("Invalid version requirement")?;
                Ok(Some(req))
            }
            Self::Detailed {
                version: Some(v), ..
            } => {
                let req = VersionReq::parse(v).context("Invalid version requirement")?;
                Ok(Some(req))
            }
            _ => Ok(None),
        }
    }
}

pub fn init(dir: &Path) -> Result<()> {
    let path = dir.join("contract-dependencies.toml");
    if path.exists() {
        anyhow::bail!("contract-dependencies.toml already exists in this directory");
    }
    let deps = ContractDependencies::default();
    save(&path, &deps)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<ContractDependencies> {
    let content = fs::read_to_string(path).context("Failed to read contract-dependencies.toml")?;
    let deps = toml::from_str(&content).context("Failed to parse contract-dependencies.toml")?;
    Ok(deps)
}

pub fn save(path: &Path, deps: &ContractDependencies) -> Result<()> {
    let content = toml::to_string_pretty(deps)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn add_dependency(dir: &Path, name: &str, source: DependencySource) -> Result<()> {
    let path = dir.join("contract-dependencies.toml");
    let mut deps = if path.exists() {
        load(&path)?
    } else {
        ContractDependencies::default()
    };

    // Validate version if present
    source.get_version_req()?;

    deps.dependencies.insert(name.to_string(), source);
    save(&path, &deps)?;
    Ok(())
}

pub fn update_dependency(dir: &Path, name: &str, version: &str) -> Result<()> {
    let path = dir.join("contract-dependencies.toml");
    let mut deps = load(&path)?;

    // Validate semantic version syntax
    VersionReq::parse(version).context("Invalid version constraint format")?;

    if let Some(dep) = deps.dependencies.get_mut(name) {
        *dep = match dep {
            DependencySource::Version(_) => DependencySource::Version(version.to_string()),
            DependencySource::Detailed {
                path: p,
                git,
                branch,
                ..
            } => DependencySource::Detailed {
                version: Some(version.to_string()),
                path: p.clone(),
                git: git.clone(),
                branch: branch.clone(),
            },
        };
        save(&path, &deps)?;
        Ok(())
    } else {
        anyhow::bail!("Dependency '{}' not found", name);
    }
}

// Graph resolution
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub nodes: HashSet<String>,
    pub edges: HashMap<String, Vec<String>>,
}

pub fn resolve_graph(dir: &Path) -> Result<DependencyGraph> {
    let mut graph = DependencyGraph {
        nodes: HashSet::new(),
        edges: HashMap::new(),
    };

    let mut visited = HashSet::new();
    // queue of (dir_path, node_name)
    let mut queue = vec![(dir.to_path_buf(), "root".to_string())];

    graph.nodes.insert("root".to_string());

    while let Some((curr_dir, node_name)) = queue.pop() {
        if visited.contains(&node_name) {
            continue;
        }
        visited.insert(node_name.clone());

        let path = curr_dir.join("contract-dependencies.toml");
        if !path.exists() {
            continue;
        }

        let deps = match load(&path) {
            Ok(d) => d,
            Err(_) => continue, // If we can't parse a dependency's toml, skip its transitive deps
        };

        for (dep_name, source) in deps.dependencies {
            graph.nodes.insert(dep_name.clone());
            graph
                .edges
                .entry(node_name.clone())
                .or_default()
                .push(dep_name.clone());

            // If it has a local path, we can resolve its transitive dependencies
            if let DependencySource::Detailed { path: Some(p), .. } = source {
                let dep_dir = curr_dir.join(p);
                queue.push((dep_dir, dep_name));
            }
        }
    }

    Ok(graph)
}

pub fn resolve_deployment_order(graph: &DependencyGraph) -> Result<Vec<String>> {
    let mut sorted = Vec::new();
    let mut visited = HashSet::new();
    let mut temp_visited = HashSet::new();

    for node in &graph.nodes {
        if node != "root" {
            topological_sort(
                node,
                &graph.edges,
                &mut visited,
                &mut temp_visited,
                &mut sorted,
            )?;
        }
    }

    Ok(sorted)
}

fn topological_sort(
    node: &str,
    edges: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    temp_visited: &mut HashSet<String>,
    sorted: &mut Vec<String>,
) -> Result<()> {
    if visited.contains(node) {
        return Ok(());
    }
    if temp_visited.contains(node) {
        anyhow::bail!("Circular dependency detected involving {}", node);
    }

    temp_visited.insert(node.to_string());

    if let Some(deps) = edges.get(node) {
        for dep in deps {
            topological_sort(dep, edges, visited, temp_visited, sorted)?;
        }
    }

    temp_visited.remove(node);
    visited.insert(node.to_string());
    sorted.push(node.to_string());

    Ok(())
}

pub fn render_ascii_graph(graph: &DependencyGraph) -> String {
    let mut out = String::new();
    out.push_str("root\n");
    let mut visited = HashSet::new();
    visited.insert("root".to_string());
    render_ascii_node(graph, "root", "", &mut out, &mut visited);
    out
}

fn render_ascii_node(
    graph: &DependencyGraph,
    node: &str,
    prefix: &str,
    out: &mut String,
    visited: &mut HashSet<String>,
) {
    if let Some(deps) = graph.edges.get(node) {
        let count = deps.len();
        for (i, dep) in deps.iter().enumerate() {
            let is_last = i == count - 1;
            let marker = if is_last { "└── " } else { "├── " };

            out.push_str(&format!("{}{}{}\n", prefix, marker, dep));

            if !visited.contains(dep) {
                visited.insert(dep.clone());
                let next_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
                render_ascii_node(graph, dep, &next_prefix, out, visited);
            } else {
                let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
                out.push_str(&format!("{}└── (deduplicated)\n", child_prefix));
            }
        }
    }
}

pub fn render_dot_graph(graph: &DependencyGraph) -> String {
    let mut dot = String::from("digraph Dependencies {\n");
    dot.push_str("    rankdir=TB;\n");
    dot.push_str("    node [shape=box, style=rounded];\n\n");

    for (node, deps) in &graph.edges {
        for dep in deps {
            dot.push_str(&format!("    \"{}\" -> \"{}\";\n", node, dep));
        }
    }

    dot.push_str("}\n");
    dot
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_version_req() {
        let src = DependencySource::Version("^1.0".to_string());
        assert!(src.get_version_req().unwrap().is_some());

        let src_bad = DependencySource::Version("invalid".to_string());
        assert!(src_bad.get_version_req().is_err());
    }

    #[test]
    fn test_init_and_add() {
        let dir = tempdir().unwrap();
        init(dir.path()).unwrap();

        add_dependency(
            dir.path(),
            "token",
            DependencySource::Version("1.0.0".to_string()),
        )
        .unwrap();
        let deps = load(&dir.path().join("contract-dependencies.toml")).unwrap();

        assert_eq!(deps.dependencies.len(), 1);
        assert!(matches!(
            deps.dependencies.get("token").unwrap(),
            DependencySource::Version(v) if v == "1.0.0"
        ));
    }
}
