use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDocumentation {
    pub contract_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub functions: Vec<FunctionDoc>,
    pub structs: Vec<StructDoc>,
    pub enums: Vec<EnumDoc>,
    pub events: Vec<EventDoc>,
    pub examples: Vec<UsageExample>,
    pub metadata: DocumentationMetadata,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDoc {
    pub name: String,
    pub description: String,
    pub inputs: Vec<ParameterDoc>,
    pub outputs: Vec<ParameterDoc>,
    pub access: AccessType,
    pub example: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDoc {
    pub name: String,
    pub param_type: String,
    pub description: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessType {
    Public,
    Admin,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDoc {
    pub name: String,
    pub description: String,
    pub fields: Vec<FieldDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDoc {
    pub name: String,
    pub field_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDoc {
    pub name: String,
    pub description: String,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDoc {
    pub name: String,
    pub description: String,
    pub fields: Vec<FieldDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageExample {
    pub title: String,
    pub description: String,
    pub code: String,
    pub language: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationMetadata {
    pub network: String,
    pub deployer: String,
    pub deployment_tx: String,
    pub soroban_version: String,
    pub stellar_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationIndex {
    pub contracts: Vec<ContractIndexEntry>,
    pub last_updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractIndexEntry {
    pub contract_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub tags: Vec<String>,
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationVersion {
    pub version: String,
    pub contract_id: String,
    pub documentation: ContractDocumentation,
    pub created_at: String,
    pub changelog: String,
}

pub struct DocumentationGenerator {
    output_dir: PathBuf,
}

impl DocumentationGenerator {
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }

    pub fn generate_from_wasm(
        &self,
        wasm_path: &Path,
        contract_id: &str,
        metadata: DocumentationMetadata,
    ) -> Result<ContractDocumentation> {
        let wasm_bytes = fs::read(wasm_path)?;

        // Parse WASM for function signatures (simplified)
        let functions = self.extract_functions_from_wasm(&wasm_bytes)?;

        let documentation = ContractDocumentation {
            contract_id: contract_id.to_string(),
            name: contract_id[..16].to_string(),
            description: "Auto-generated documentation".to_string(),
            version: "1.0.0".to_string(),
            author: metadata.deployer.clone(),
            functions,
            structs: vec![],
            enums: vec![],
            events: vec![],
            examples: vec![],
            metadata,
            generated_at: chrono::Utc::now().to_rfc3339(),
        };

        Ok(documentation)
    }

    // Multi-field struct literals read more clearly as sequential pushes
    // than as one large `vec![]` literal.
    #[allow(clippy::vec_init_then_push)]
    fn extract_functions_from_wasm(&self, _wasm_bytes: &[u8]) -> Result<Vec<FunctionDoc>> {
        // Simplified function extraction - in production would use proper WASM parsing
        let mut functions = Vec::new();

        // Add common Soroban contract functions
        functions.push(FunctionDoc {
            name: "__init".to_string(),
            description: "Contract constructor".to_string(),
            inputs: vec![],
            outputs: vec![],
            access: AccessType::Public,
            example: Some("let contract_id = deploy_contract(env);".to_string()),
        });

        functions.push(FunctionDoc {
            name: "invoke".to_string(),
            description: "Generic function invoker".to_string(),
            inputs: vec![ParameterDoc {
                name: "function".to_string(),
                param_type: "Symbol".to_string(),
                description: "Function name to invoke".to_string(),
                optional: false,
            }],
            outputs: vec![],
            access: AccessType::Public,
            example: Some("contract.invoke(env, symbol!(\"my_function\"));".to_string()),
        });

        Ok(functions)
    }

    pub fn add_usage_example(
        &self,
        documentation: &mut ContractDocumentation,
        example: UsageExample,
    ) {
        documentation.examples.push(example);
    }

    pub fn save_documentation(&self, documentation: &ContractDocumentation) -> Result<()> {
        let docs_dir = self.output_dir.join("contracts");
        fs::create_dir_all(&docs_dir)?;

        let doc_path = docs_dir.join(format!("{}.json", documentation.contract_id));
        let json = serde_json::to_string_pretty(documentation)?;
        fs::write(doc_path, json)?;

        // Update index
        self.update_index(documentation)?;

        Ok(())
    }

    fn update_index(&self, documentation: &ContractDocumentation) -> Result<()> {
        let index_path = self.output_dir.join("index.json");

        let mut index = if index_path.exists() {
            let content = fs::read_to_string(&index_path)?;
            serde_json::from_str(&content)?
        } else {
            DocumentationIndex {
                contracts: vec![],
                last_updated: chrono::Utc::now().to_rfc3339(),
            }
        };

        // Check if contract already exists in index
        if let Some(_existing) = index
            .contracts
            .iter()
            .find(|c| c.contract_id == documentation.contract_id)
        {
            // Update existing entry
            let idx = index
                .contracts
                .iter()
                .position(|c| c.contract_id == documentation.contract_id)
                .unwrap();
            index.contracts[idx] = ContractIndexEntry {
                contract_id: documentation.contract_id.clone(),
                name: documentation.name.clone(),
                description: documentation.description.clone(),
                version: documentation.version.clone(),
                tags: vec![],
                network: documentation.metadata.network.clone(),
            };
        } else {
            // Add new entry
            index.contracts.push(ContractIndexEntry {
                contract_id: documentation.contract_id.clone(),
                name: documentation.name.clone(),
                description: documentation.description.clone(),
                version: documentation.version.clone(),
                tags: vec![],
                network: documentation.metadata.network.clone(),
            });
        }

        index.last_updated = chrono::Utc::now().to_rfc3339();

        let json = serde_json::to_string_pretty(&index)?;
        fs::write(index_path, json)?;

        Ok(())
    }
}

pub struct DocumentationPortal {
    docs_dir: PathBuf,
}

impl DocumentationPortal {
    pub fn new(docs_dir: PathBuf) -> Self {
        Self { docs_dir }
    }

    pub fn search(&self, query: &str) -> Result<Vec<ContractIndexEntry>> {
        let index_path = self.docs_dir.join("index.json");

        if !index_path.exists() {
            return Ok(vec![]);
        }

        let content = fs::read_to_string(&index_path)?;
        let index: DocumentationIndex = serde_json::from_str(&content)?;

        let query_lower = query.to_lowercase();
        let results: Vec<ContractIndexEntry> = index
            .contracts
            .into_iter()
            .filter(|entry| {
                entry.name.to_lowercase().contains(&query_lower)
                    || entry.description.to_lowercase().contains(&query_lower)
                    || entry.contract_id.to_lowercase().contains(&query_lower)
            })
            .collect();

        Ok(results)
    }

    pub fn get_documentation(&self, contract_id: &str) -> Result<ContractDocumentation> {
        let doc_path = self
            .docs_dir
            .join("contracts")
            .join(format!("{}.json", contract_id));

        if !doc_path.exists() {
            anyhow::bail!("Documentation not found for contract: {}", contract_id);
        }

        let content = fs::read_to_string(&doc_path)?;
        let documentation: ContractDocumentation = serde_json::from_str(&content)?;

        Ok(documentation)
    }

    pub fn list_contracts(&self) -> Result<Vec<ContractIndexEntry>> {
        let index_path = self.docs_dir.join("index.json");

        if !index_path.exists() {
            return Ok(vec![]);
        }

        let content = fs::read_to_string(&index_path)?;
        let index: DocumentationIndex = serde_json::from_str(&content)?;

        Ok(index.contracts)
    }

    pub fn generate_html_portal(&self, output_path: &Path) -> Result<()> {
        let contracts = self.list_contracts()?;

        let html = self.generate_html_index(&contracts)?;
        fs::write(output_path, html)?;

        Ok(())
    }

    fn generate_html_index(&self, contracts: &[ContractIndexEntry]) -> Result<String> {
        let mut html = String::from(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>StarForge Contract Documentation Portal</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .header { text-align: center; margin-bottom: 40px; }
        .header h1 { color: #333; margin-bottom: 10px; }
        .search-box { margin: 20px 0; }
        .search-box input { width: 100%; padding: 12px; border: 1px solid #ddd; border-radius: 4px; font-size: 16px; }
        .contract-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 20px; }
        .contract-card { border: 1px solid #e0e0e0; border-radius: 8px; padding: 20px; transition: transform 0.2s; }
        .contract-card:hover { transform: translateY(-5px); box-shadow: 0 4px 8px rgba(0,0,0,0.1); }
        .contract-card h3 { margin: 0 0 10px 0; color: #0066cc; }
        .contract-card .contract-id { font-family: monospace; color: #666; font-size: 12px; margin-bottom: 10px; }
        .contract-card .description { color: #555; line-height: 1.5; }
        .contract-card .network { display: inline-block; background: #e8f4f8; padding: 4px 8px; border-radius: 4px; font-size: 12px; color: #0066cc; margin-top: 10px; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>⚡ StarForge Contract Documentation Portal</h1>
            <p>Explore and interact with deployed Soroban contracts</p>
        </div>
        
        <div class="search-box">
            <input type="text" id="search" placeholder="Search contracts by name, description, or contract ID..." onkeyup="filterContracts()">
        </div>
        
        <div class="contract-grid" id="contractGrid">
"#,
        );

        for contract in contracts {
            html.push_str(&format!(
                r#"            <div class="contract-card" data-name="{}" data-description="{}" data-contract-id="{}">
                <h3>{}</h3>
                <div class="contract-id">{}</div>
                <div class="description">{}</div>
                <div class="network">{}</div>
            </div>
"#,
                contract.name.to_lowercase(),
                contract.description.to_lowercase(),
                contract.contract_id.to_lowercase(),
                contract.name,
                contract.contract_id,
                contract.description,
                contract.network
            ));
        }

        html.push_str(r#"        </div>
    </div>
    
    <script>
        function filterContracts() {
            const search = document.getElementById('search').value.toLowerCase();
            const cards = document.querySelectorAll('.contract-card');
            
            cards.forEach(card => {
                const name = card.dataset.name;
                const description = card.dataset.description;
                const contractId = card.dataset.contractId;
                
                if (name.includes(search) || description.includes(search) || contractId.includes(search)) {
                    card.style.display = 'block';
                } else {
                    card.style.display = 'none';
                }
            });
        }
    </script>
</body>
</html>"#);

        Ok(html)
    }

    pub fn generate_contract_html(&self, contract_id: &str, output_path: &Path) -> Result<()> {
        let documentation = self.get_documentation(contract_id)?;
        let html = self.generate_contract_detail_html(&documentation)?;
        fs::write(output_path, html)?;
        Ok(())
    }

    fn generate_contract_detail_html(&self, doc: &ContractDocumentation) -> Result<String> {
        let mut functions_html = String::new();

        for func in &doc.functions {
            functions_html.push_str(&format!(
                r#"        <div class="function">
            <h4>{} <span class="access-badge">{:?}</span></h4>
            <p class="description">{}</p>
            <div class="parameters">
                <strong>Inputs:</strong>
                <ul>
                    {}
                </ul>
            </div>
            <div class="example">
                <strong>Example:</strong>
                <pre><code>{}</code></pre>
            </div>
        </div>
"#,
                func.name,
                func.access,
                func.description,
                func.inputs
                    .iter()
                    .map(|p| format!("<li>{}: {} - {}</li>", p.name, p.param_type, p.description))
                    .collect::<Vec<_>>()
                    .join("\n                    "),
                func.example.as_deref().unwrap_or("No example available")
            ));
        }

        let mut examples_html = String::new();

        for example in &doc.examples {
            examples_html.push_str(&format!(
                r#"        <div class="example-block">
            <h4>{}</h4>
            <p>{}</p>
            <pre><code class="language-{}">{}</code></pre>
        </div>
"#,
                example.title, example.description, example.language, example.code
            ));
        }

        Ok(format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>{} - StarForge Documentation</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }}
        .container {{ max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .header {{ margin-bottom: 30px; border-bottom: 2px solid #0066cc; padding-bottom: 20px; }}
        .header h1 {{ color: #333; margin: 0; }}
        .metadata {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 15px; margin: 20px 0; }}
        .metadata-item {{ background: #f8f9fa; padding: 10px; border-radius: 4px; }}
        .metadata-item strong {{ color: #0066cc; }}
        .section {{ margin: 30px 0; }}
        .section h2 {{ color: #333; border-bottom: 1px solid #e0e0e0; padding-bottom: 10px; }}
        .function {{ background: #f8f9fa; padding: 20px; border-radius: 8px; margin: 15px 0; }}
        .function h4 {{ margin: 0 0 10px 0; color: #0066cc; }}
        .access-badge {{ background: #e8f4f8; padding: 2px 8px; border-radius: 4px; font-size: 12px; color: #0066cc; }}
        .parameters {{ margin: 10px 0; }}
        .parameters ul {{ margin: 5px 0; padding-left: 20px; }}
        .example {{ margin: 10px 0; }}
        .example pre {{ background: #2d2d2d; color: #f8f8f2; padding: 15px; border-radius: 4px; overflow-x: auto; }}
        .example code {{ font-family: 'Courier New', monospace; }}
        .example-block {{ background: #f8f9fa; padding: 20px; border-radius: 8px; margin: 15px 0; }}
        .example-block h4 {{ margin: 0 0 10px 0; color: #0066cc; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>{}</h1>
            <p>{}</p>
        </div>
        
        <div class="metadata">
            <div class="metadata-item"><strong>Contract ID:</strong> {}</div>
            <div class="metadata-item"><strong>Version:</strong> {}</div>
            <div class="metadata-item"><strong>Author:</strong> {}</div>
            <div class="metadata-item"><strong>Network:</strong> {}</div>
            <div class="metadata-item"><strong>Soroban Version:</strong> {}</div>
        </div>
        
        <div class="section">
            <h2>Functions</h2>
            {}
        </div>
        
        <div class="section">
            <h2>Usage Examples</h2>
            {}
        </div>
    </div>
</body>
</html>"#,
            doc.name,
            doc.name,
            doc.description,
            doc.contract_id,
            doc.version,
            doc.author,
            doc.metadata.network,
            doc.metadata.soroban_version,
            functions_html,
            examples_html
        ))
    }
}

pub struct DocumentationVersionManager {
    versions_dir: PathBuf,
}

impl DocumentationVersionManager {
    pub fn new(versions_dir: PathBuf) -> Self {
        Self { versions_dir }
    }

    pub fn create_version(
        &self,
        contract_id: &str,
        documentation: ContractDocumentation,
        changelog: &str,
    ) -> Result<DocumentationVersion> {
        let version = DocumentationVersion {
            version: documentation.version.clone(),
            contract_id: contract_id.to_string(),
            documentation,
            created_at: chrono::Utc::now().to_rfc3339(),
            changelog: changelog.to_string(),
        };

        let contract_versions_dir = self.versions_dir.join(contract_id);
        fs::create_dir_all(&contract_versions_dir)?;

        let version_path = contract_versions_dir.join(format!("{}.json", version.version));
        let json = serde_json::to_string_pretty(&version)?;
        fs::write(version_path, json)?;

        Ok(version)
    }

    pub fn list_versions(&self, contract_id: &str) -> Result<Vec<DocumentationVersion>> {
        let contract_versions_dir = self.versions_dir.join(contract_id);

        if !contract_versions_dir.exists() {
            return Ok(vec![]);
        }

        let mut versions = Vec::new();

        for entry in fs::read_dir(&contract_versions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let version: DocumentationVersion = serde_json::from_str(&content)?;
                versions.push(version);
            }
        }

        // Sort by version (simplified - would use proper semver in production)
        versions.sort_by(|a, b| b.version.cmp(&a.version));

        Ok(versions)
    }

    pub fn get_version(&self, contract_id: &str, version: &str) -> Result<DocumentationVersion> {
        let version_path = self
            .versions_dir
            .join(contract_id)
            .join(format!("{}.json", version));

        if !version_path.exists() {
            anyhow::bail!("Version {} not found for contract {}", version, contract_id);
        }

        let content = fs::read_to_string(&version_path)?;
        let version_doc: DocumentationVersion = serde_json::from_str(&content)?;

        Ok(version_doc)
    }
}
