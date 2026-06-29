use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    pub contract_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub network: String,
    pub generated_at: String,
    pub sections: Vec<DocSection>,
    pub api: ApiDocumentation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSection {
    pub title: String,
    pub content: String,
    pub order: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiDocumentation {
    pub functions: Vec<FunctionDoc>,
    pub events: Vec<EventDoc>,
    pub storage: Vec<StorageDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDoc {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ParamDoc>,
    pub returns: Option<String>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDoc {
    pub name: String,
    pub ty: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDoc {
    pub name: String,
    pub description: String,
    pub topics: Vec<TopicDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicDoc {
    pub name: String,
    pub ty: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDoc {
    pub key: String,
    pub ty: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocVersion {
    pub version: String,
    pub generated_at: String,
    pub entry: DocEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocIndex {
    pub contracts: Vec<DocIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocIndexEntry {
    pub contract_id: String,
    pub name: String,
    pub versions: Vec<DocVersionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocVersionRef {
    pub version: String,
    pub path: String,
}

fn docs_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".starforge").join("docs");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

fn index_file() -> Result<PathBuf> {
    Ok(docs_dir()?.join("index.json"))
}

fn contract_doc_dir(contract_id: &str) -> Result<PathBuf> {
    let safe_id = contract_id.replace('/', "_");
    let dir = docs_dir()?.join(safe_id);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn generate_documentation(
    contract_id: &str,
    name: &str,
    description: &str,
    network: &str,
    version: &str,
    functions: Vec<FunctionDoc>,
    events: Vec<EventDoc>,
    storage: Vec<StorageDoc>,
    sections: Vec<DocSection>,
) -> Result<DocEntry> {
    let entry = DocEntry {
        contract_id: contract_id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        version: version.to_string(),
        network: network.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        sections,
        api: ApiDocumentation {
            functions,
            events,
            storage,
        },
    };

    let doc_dir = contract_doc_dir(contract_id)?;
    let doc_file = doc_dir.join(format!("{}.json", version));

    fs::write(&doc_file, serde_json::to_string_pretty(&entry)?)?;

    update_index(contract_id, name, version, &doc_file)?;

    Ok(entry)
}

pub fn get_documentation(contract_id: &str, version: Option<&str>) -> Result<DocEntry> {
    let doc_dir = contract_doc_dir(contract_id)?;

    if let Some(v) = version {
        let doc_file = doc_dir.join(format!("{}.json", v));
        if !doc_file.exists() {
            anyhow::bail!(
                "Documentation version '{}' not found for contract '{}'",
                v,
                contract_id
            );
        }
        let content = fs::read_to_string(&doc_file)?;
        let entry: DocEntry = serde_json::from_str(&content)?;
        return Ok(entry);
    }

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    if doc_dir.exists() {
        for entry in fs::read_dir(&doc_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    versions.push((stem.to_string(), path));
                }
            }
        }
    }

    versions.sort_by(|a, b| b.0.cmp(&a.0));

    let (_, latest_path) = versions
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No documentation found for contract '{}'", contract_id))?;

    let content = fs::read_to_string(&latest_path)?;
    let entry: DocEntry = serde_json::from_str(&content)?;
    Ok(entry)
}

pub fn list_documentation() -> Result<DocIndex> {
    let idx_file = index_file()?;
    if !idx_file.exists() {
        return Ok(DocIndex {
            contracts: Vec::new(),
        });
    }

    let content = fs::read_to_string(&idx_file)?;
    let index: DocIndex = serde_json::from_str(&content)?;
    Ok(index)
}

pub fn search_documentation(query: &str) -> Result<Vec<SearchResult>> {
    let index = list_documentation()?;
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for contract in &index.contracts {
        for version_ref in &contract.versions {
            let doc_dir = contract_doc_dir(&contract.contract_id)?;
            let doc_file = doc_dir.join(&version_ref.path);
            if !doc_file.exists() {
                continue;
            }

            let content = fs::read_to_string(&doc_file)?;
            let entry: DocEntry = serde_json::from_str(&content)?;

            let mut score = 0;
            let mut matched_sections = Vec::new();

            if entry.name.to_lowercase().contains(&query_lower) {
                score += 100;
            }
            if entry.description.to_lowercase().contains(&query_lower) {
                score += 50;
            }

            for section in &entry.sections {
                if section.title.to_lowercase().contains(&query_lower)
                    || section.content.to_lowercase().contains(&query_lower)
                {
                    score += 20;
                    matched_sections.push(section.title.clone());
                }
            }

            for func in &entry.api.functions {
                if func.name.to_lowercase().contains(&query_lower)
                    || func.description.to_lowercase().contains(&query_lower)
                {
                    score += 30;
                    matched_sections.push(format!("function:{}", func.name));
                }
            }

            for event in &entry.api.events {
                if event.name.to_lowercase().contains(&query_lower) {
                    score += 30;
                    matched_sections.push(format!("event:{}", event.name));
                }
            }

            if score > 0 {
                results.push(SearchResult {
                    contract_id: contract.contract_id.clone(),
                    name: contract.name.clone(),
                    version: version_ref.version.clone(),
                    score,
                    matched_sections,
                });
            }
        }
    }

    results.sort_by_key(|b| std::cmp::Reverse(b.score));
    Ok(results)
}

pub fn list_versions(contract_id: &str) -> Result<Vec<String>> {
    let doc_dir = contract_doc_dir(contract_id)?;
    let mut versions = Vec::new();

    if doc_dir.exists() {
        for entry in fs::read_dir(&doc_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    versions.push(stem.to_string());
                }
            }
        }
    }

    versions.sort_by(|a, b| b.cmp(a));
    Ok(versions)
}

pub fn render_markdown(contract_id: &str, version: Option<&str>) -> Result<String> {
    let entry = get_documentation(contract_id, version)?;

    let mut md = String::new();
    md.push_str(&format!("# {} Documentation\n\n", entry.name));
    md.push_str(&format!("**Contract:** `{}`\n", entry.contract_id));
    md.push_str(&format!("**Network:** {}\n", entry.network));
    md.push_str(&format!("**Version:** {}\n", entry.version));
    md.push_str(&format!("**Generated:** {}\n\n", &entry.generated_at[..10]));
    md.push_str(&format!("{}\n\n", entry.description));

    for section in &entry.sections {
        md.push_str(&format!("## {}\n\n", section.title));
        md.push_str(&format!("{}\n\n", section.content));
    }

    if !entry.api.functions.is_empty() {
        md.push_str("## API Reference\n\n");
        md.push_str("### Functions\n\n");

        for func in &entry.api.functions {
            md.push_str(&format!("#### `{}`\n\n", func.name));
            md.push_str(&format!("{}\n\n", func.description));

            if !func.parameters.is_empty() {
                md.push_str("**Parameters:**\n\n");
                for param in &func.parameters {
                    let req = if param.required {
                        "required"
                    } else {
                        "optional"
                    };
                    md.push_str(&format!(
                        "- `{}` ({}, {}): {}\n",
                        param.name, param.ty, req, param.description
                    ));
                }
                md.push('\n');
            }

            if let Some(ref returns) = func.returns {
                md.push_str(&format!("**Returns:** {}\n\n", returns));
            }

            if !func.examples.is_empty() {
                md.push_str("**Examples:**\n\n");
                for example in &func.examples {
                    md.push_str(&format!("```\n{}\n```\n\n", example));
                }
            }
        }
    }

    if !entry.api.events.is_empty() {
        md.push_str("### Events\n\n");
        for event in &entry.api.events {
            md.push_str(&format!("#### `{}`\n\n", event.name));
            md.push_str(&format!("{}\n\n", event.description));
            if !event.topics.is_empty() {
                md.push_str("**Topics:**\n\n");
                for topic in &event.topics {
                    md.push_str(&format!(
                        "- `{}` ({}): {}\n",
                        topic.name, topic.ty, topic.description
                    ));
                }
                md.push('\n');
            }
        }
    }

    if !entry.api.storage.is_empty() {
        md.push_str("### Storage\n\n");
        for storage in &entry.api.storage {
            md.push_str(&format!(
                "- `{}` ({}): {}\n",
                storage.key, storage.ty, storage.description
            ));
        }
        md.push('\n');
    }

    Ok(md)
}

#[derive(Debug)]
pub struct SearchResult {
    pub contract_id: String,
    pub name: String,
    pub version: String,
    pub score: u32,
    pub matched_sections: Vec<String>,
}

fn update_index(contract_id: &str, name: &str, version: &str, doc_file: &Path) -> Result<()> {
    let mut index = list_documentation()?;

    let filename = doc_file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    if let Some(contract) = index
        .contracts
        .iter_mut()
        .find(|c| c.contract_id == contract_id)
    {
        contract.name = name.to_string();
        if !contract.versions.iter().any(|v| v.version == version) {
            contract.versions.push(DocVersionRef {
                version: version.to_string(),
                path: filename,
            });
        }
        contract.versions.sort_by(|a, b| b.version.cmp(&a.version));
    } else {
        index.contracts.push(DocIndexEntry {
            contract_id: contract_id.to_string(),
            name: name.to_string(),
            versions: vec![DocVersionRef {
                version: version.to_string(),
                path: filename,
            }],
        });
    }

    fs::write(index_file()?, serde_json::to_string_pretty(&index)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_and_retrieve_documentation() {
        let tmp = tempdir().unwrap();
        let docs = tmp.path().join("docs");
        fs::create_dir_all(&docs).unwrap();

        let entry = DocEntry {
            contract_id: "CABC123".to_string(),
            name: "Test Contract".to_string(),
            description: "A test contract".to_string(),
            version: "1.0.0".to_string(),
            network: "testnet".to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            sections: vec![DocSection {
                title: "Overview".to_string(),
                content: "This is a test contract.".to_string(),
                order: 0,
            }],
            api: ApiDocumentation {
                functions: vec![FunctionDoc {
                    name: "transfer".to_string(),
                    description: "Transfer tokens".to_string(),
                    parameters: vec![ParamDoc {
                        name: "amount".to_string(),
                        ty: "i128".to_string(),
                        description: "Amount to transfer".to_string(),
                        required: true,
                    }],
                    returns: Some("bool".to_string()),
                    examples: vec!["transfer(100)".to_string()],
                }],
                events: vec![],
                storage: vec![],
            },
        };

        let json = serde_json::to_string_pretty(&entry).unwrap();
        let doc_file = docs.join("1.0.0.json");
        fs::write(&doc_file, &json).unwrap();

        let loaded: DocEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.name, "Test Contract");
        assert_eq!(loaded.api.functions.len(), 1);
        assert_eq!(loaded.api.functions[0].name, "transfer");
    }

    #[test]
    fn doc_index_serializes() {
        let index = DocIndex {
            contracts: vec![DocIndexEntry {
                contract_id: "CABC".to_string(),
                name: "Test".to_string(),
                versions: vec![DocVersionRef {
                    version: "1.0.0".to_string(),
                    path: "1.0.0.json".to_string(),
                }],
            }],
        };

        let json = serde_json::to_string(&index).unwrap();
        assert!(json.contains("CABC"));
        assert!(json.contains("1.0.0"));
    }
}
