use anyhow::{Context, Result};
use std::io::Cursor;
use std::path::Path;
use stellar_xdr::curr::{
    Limited, Limits, ReadXdr, ScSpecEntry, ScSpecFunctionV0, ScSpecTypeDef, ScSpecUdtEnumV0,
    ScSpecUdtErrorEnumV0, ScSpecUdtStructV0, ScSpecUdtUnionCaseV0, ScSpecUdtUnionV0, WriteXdr,
};

pub mod typescript;

pub use typescript::{
    generate_typescript_package, write_generated_files, GeneratedFile, TsModuleFormat,
    TsPackageOptions, WriteReport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingLanguage {
    Rust,
    TypeScript,
    Python,
    Go,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMetadata {
    pub functions: Vec<ContractFunction>,
    pub structs: Vec<ContractStruct>,
    pub enums: Vec<ContractEnum>,
    pub events: Vec<ContractEvent>,
    /// Tagged unions (`ScSpecUdtUnionV0`). Every variant is rendered as a
    /// discriminated-union member, even when it carries no data.
    pub unions: Vec<ContractEnum>,
    /// Contract error enums (`ScSpecUdtErrorEnumV0`) with their numeric codes.
    pub errors: Vec<ContractErrorEnum>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractFunction {
    pub name: String,
    pub inputs: Vec<ContractInput>,
    pub output: Option<String>,
    /// Documentation comment from the contract spec (empty when absent).
    pub doc: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractInput {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractStruct {
    pub name: String,
    pub fields: Vec<ContractField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractField {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractEnum {
    pub name: String,
    pub variants: Vec<ContractVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractVariant {
    pub name: String,
    pub type_name: Option<String>,
    /// Integer discriminant for C-like enums (`None` for union cases).
    pub value: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractErrorEnum {
    pub name: String,
    pub cases: Vec<ContractErrorCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractErrorCase {
    pub name: String,
    pub value: u32,
    pub doc: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractEvent {
    pub name: String,
    pub fields: Vec<ContractField>,
}

pub fn generate_bindings(wasm_path: &Path, language: BindingLanguage) -> Result<String> {
    let metadata = load_metadata(wasm_path)?;
    generate_from_metadata(&metadata, language)
}

/// Read a contract spec from `path` and parse it into [`ContractMetadata`].
///
/// The input may be a compiled WASM (the spec is read from its
/// `contractspecv0` custom section), a raw XDR stream of `ScSpecEntry`
/// values, or that XDR encoded as base64 (a single blob, or one entry per
/// whitespace-separated chunk).
pub fn load_metadata(path: &Path) -> Result<ContractMetadata> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read contract input {}", path.display()))?;
    let entries = read_spec_input(&bytes)?;
    let metadata = parse_spec_entries(&entries);
    if metadata.functions.is_empty() {
        anyhow::bail!("No contract functions found in WASM metadata");
    }
    Ok(metadata)
}

/// Decode spec entries from WASM, raw XDR, or base64 XDR bytes.
pub fn read_spec_input(bytes: &[u8]) -> Result<Vec<ScSpecEntry>> {
    if bytes.starts_with(b"\0asm") {
        return read_spec_entries(bytes);
    }
    if bytes.is_empty() {
        anyhow::bail!("Input is not a valid WASM binary or contract spec (file is empty)");
    }
    if let Ok(entries) = decode_spec_xdr(bytes) {
        if !entries.is_empty() {
            return Ok(entries);
        }
    }
    if let Some(entries) = decode_spec_base64(bytes) {
        return Ok(entries);
    }
    anyhow::bail!(
        "Input is not a valid WASM binary or contract spec (expected WASM, raw XDR, or base64 XDR)"
    )
}

fn decode_spec_base64(bytes: &[u8]) -> Option<Vec<ScSpecEntry>> {
    use base64::Engine as _;
    let text = std::str::from_utf8(bytes).ok()?;
    let engine = base64::engine::general_purpose::STANDARD;
    let mut entries = Vec::new();
    for chunk in text.split_whitespace() {
        let raw = engine.decode(chunk).ok()?;
        entries.extend(decode_spec_xdr(&raw).ok()?);
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn decode_spec_xdr(bytes: &[u8]) -> Result<Vec<ScSpecEntry>> {
    let cursor = Cursor::new(bytes);
    ScSpecEntry::read_xdr_iter(&mut Limited::new(
        cursor,
        Limits {
            depth: 500,
            len: 0x1000000,
        },
    ))
    .collect::<std::result::Result<Vec<_>, _>>()
    .context("Failed to decode contract spec XDR")
}

/// Build a minimal WASM module whose `contractspecv0` custom section holds
/// `entries`. Useful for fixtures and tests that need a "compiled" contract
/// without a Soroban toolchain.
pub fn wasm_with_spec(entries: &[ScSpecEntry]) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    for entry in entries {
        payload.extend(
            entry
                .to_xdr(Limits::none())
                .context("Failed to encode spec entry as XDR")?,
        );
    }
    let name = b"contractspecv0";
    let mut section = Vec::new();
    write_var_u32(&mut section, name.len() as u32);
    section.extend_from_slice(name);
    section.extend(payload);

    let mut wasm = b"\0asm\x01\x00\x00\x00".to_vec();
    wasm.push(0);
    write_var_u32(&mut wasm, section.len() as u32);
    wasm.extend(section);
    Ok(wasm)
}

fn write_var_u32(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Generate a language binding from already-parsed contract metadata.
///
/// This is the single dispatch point used both by [`generate_bindings`] (which
/// reads the WASM spec first) and by tests that build metadata directly.
pub fn generate_from_metadata(
    metadata: &ContractMetadata,
    language: BindingLanguage,
) -> Result<String> {
    match language {
        BindingLanguage::Rust => Ok(generate_rust(metadata)),
        BindingLanguage::TypeScript => typescript::generate_single_file(metadata),
        BindingLanguage::Python => Ok(generate_python(metadata)),
        BindingLanguage::Go => Ok(generate_go(metadata)),
    }
}

/// One file of a generated package, relative to the package's output
/// directory (e.g. `"pyproject.toml"`, `"my_contract/client.py"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
    pub relative_path: String,
    pub contents: String,
}

/// Generates a complete, installable Python package for a contract's
/// bindings (#720): a `pyproject.toml` with build metadata plus a package
/// directory containing `__init__.py` and `client.py`. The generated
/// `client.py` reuses [`generate_python`]'s existing client-code generation
/// unchanged; this function only adds the packaging layer around it so the
/// output can be installed with `pip install .` rather than pasted by hand.
///
/// `package_name` is normalized into a valid Python distribution name
/// (PEP 503: lowercase, hyphens) and a valid importable module name
/// (PEP 8: lowercase, underscores) derived from it.
pub fn generate_python_package(
    metadata: &ContractMetadata,
    package_name: &str,
) -> Vec<PackageFile> {
    let module_name = python_module_name(package_name);
    let distribution_name = python_distribution_name(package_name);
    let client_source = generate_python(metadata);

    let pyproject = format!(
        "[build-system]\n\
         requires = [\"setuptools>=68\"]\n\
         build-backend = \"setuptools.build_meta\"\n\n\
         [project]\n\
         name = \"{distribution_name}\"\n\
         version = \"0.1.0\"\n\
         description = \"Generated Soroban contract client for {distribution_name}\"\n\
         requires-python = \">=3.10\"\n\
         readme = \"README.md\"\n\n\
         [tool.setuptools.packages.find]\n\
         include = [\"{module_name}*\"]\n",
        distribution_name = distribution_name,
        module_name = module_name,
    );

    let readme = format!(
        "# {distribution_name}\n\n\
         Generated Soroban contract client. Install locally with:\n\n\
         ```bash\n\
         pip install .\n\
         ```\n\n\
         Then invoke:\n\n\
         ```python\n\
         from {module_name} import ContractClient, ContractClientOptions\n\n\
         client = ContractClient(ContractClientOptions(contract_id=\"C...\", network=\"testnet\"))\n\
         args = client.some_function(...)  # returns starforge CLI invocation args\n\
         ```\n",
        distribution_name = distribution_name,
        module_name = module_name,
    );

    let init_py = format!(
        "from .client import ContractClient, ContractClientOptions\n\n\
         __all__ = [\"ContractClient\", \"ContractClientOptions\"]\n\
         __version__ = \"0.1.0\"\n"
    );

    vec![
        PackageFile {
            relative_path: "pyproject.toml".to_string(),
            contents: pyproject,
        },
        PackageFile {
            relative_path: "README.md".to_string(),
            contents: readme,
        },
        PackageFile {
            relative_path: format!("{}/__init__.py", module_name),
            contents: init_py,
        },
        PackageFile {
            relative_path: format!("{}/client.py", module_name),
            contents: client_source,
        },
    ]
}

/// Writes a generated package's files to `output_dir`, creating parent
/// directories as needed. Used by the CLI when `--output-dir` is supplied
/// for a Python target.
pub fn write_package(output_dir: &Path, files: &[PackageFile]) -> Result<()> {
    for file in files {
        let path = output_dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        std::fs::write(&path, &file.contents)
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(())
}

/// A valid, importable Python module name: lowercase, `_` separators,
/// starting with a letter or underscore (PEP 8).
fn python_module_name(input: &str) -> String {
    if !input.chars().any(|c| c.is_ascii_alphanumeric()) {
        return "contract_client".to_string();
    }

    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// A valid PEP 503 Python distribution (package) name: lowercase with
/// hyphen separators.
fn python_distribution_name(input: &str) -> String {
    python_module_name(input).replace('_', "-")
}

pub fn read_spec_entries(wasm: &[u8]) -> Result<Vec<ScSpecEntry>> {
    let spec = contract_spec_section(wasm)?;
    let cursor = Cursor::new(spec);
    let entries = ScSpecEntry::read_xdr_iter(&mut Limited::new(
        cursor,
        Limits {
            depth: 500,
            len: 0x1000000,
        },
    ))
    .collect::<std::result::Result<Vec<_>, _>>()
    .context("Failed to decode contractspecv0 XDR metadata")?;
    Ok(entries)
}

pub fn contract_spec_section(wasm: &[u8]) -> Result<&[u8]> {
    if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
        anyhow::bail!("Input is not a valid WASM binary");
    }

    let mut offset = 8;
    while offset < wasm.len() {
        let section_id = wasm[offset];
        offset += 1;
        let section_len = read_var_u32(wasm, &mut offset)? as usize;
        let section_end = offset
            .checked_add(section_len)
            .filter(|end| *end <= wasm.len())
            .ok_or_else(|| anyhow::anyhow!("Malformed WASM section length"))?;

        if section_id == 0 {
            let mut section_offset = offset;
            let name_len = read_var_u32(wasm, &mut section_offset)? as usize;
            let name_end = section_offset
                .checked_add(name_len)
                .filter(|end| *end <= section_end)
                .ok_or_else(|| anyhow::anyhow!("Malformed WASM custom section name"))?;
            let name = std::str::from_utf8(&wasm[section_offset..name_end])
                .context("WASM custom section name is not UTF-8")?;
            if name == "contractspecv0" {
                return Ok(&wasm[name_end..section_end]);
            }
        }

        offset = section_end;
    }

    anyhow::bail!("No contractspecv0 metadata section found in WASM")
}

pub fn read_var_u32(bytes: &[u8], offset: &mut usize) -> Result<u32> {
    let mut result = 0u32;
    let mut shift = 0;

    loop {
        let byte = *bytes
            .get(*offset)
            .ok_or_else(|| anyhow::anyhow!("Unexpected end of WASM while reading LEB128"))?;
        *offset += 1;
        result |= ((byte & 0x7f) as u32) << shift;

        if byte & 0x80 == 0 {
            return Ok(result);
        }

        shift += 7;
        if shift >= 35 {
            anyhow::bail!("LEB128 integer overflow");
        }
    }
}

pub fn parse_spec_entries(entries: &[ScSpecEntry]) -> ContractMetadata {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut events = Vec::new();
    let mut unions = Vec::new();
    let mut errors = Vec::new();

    for entry in entries {
        match entry {
            ScSpecEntry::FunctionV0(function) => {
                functions.push(contract_function(function));
            }
            ScSpecEntry::UdtStructV0(udt) => {
                structs.push(contract_struct(udt));
            }
            ScSpecEntry::UdtEnumV0(udt) => {
                enums.push(contract_enum(udt));
            }
            ScSpecEntry::UdtErrorEnumV0(error_enum) => {
                errors.push(contract_error_enum(error_enum));
                // Also kept as event-like records for the Rust/Python/Go
                // generators, which predate first-class error enums.
                events.push(ContractEvent {
                    name: error_enum.name.to_string(),
                    fields: error_enum
                        .cases
                        .iter()
                        .map(|case| ContractField {
                            name: case.name.to_string(),
                            type_name: "String".to_string(), // Error messages as strings
                        })
                        .collect(),
                });
            }
            ScSpecEntry::UdtUnionV0(udt) => {
                unions.push(contract_union(udt));
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    ContractMetadata {
        functions,
        structs,
        enums,
        events,
        unions,
        errors,
    }
}

fn contract_error_enum(udt: &ScSpecUdtErrorEnumV0) -> ContractErrorEnum {
    ContractErrorEnum {
        name: udt.name.to_string(),
        cases: udt
            .cases
            .iter()
            .map(|case| ContractErrorCase {
                name: case.name.to_string(),
                value: case.value,
                doc: case.doc.to_string(),
            })
            .collect(),
    }
}

fn contract_union(udt: &ScSpecUdtUnionV0) -> ContractEnum {
    ContractEnum {
        name: udt.name.to_string(),
        variants: udt
            .cases
            .iter()
            .map(|case| match case {
                ScSpecUdtUnionCaseV0::VoidV0(void) => ContractVariant {
                    name: void.name.to_string(),
                    type_name: None,
                    value: None,
                },
                ScSpecUdtUnionCaseV0::TupleV0(tuple) => {
                    let types = tuple.type_.iter().map(spec_type_name).collect::<Vec<_>>();
                    let type_name = if types.len() == 1 {
                        types[0].clone()
                    } else {
                        format!("({})", types.join(", "))
                    };
                    ContractVariant {
                        name: tuple.name.to_string(),
                        type_name: Some(type_name),
                        value: None,
                    }
                }
            })
            .collect(),
    }
}

fn contract_function(function: &ScSpecFunctionV0) -> ContractFunction {
    ContractFunction {
        name: function.name.to_string(),
        inputs: function
            .inputs
            .iter()
            .map(|input| ContractInput {
                name: input.name.to_string(),
                type_name: spec_type_name(&input.type_),
            })
            .collect(),
        output: function.outputs.first().map(spec_type_name),
        doc: function.doc.to_string(),
    }
}

fn contract_struct(udt: &ScSpecUdtStructV0) -> ContractStruct {
    ContractStruct {
        name: udt.name.to_string(),
        fields: udt
            .fields
            .iter()
            .map(|field| ContractField {
                name: field.name.to_string(),
                type_name: spec_type_name(&field.type_),
            })
            .collect(),
    }
}

fn contract_enum(udt: &ScSpecUdtEnumV0) -> ContractEnum {
    ContractEnum {
        name: udt.name.to_string(),
        variants: udt
            .cases
            .iter()
            .map(|case| ContractVariant {
                name: case.name.to_string(),
                type_name: None,
                value: Some(case.value),
            })
            .collect(),
    }
}

fn spec_type_name(type_def: &ScSpecTypeDef) -> String {
    match type_def {
        ScSpecTypeDef::Val => "Val".to_string(),
        ScSpecTypeDef::Bool => "bool".to_string(),
        ScSpecTypeDef::Void => "()".to_string(),
        ScSpecTypeDef::Error => "Error".to_string(),
        ScSpecTypeDef::U32 => "u32".to_string(),
        ScSpecTypeDef::I32 => "i32".to_string(),
        ScSpecTypeDef::U64 => "u64".to_string(),
        ScSpecTypeDef::I64 => "i64".to_string(),
        ScSpecTypeDef::Timepoint => "u64".to_string(),
        ScSpecTypeDef::Duration => "u64".to_string(),
        ScSpecTypeDef::U128 => "u128".to_string(),
        ScSpecTypeDef::I128 => "i128".to_string(),
        ScSpecTypeDef::U256 => "U256".to_string(),
        ScSpecTypeDef::I256 => "I256".to_string(),
        ScSpecTypeDef::Bytes => "Bytes".to_string(),
        ScSpecTypeDef::String => "String".to_string(),
        ScSpecTypeDef::Symbol => "Symbol".to_string(),
        ScSpecTypeDef::Address => "Address".to_string(),
        ScSpecTypeDef::Option(inner) => format!("Option<{}>", spec_type_name(&inner.value_type)),
        ScSpecTypeDef::Result(inner) => format!(
            "Result<{}, {}>",
            spec_type_name(&inner.ok_type),
            spec_type_name(&inner.error_type)
        ),
        ScSpecTypeDef::Vec(inner) => format!("Vec<{}>", spec_type_name(&inner.element_type)),
        ScSpecTypeDef::Map(inner) => format!(
            "Map<{}, {}>",
            spec_type_name(&inner.key_type),
            spec_type_name(&inner.value_type)
        ),
        ScSpecTypeDef::Tuple(inner) => {
            let types = inner
                .value_types
                .iter()
                .map(spec_type_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", types)
        }
        ScSpecTypeDef::BytesN(inner) => format!("BytesN<{}>", inner.n),
        ScSpecTypeDef::Udt(inner) => inner.name.to_string(),
    }
}

/// Pinned Soroban SDK version used for generated Rust client crates and interfaces.
pub const PINNED_SOROBAN_SDK_VERSION: &str = "22.0.0";

/// Pinned Stellar XDR version used for generated Rust client crates and interfaces.
pub const PINNED_STELLAR_XDR_VERSION: &str = "22.0.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCrateOptions {
    pub crate_name: String,
    pub crate_version: String,
    pub description: Option<String>,
    pub soroban_sdk_version: String,
    pub stellar_xdr_version: String,
    pub authors: Vec<String>,
    pub edition: String,
    pub no_std: bool,
}

impl Default for RustCrateOptions {
    fn default() -> Self {
        Self {
            crate_name: "contract-client".to_string(),
            crate_version: "0.1.0".to_string(),
            description: Some(
                "Generated StarForge typed client crate for Soroban smart contract".to_string(),
            ),
            soroban_sdk_version: PINNED_SOROBAN_SDK_VERSION.to_string(),
            stellar_xdr_version: PINNED_STELLAR_XDR_VERSION.to_string(),
            authors: vec!["StarForge Generator <starforge@example.com>".to_string()],
            edition: "2021".to_string(),
            no_std: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedRustCrate {
    pub cargo_toml: String,
    pub lib_rs: String,
    pub readme: String,
}

/// Generate a complete Cargo-compatible crate skeleton from contract metadata.
pub fn generate_rust_crate(
    metadata: &ContractMetadata,
    options: &RustCrateOptions,
) -> GeneratedRustCrate {
    let cargo_toml = generate_cargo_toml(options);
    let lib_rs = generate_rust_lib(metadata, options);
    let readme = generate_crate_readme(metadata, options);

    GeneratedRustCrate {
        cargo_toml,
        lib_rs,
        readme,
    }
}

/// Emit a complete Cargo-compatible crate skeleton into a destination directory.
pub fn emit_rust_crate(
    metadata: &ContractMetadata,
    options: &RustCrateOptions,
    destination: &Path,
) -> Result<()> {
    let crate_data = generate_rust_crate(metadata, options);
    std::fs::create_dir_all(destination.join("src")).with_context(|| {
        format!(
            "Failed to create crate src directory in {}",
            destination.display()
        )
    })?;

    std::fs::write(destination.join("Cargo.toml"), &crate_data.cargo_toml)
        .with_context(|| format!("Failed to write Cargo.toml in {}", destination.display()))?;

    std::fs::write(destination.join("src").join("lib.rs"), &crate_data.lib_rs)
        .with_context(|| format!("Failed to write src/lib.rs in {}", destination.display()))?;

    std::fs::write(destination.join("README.md"), &crate_data.readme)
        .with_context(|| format!("Failed to write README.md in {}", destination.display()))?;

    Ok(())
}

/// Generate a full client crate from a WASM file.
pub fn generate_crate_from_wasm(
    wasm_path: &Path,
    options: &RustCrateOptions,
    destination: &Path,
) -> Result<()> {
    let wasm = std::fs::read(wasm_path)
        .with_context(|| format!("Failed to read WASM file {}", wasm_path.display()))?;
    let entries = read_spec_entries(&wasm)?;
    let metadata = parse_spec_entries(&entries);

    if metadata.functions.is_empty() {
        anyhow::bail!("No contract functions found in WASM metadata");
    }

    emit_rust_crate(&metadata, options, destination)
}

/// Generate Cargo.toml manifest with feature flags for backend and environment selection.
pub fn generate_cargo_toml(options: &RustCrateOptions) -> String {
    let description = options
        .description
        .as_deref()
        .unwrap_or("Generated StarForge typed client crate for Soroban smart contract");
    let authors_line = if options.authors.is_empty() {
        "".to_string()
    } else {
        format!(
            "authors = [{}]\n",
            options
                .authors
                .iter()
                .map(|a| format!("\"{}\"", a))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        r#"[package]
name = "{crate_name}"
version = "{crate_version}"
edition = "{edition}"
rust-version = "1.80"
description = "{description}"
{authors_line}
[features]
default = ["std"]
std = [
    "serde/std",
    "dep:thiserror",
    "dep:anyhow",
]
no_std = []
cli-backend = ["std", "dep:anyhow"]
rpc-backend = ["std", "dep:reqwest", "dep:tokio", "dep:serde_json"]
testutils = ["soroban-sdk/testutils"]

[dependencies]
# Pinned Soroban SDK and Stellar XDR versions for protocol stability
soroban-sdk = {{ version = "={soroban_version}", default-features = false, optional = true }}
stellar-xdr = {{ version = "={xdr_version}", default-features = false, features = ["alloc"] }}
serde = {{ version = "1.0", default-features = false, features = ["derive"] }}
thiserror = {{ version = "1.0", optional = true }}
anyhow = {{ version = "1.0", optional = true }}
reqwest = {{ version = "0.11", default-features = false, features = ["json", "rustls-tls"], optional = true }}
tokio = {{ version = "1", default-features = false, features = ["rt", "macros"], optional = true }}
serde_json = {{ version = "1.0", optional = true }}
"#,
        crate_name = options.crate_name,
        crate_version = options.crate_version,
        edition = options.edition,
        description = description,
        authors_line = authors_line,
        soroban_version = options.soroban_sdk_version,
        xdr_version = options.stellar_xdr_version,
    )
}

/// Generate README.md explaining crate layout, versioning policy, and usage.
pub fn generate_crate_readme(metadata: &ContractMetadata, options: &RustCrateOptions) -> String {
    let mut functions_list = String::new();
    for f in &metadata.functions {
        let params = f
            .inputs
            .iter()
            .map(|i| format!("{}: {}", i.name, i.type_name))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = f.output.as_deref().unwrap_or("()");
        functions_list.push_str(&format!("- `fn {}({}) -> {}`\n", f.name, params, ret));
    }

    format!(
        r#"# {crate_name}

Generated StarForge typed client crate for Soroban smart contracts.

## Soroban Compatibility & Pinned Versions
- **Soroban SDK**: `={soroban_version}`
- **Stellar XDR**: `={xdr_version}`

## Crate Layout
```text
.
├── Cargo.toml      # Package manifest with network backend feature flags
├── README.md       # Crate documentation, versioning policy, and quickstart
└── src/
    └── lib.rs      # Typed client implementation, argument encoders, data structures
```

## Feature Flags
| Feature | Description |
|---|---|
| `std` (default) | Standard library support with rich error conversions and CLI execution. |
| `no_std` | Lightweight zero-allocation client data structures suitable for WASM and embedded callers. |
| `cli-backend` | Invokes the contract through the StarForge CLI command runner. |
| `rpc-backend` | Direct async RPC client backend for network transactions. |
| `testutils` | In-memory Soroban test environment integration with `soroban-sdk`. |

## Versioning Policy
This crate adheres to [Semantic Versioning](https://semver.org/).
Contract client versions are locked against explicit Soroban SDK releases (`{soroban_version}`) to guarantee wire-level XDR encoding compatibility and prevent silent ABI drift.

## Available Contract Functions
{functions_list}
## Usage Example

```rust,no_run
use {crate_name}::ContractClient;

fn main() -> Result<(), Box<dyn std::error::Error>> {{
    let client = ContractClient::new("CA...", "testnet")
        .with_wallet("alice");

    // Invoke typed contract functions directly:
    // let result = client.balance_of("G...".to_string())?;
    Ok(())
}}
```
"#,
        crate_name = options.crate_name,
        soroban_version = options.soroban_sdk_version,
        xdr_version = options.stellar_xdr_version,
        functions_list = functions_list,
    )
}

/// Generate `src/lib.rs` for the client crate.
pub fn generate_rust_lib(metadata: &ContractMetadata, options: &RustCrateOptions) -> String {
    let mut out = format!(
        r#"//! Generated StarForge client crate for Soroban contract.
//!
//! Pinned Soroban SDK: {soroban_version}
//! Pinned Stellar XDR: {xdr_version}
//!
//! # Crate Layout
//! - `Cargo.toml`: Package configuration with feature flags for backend and environment selection.
//! - `src/lib.rs`: Type-safe client, argument serialization, and contract data types.
//! - `README.md`: Usage documentation, feature flags, and versioning policy.
//!
//! # Feature Flags
//! - `std` (default): Standard library support, error reporting with `thiserror`/`std::error::Error`.
//! - `no_std`: Zero-allocation / embedded / WASM client compatibility.
//! - `cli-backend`: CLI-based execution invoking StarForge commands.
//! - `rpc-backend`: Direct JSON-RPC Soroban network backend.
//! - `testutils`: In-memory Soroban test environment integration.
//!
//! # Versioning Policy
//! This crate follows Semantic Versioning (SemVer). The client interface is pinned against
//! Soroban SDK {soroban_version} to ensure deterministic wire encoding and execution.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{{
    borrow::ToOwned,
    format,
    string::{{String, ToString}},
    vec,
    vec::Vec,
}};

#[cfg(feature = "std")]
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {{
    Execution(String),
    Serialization(String),
    Deserialization(String),
}}

impl core::fmt::Display for ClientError {{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{
        match self {{
            Self::Execution(e) => write!(f, "Contract execution error: {{}}", e),
            Self::Serialization(e) => write!(f, "Argument serialization error: {{}}", e),
            Self::Deserialization(e) => write!(f, "Result deserialization error: {{}}", e),
        }}
    }}
}}

#[cfg(feature = "std")]
impl std::error::Error for ClientError {{}}

pub type Result<T> = core::result::Result<T, ClientError>;

pub struct ContractClient {{
    pub contract_id: String,
    pub network: String,
    pub wallet: Option<String>,
}}

impl ContractClient {{
    pub fn new(contract_id: impl Into<String>, network: impl Into<String>) -> Self {{
        Self {{
            contract_id: contract_id.into(),
            network: network.into(),
            wallet: None,
        }}
    }}

    pub fn with_wallet(mut self, wallet: impl Into<String>) -> Self {{
        self.wallet = Some(wallet.into());
        self
    }}

    pub fn with_network(mut self, network: impl Into<String>) -> Self {{
        self.network = network.into();
        self
    }}

    #[cfg(feature = "std")]
    fn execute_command(&self, mut cmd: Command) -> Result<String> {{
        let output = cmd
            .output()
            .map_err(|e| ClientError::Execution(e.to_string()))?;
        if output.status.success() {{
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }} else {{
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ClientError::Execution(format!("Command failed: {{}}", stderr)))
        }}
    }}

    pub fn serialize_arg<T: core::fmt::Display>(&self, value: &T) -> Result<String> {{
        Ok(value.to_string())
    }}

    pub fn parse_result<T>(&self, result: &str) -> Result<T>
    where
        T: core::str::FromStr,
        T::Err: core::fmt::Display,
    {{
        result
            .parse()
            .map_err(|e| ClientError::Deserialization(format!("{{}}", e)))
    }}

    pub fn build_cli_args(&self, function: &str, args: &[(&str, &str)]) -> Vec<String> {{
        let mut cli = vec![
            "contract".to_string(),
            "invoke".to_string(),
            self.contract_id.clone(),
            function.to_string(),
            "--network".to_string(),
            self.network.clone(),
        ];
        for (val, ty) in args {{
            cli.push("--arg".to_string());
            cli.push((*val).to_string());
            cli.push("--type".to_string());
            cli.push((*ty).to_string());
        }}
        if let Some(w) = &self.wallet {{
            cli.push("--wallet".to_string());
            cli.push(w.clone());
            cli.push("--submit".to_string());
        }}
        cli
    }}
"#,
        soroban_version = options.soroban_sdk_version,
        xdr_version = options.stellar_xdr_version,
    );

    for function in &metadata.functions {
        let rust_name = sanitize_ident(&function.name);
        let params = function
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{}: {}",
                    sanitize_ident(&input.name),
                    rust_type(&input.type_name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let return_type = function
            .output
            .as_deref()
            .map(rust_type)
            .unwrap_or_else(|| "()".to_string());
        let comma = if params.is_empty() { "" } else { ", " };

        out.push_str(&format!(
            "\n    #[cfg(feature = \"std\")]\n    pub fn {rust_name}(&self{comma}{params}) -> Result<{return_type}> {{\n\
                     let mut cmd = Command::new(\"starforge\");\n\
                     cmd.args([\"contract\", \"invoke\", &self.contract_id, \"{name}\", \"--network\", &self.network]);\n",
            name = function.name,
            return_type = return_type
        ));

        for input in &function.inputs {
            let ident = sanitize_ident(&input.name);
            out.push_str(&format!(
                "        cmd.arg(\"--arg\").arg(self.serialize_arg(&{ident})?).arg(\"--type\").arg(\"{ty}\");\n",
                ty = input.type_name
            ));
        }

        let parse_expr = if return_type == "()" {
            "Ok(())".to_string()
        } else {
            format!("Ok(self.parse_result::<{return_type}>(&result)?)")
        };

        out.push_str(&format!(
            "        if let Some(wallet) = &self.wallet {{\n\
                         cmd.arg(\"--wallet\").arg(wallet).arg(\"--submit\");\n\
                     }}\n\
                     let result = self.execute_command(cmd)?;\n\
                     {parse_expr}\n\
                 }}\n"
        ));
    }

    out.push_str("}\n\n");

    for struct_def in &metadata.structs {
        let struct_name = pascal_case(&struct_def.name);
        out.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
        out.push_str(
            "#[cfg_attr(feature = \"serde\", derive(serde::Serialize, serde::Deserialize))]\n",
        );
        out.push_str(&format!("pub struct {} {{\n", struct_name));
        for field in &struct_def.fields {
            let field_name = sanitize_ident(&field.name);
            let rust_ty = rust_type(&field.type_name);
            out.push_str(&format!("    pub {}: {},\n", field_name, rust_ty));
        }
        out.push_str("}\n\n");
    }

    for enum_def in &metadata.enums {
        let enum_name = pascal_case(&enum_def.name);
        out.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
        out.push_str(
            "#[cfg_attr(feature = \"serde\", derive(serde::Serialize, serde::Deserialize))]\n",
        );
        out.push_str(&format!("pub enum {} {{\n", enum_name));
        for variant in &enum_def.variants {
            let variant_name = pascal_case(&variant.name);
            if let Some(ty) = &variant.type_name {
                out.push_str(&format!("    {}({}),\n", variant_name, rust_type(ty)));
            } else {
                out.push_str(&format!("    {},\n", variant_name));
            }
        }
        out.push_str("}\n\n");
    }

    if !metadata.events.is_empty() {
        out.push_str("// Event type definitions\n");
        for event in &metadata.events {
            let event_name = pascal_case(&event.name);
            out.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
            out.push_str(
                "#[cfg_attr(feature = \"serde\", derive(serde::Serialize, serde::Deserialize))]\n",
            );
            out.push_str(&format!("pub struct {}Event {{\n", event_name));
            for field in &event.fields {
                let field_name = sanitize_ident(&field.name);
                let rust_ty = rust_type(&field.type_name);
                out.push_str(&format!("    pub {}: {},\n", field_name, rust_ty));
            }
            out.push_str("}\n\n");
        }
    }

    out
}

pub fn generate_rust(metadata: &ContractMetadata) -> String {
    let mut out = String::from(
        "use std::process::Command;\nuse std::io::{self, Write};\nuse anyhow::{Result, Context};\n\n\
         pub struct ContractClient {\n\
         \tpub contract_id: String,\n\
         \tpub network: String,\n\
         \tpub wallet: Option<String>,\n\
         }\n\n\
         impl ContractClient {\n\
         \tpub fn new(contract_id: impl Into<String>, network: impl Into<String>) -> Self {\n\
         \t\tSelf { contract_id: contract_id.into(), network: network.into(), wallet: None }\n\
         \t}\n\n\
         \tpub fn with_wallet(mut self, wallet: impl Into<String>) -> Self {\n\
         \t\tself.wallet = Some(wallet.into());\n\
         \t\tself\n\
         \t}\n\n\
         \tfn execute_command(&self, mut cmd: Command) -> Result<String> {\n\
         \t\tlet output = cmd.output().context(\"Failed to execute command\")?;\n\
         \t\tif output.status.success() {\n\
         \t\t\tOk(String::from_utf8_lossy(&output.stdout).trim().to_string())\n\
         \t\t} else {\n\
         \t\t\tlet stderr = String::from_utf8_lossy(&output.stderr);\n\
         \t\t\tanyhow::bail!(\"Command failed: {}\", stderr)\n\
         \t\t}\n\
         \t}\n\n",
    );

    for function in &metadata.functions {
        let rust_name = sanitize_ident(&function.name);
        let params = function
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{}: {}",
                    sanitize_ident(&input.name),
                    rust_type(&input.type_name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let return_type = function
            .output
            .as_deref()
            .map(rust_type)
            .unwrap_or_else(|| "()".to_string());
        let comma = if params.is_empty() { "" } else { ", " };

        out.push_str(&format!(
            "\tpub fn {rust_name}(&self{comma}{params}) -> Result<{return_type}> {{\n\
             \t\tlet mut cmd = Command::new(\"starforge\");\n\
             \t\tcmd.args([\"contract\", \"invoke\", &self.contract_id, \"{name}\", \"--network\", &self.network]);\n",
            name = function.name,
            return_type = return_type
        ));

        for input in &function.inputs {
            let ident = sanitize_ident(&input.name);
            out.push_str(&format!(
                "\t\tcmd.arg(\"--arg\").arg(self.serialize_arg(&{ident})?).arg(\"--type\").arg(\"{ty}\");\n",
                ty = input.type_name
            ));
        }

        out.push_str(
            "\t\tif let Some(wallet) = &self.wallet {\n\
             \t\t\tcmd.arg(\"--wallet\").arg(wallet).arg(\"--submit\");\n\
             \t\t}\n\
             \t\tlet result = self.execute_command(cmd)?;\n\
             \t\t// Parse result based on return type\n\
             \t\tOk(self.parse_result::<{return_type}>(&result)?)\n\
             \t}\n\n",
        );
    }

    // Add serialization/deserialization helper methods
    out.push_str(
        "\tfn serialize_arg<T: std::fmt::Display>(&self, value: &T) -> Result<String> {\n\
         \t\tOk(value.to_string())\n\
         \t}\n\n\
         \tfn parse_result<T>(&self, result: &str) -> Result<T> \n\
         \twhere T: std::str::FromStr,\n\
         \t      T::Err: std::error::Error + Send + Sync + 'static,\n\
         \t{\n\
         \t\tresult.parse().context(\"Failed to parse result\")\n\
         \t}\n\n\
         }\n\n",
    );

    for struct_def in &metadata.structs {
        let struct_name = pascal_case(&struct_def.name);
        out.push_str(&format!("pub struct {} {{\n", struct_name));
        for field in &struct_def.fields {
            let field_name = sanitize_ident(&field.name);
            let rust_ty = rust_type(&field.type_name);
            out.push_str(&format!("\tpub {}: {},\n", field_name, rust_ty));
        }
        out.push_str("}\n\n");
    }

    for enum_def in &metadata.enums {
        let enum_name = pascal_case(&enum_def.name);
        out.push_str(&format!("pub enum {} {{\n", enum_name));
        for variant in &enum_def.variants {
            let variant_name = pascal_case(&variant.name);
            if let Some(ty) = &variant.type_name {
                out.push_str(&format!("\t{}({}),\n", variant_name, rust_type(ty)));
            } else {
                out.push_str(&format!("\t{},\n", variant_name));
            }
        }
        out.push_str("}\n\n");
    }

    // Generate event type definitions
    if !metadata.events.is_empty() {
        out.push_str("// Event type definitions\n");
        for event in &metadata.events {
            let event_name = pascal_case(&event.name);
            out.push_str(&format!("pub struct {}Event {{\n", event_name));
            for field in &event.fields {
                let field_name = sanitize_ident(&field.name);
                let rust_ty = rust_type(&field.type_name);
                out.push_str(&format!("\tpub {}: {},\n", field_name, rust_ty));
            }
            out.push_str("}\n\n");
        }
    }

    out
}

fn generate_python(metadata: &ContractMetadata) -> String {
    let mut out = String::from(
        "from dataclasses import asdict, dataclass, is_dataclass\n\
         from typing import Any, List, Dict, Optional, Union, Tuple\n\
         import asyncio\n\
         import json\n\
         import subprocess\n\n\
         class ContractInvocationError(RuntimeError):\n\
             \"\"\"Raised when the StarForge CLI cannot invoke a contract function.\"\"\"\n\n\
         @dataclass\n\
         class ContractClientOptions:\n\
             contract_id: str\n\
             network: str = \"testnet\"\n\
             wallet: Optional[str] = None\n\n\
         class ContractClient:\n\
             def __init__(self, options: ContractClientOptions):\n\
                 self.options = options\n\n\
             @staticmethod\n\
             def _encode_arg(value: Any) -> str:\n\
                 if isinstance(value, bool):\n\
                     return str(value).lower()\n\
                 if isinstance(value, bytes):\n\
                     return value.hex()\n\
                 if is_dataclass(value):\n\
                     value = asdict(value)\n\
                 if isinstance(value, (dict, list, tuple)):\n\
                     return json.dumps(value, separators=(\",\", \":\"), default=str)\n\
                 return str(value)\n\n\
             def _invoke_args(self, function_name: str, args: List[Tuple[Any, str]]) -> List[str]:\n\
                 cli = [\"starforge\", \"contract\", \"invoke\", self.options.contract_id, function_name, \"--network\", self.options.network]\n\
                 for value, type_name in args:\n\
                     cli.extend([\"--arg\", self._encode_arg(value), \"--type\", type_name])\n\
                 if self.options.wallet:\n\
                     cli.extend([\"--wallet\", self.options.wallet, \"--submit\"])\n\
                 return cli\n\n\
             def _invoke(self, function_name: str, args: List[Tuple[Any, str]]) -> Any:\n\
                 completed = subprocess.run(self._invoke_args(function_name, args), capture_output=True, text=True)\n\
                 if completed.returncode != 0:\n\
                     detail = completed.stderr.strip() or completed.stdout.strip() or \"unknown error\"\n\
                     raise ContractInvocationError(f\"{function_name} failed: {detail}\")\n\
                 result = completed.stdout.strip()\n\
                 try:\n\
                     return json.loads(result)\n\
                 except json.JSONDecodeError:\n\
                     return result\n\n",
    );

    for function in &metadata.functions {
        let py_name = sanitize_ident(&function.name);
        let params = function
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{}: {}",
                    python_ident(&input.name),
                    python_type(&input.type_name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let return_type = function
            .output
            .as_deref()
            .map(python_type)
            .unwrap_or_else(|| "None".to_string());
        let signature = if params.is_empty() {
            "self".to_string()
        } else {
            format!("self, {}", params)
        };
        out.push_str(&format!(
            "    def {}({}) -> {}:\n\
             \"\"\"Invoke the contract function and decode its JSON result.\"\"\"\n\
             args = [\n",
            py_name, signature, return_type
        ));
        for (i, input) in function.inputs.iter().enumerate() {
            if i == function.inputs.len() - 1 {
                out.push_str(&format!(
                    "                ({}, \"{}\")\n",
                    python_ident(&input.name),
                    input.type_name
                ));
            } else {
                out.push_str(&format!(
                    "                ({}, \"{}\"),\n",
                    python_ident(&input.name),
                    input.type_name
                ));
            }
        }
        out.push_str(&format!(
            "            ]\n\
             return self._invoke(\"{}\", args)\n\n",
            function.name
        ));
        let async_args = function
            .inputs
            .iter()
            .map(|input| python_ident(&input.name))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "    async def {}_async({}) -> {}:\n\
             return await asyncio.to_thread(self.{}, {})\n\n",
            py_name, signature, return_type, py_name, async_args
        ));
    }

    out.push('\n');

    for struct_def in &metadata.structs {
        let struct_name = pascal_case(&struct_def.name);
        out.push_str(&format!("@dataclass\nclass {}:\n", struct_name));
        for field in &struct_def.fields {
            let field_name = python_ident(&snake_case(&field.name));
            let py_ty = python_type(&field.type_name);
            out.push_str(&format!("    {}: {}\n", field_name, py_ty));
        }
        out.push('\n');
    }

    // Generate event type definitions
    if !metadata.events.is_empty() {
        out.push_str("# Event type definitions\n");
        for event in &metadata.events {
            let event_name = pascal_case(&event.name);
            out.push_str(&format!("@dataclass\nclass {}Event:\n", event_name));
            for field in &event.fields {
                let field_name = snake_case(&field.name);
                let py_ty = python_type(&field.type_name);
                out.push_str(&format!("    {}: {}\n", field_name, py_ty));
            }
            out.push('\n');
        }
    }

    out
}

fn generate_go(metadata: &ContractMetadata) -> String {
    let mut out = String::from(
        "package client\n\n\
         import \"os/exec\"\n\n\
         type ContractClientOptions struct {\n\
         \tContractID string\n\
         \tNetwork    string\n\
         \tWallet     *string\n\
         }\n\n\
         type ContractClient struct {\n\
         \toptions ContractClientOptions\n\
         }\n\n\
         func NewContractClient(options ContractClientOptions) *ContractClient {\n\
         \tif options.Network == \"\" {\n\
         \t\toptions.Network = \"testnet\"\n\
         \t}\n\
         \treturn &ContractClient{options: options}\n\
         }\n\n\
         func (c *ContractClient) invokeArgs(functionName string, args [][2]string) []string {\n\
         \tcli := []string{\"contract\", \"invoke\", c.options.ContractID, functionName, \"--network\", c.options.Network}\n\
         \tfor _, arg := range args {\n\
         \t\tcli = append(cli, \"--arg\", arg[0], \"--type\", arg[1])\n\
         \t}\n\
         \tif c.options.Wallet != nil {\n\
         \t\tcli = append(cli, \"--wallet\", *c.options.Wallet, \"--submit\")\n\
         \t}\n\
         \treturn cli\n\
         }\n\n",
    );

    for function in &metadata.functions {
        let go_name = pascal_case(&function.name);
        let params = function
            .inputs
            .iter()
            .map(|input| format!("{} {}", pascal_case(&input.name), go_type(&input.type_name)))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "func (c *ContractClient) {}({}) []string {{\n\
             \targs := [][2]string{{\n",
            go_name, params
        ));
        for input in &function.inputs {
            out.push_str(&format!(
                "\t\t{{\"{}\", \"{}\"}},\n",
                pascal_case(&input.name),
                input.type_name
            ));
        }
        out.push_str(&format!(
            "\t}}\n\
             \treturn c.invokeArgs(\"{}\", args)\n\
             }}\n\n",
            function.name
        ));
    }

    for struct_def in &metadata.structs {
        let struct_name = pascal_case(&struct_def.name);
        out.push_str(&format!("type {} struct {{\n", struct_name));
        for field in &struct_def.fields {
            let field_name = pascal_case(&field.name);
            let go_ty = go_type(&field.type_name);
            out.push_str(&format!("\t{} {}\n", field_name, go_ty));
        }
        out.push_str("}\n\n");
    }

    // Generate event type definitions
    if !metadata.events.is_empty() {
        out.push_str("// Event type definitions\n");
        for event in &metadata.events {
            let event_name = pascal_case(&event.name);
            out.push_str(&format!("type {}Event struct {{\n", event_name));
            for field in &event.fields {
                let field_name = pascal_case(&field.name);
                let go_ty = go_type(&field.type_name);
                out.push_str(&format!("\t{} {}\n", field_name, go_ty));
            }
            out.push_str("}\n\n");
        }
    }

    out
}

fn rust_type(type_name: &str) -> String {
    match type_name {
        "bool" => "bool".to_string(),
        "u32" => "u32".to_string(),
        "i32" => "i32".to_string(),
        "u64" => "u64".to_string(),
        "i64" => "i64".to_string(),
        "u128" => "u128".to_string(),
        "i128" => "i128".to_string(),
        "String" => "String".to_string(),
        "Symbol" => "String".to_string(),
        "Address" => "String".to_string(),
        "Bytes" => "Vec<u8>".to_string(),
        "()" => "()".to_string(),
        "Val" => "i64".to_string(),
        "Error" => "String".to_string(),
        "U256" => "String".to_string(),
        "I256" => "String".to_string(),
        _ => {
            // Handle complex types like Option<T>, Result<T, E>, Vec<T>, etc.
            if type_name.starts_with("Option<")
                || type_name.starts_with("Result<")
                || type_name.starts_with("Vec<")
                || type_name.starts_with("Map<")
                || type_name.starts_with("BytesN<")
            {
                type_name.to_string()
            } else {
                // Assume it's a custom type
                type_name.to_string()
            }
        }
    }
}

fn python_type(type_name: &str) -> String {
    match type_name {
        "bool" => "bool".to_string(),
        "u32" | "i32" | "u64" | "i64" | "u128" | "i128" => "int".to_string(),
        "String" | "Symbol" | "Address" => "str".to_string(),
        "Bytes" => "bytes".to_string(),
        "()" => "None".to_string(),
        "Val" => "int".to_string(),
        "Error" => "str".to_string(),
        "U256" | "I256" => "str".to_string(),
        _ => {
            // Handle complex types
            if type_name.starts_with("Option<") {
                let inner = &type_name[7..type_name.len() - 1]; // Remove "Option<>"
                format!("Optional[{}]", python_type(inner))
            } else if type_name.starts_with("Result<") {
                "Any".to_string()
            } else if type_name.starts_with("Vec<") {
                let inner = &type_name[4..type_name.len() - 1]; // Remove "Vec<>"
                format!("List[{}]", python_type(inner))
            } else if type_name.starts_with("Map<") {
                "Dict[str, Any]".to_string()
            } else if type_name.starts_with("BytesN<") {
                "bytes".to_string()
            } else if type_name.starts_with("(") && type_name.ends_with(")") {
                // Tuple type
                "Tuple".to_string()
            } else {
                // Custom type
                type_name.to_string()
            }
        }
    }
}

fn python_ident(input: &str) -> String {
    let ident = sanitize_ident(input);
    match ident.as_str() {
        "and" | "as" | "assert" | "async" | "await" | "break" | "case" | "class" | "continue"
        | "def" | "del" | "elif" | "else" | "except" | "False" | "finally" | "for" | "from"
        | "global" | "if" | "import" | "in" | "is" | "lambda" | "match" | "None" | "nonlocal"
        | "not" | "or" | "pass" | "raise" | "return" | "True" | "try" | "type" | "while"
        | "with" | "yield" => format!("{}", ident) + "_",
        _ => ident,
    }
}

fn go_type(type_name: &str) -> String {
    match type_name {
        "bool" => "bool".to_string(),
        "u32" => "uint32".to_string(),
        "i32" => "int32".to_string(),
        "u64" => "uint64".to_string(),
        "i64" => "int64".to_string(),
        "u128" => "string".to_string(),
        "i128" => "string".to_string(),
        "String" | "Symbol" | "Address" => "string".to_string(),
        "Bytes" => "[]byte".to_string(),
        "()" => "".to_string(),
        "Val" => "int64".to_string(),
        "Error" => "string".to_string(),
        "U256" | "I256" => "string".to_string(),
        _ => {
            // Handle complex types
            if type_name.starts_with("Option<") {
                // In Go, we can use pointer types for optional
                let inner = &type_name[7..type_name.len() - 1]; // Remove "Option<>"
                format!("*{}", go_type(inner))
            } else if type_name.starts_with("Result<") {
                "interface{}".to_string()
            } else if type_name.starts_with("Vec<") {
                let inner = &type_name[4..type_name.len() - 1]; // Remove "Vec<>"
                format!("[]{}", go_type(inner))
            } else if type_name.starts_with("Map<") {
                "map[string]interface{}".to_string()
            } else if type_name.starts_with("BytesN<") {
                "[]byte".to_string()
            } else if type_name.starts_with("(") && type_name.ends_with(")") {
                // Tuple type
                "[]interface{}".to_string()
            } else {
                // Custom type
                type_name.to_string()
            }
        }
    }
}

/// A complex fixture contract covering functions with multiple parameter
/// types, structs, enums, events, Option, Result, Vec, and Map.
pub fn complex_metadata() -> ContractMetadata {
    ContractMetadata {
        functions: vec![
            ContractFunction {
                name: "transfer".to_string(),
                inputs: vec![
                    ContractInput {
                        name: "from".to_string(),
                        type_name: "Address".to_string(),
                    },
                    ContractInput {
                        name: "to".to_string(),
                        type_name: "Address".to_string(),
                    },
                    ContractInput {
                        name: "amount".to_string(),
                        type_name: "u128".to_string(),
                    },
                    ContractInput {
                        name: "memo".to_string(),
                        type_name: "Option<String>".to_string(),
                    },
                ],
                output: Some("Result<(), Error>".to_string()),
                doc: "Transfer `amount` from `from` to `to`.\nFails with `TransferError` codes."
                    .to_string(),
            },
            ContractFunction {
                name: "balance_of".to_string(),
                inputs: vec![ContractInput {
                    name: "owner".to_string(),
                    type_name: "Address".to_string(),
                }],
                output: Some("u128".to_string()),
                doc: "Returns the balance held by `owner`.".to_string(),
            },
            ContractFunction {
                name: "get_metadata".to_string(),
                inputs: vec![],
                output: Some("TokenMetadata".to_string()),
                doc: "Returns the token metadata.".to_string(),
            },
            ContractFunction {
                name: "batch_transfer".to_string(),
                inputs: vec![
                    ContractInput {
                        name: "recipients".to_string(),
                        type_name: "Vec<Address>".to_string(),
                    },
                    ContractInput {
                        name: "amounts".to_string(),
                        type_name: "Vec<u128>".to_string(),
                    },
                ],
                output: Some("Vec<Result<(), Error>>".to_string()),
                doc: String::new(),
            },
            ContractFunction {
                name: "set_config".to_string(),
                inputs: vec![
                    ContractInput {
                        name: "key".to_string(),
                        type_name: "Symbol".to_string(),
                    },
                    ContractInput {
                        name: "value".to_string(),
                        type_name: "Bytes".to_string(),
                    },
                ],
                output: None,
                doc: String::new(),
            },
        ],
        structs: vec![
            ContractStruct {
                name: "TokenMetadata".to_string(),
                fields: vec![
                    ContractField {
                        name: "name".to_string(),
                        type_name: "String".to_string(),
                    },
                    ContractField {
                        name: "symbol".to_string(),
                        type_name: "String".to_string(),
                    },
                    ContractField {
                        name: "decimals".to_string(),
                        type_name: "u32".to_string(),
                    },
                    ContractField {
                        name: "total_supply".to_string(),
                        type_name: "u128".to_string(),
                    },
                    ContractField {
                        name: "admin".to_string(),
                        type_name: "Address".to_string(),
                    },
                ],
            },
            ContractStruct {
                name: "Allowance".to_string(),
                fields: vec![
                    ContractField {
                        name: "owner".to_string(),
                        type_name: "Address".to_string(),
                    },
                    ContractField {
                        name: "spender".to_string(),
                        type_name: "Address".to_string(),
                    },
                    ContractField {
                        name: "amount".to_string(),
                        type_name: "u128".to_string(),
                    },
                    ContractField {
                        name: "expires_at".to_string(),
                        type_name: "Option<u64>".to_string(),
                    },
                ],
            },
        ],
        enums: vec![ContractEnum {
            name: "TokenError".to_string(),
            variants: vec![
                ContractVariant {
                    name: "InsufficientBalance".to_string(),
                    type_name: None,
                    value: None,
                },
                ContractVariant {
                    name: "Unauthorized".to_string(),
                    type_name: Some("Address".to_string()),
                    value: None,
                },
                ContractVariant {
                    name: "InvalidAmount".to_string(),
                    type_name: Some("u128".to_string()),
                    value: None,
                },
            ],
        }],
        events: vec![ContractEvent {
            name: "Transfer".to_string(),
            fields: vec![
                ContractField {
                    name: "from".to_string(),
                    type_name: "Address".to_string(),
                },
                ContractField {
                    name: "to".to_string(),
                    type_name: "Address".to_string(),
                },
                ContractField {
                    name: "amount".to_string(),
                    type_name: "u128".to_string(),
                },
            ],
        }],
        unions: vec![ContractEnum {
            name: "DataKey".to_string(),
            variants: vec![
                ContractVariant {
                    name: "Admin".to_string(),
                    type_name: None,
                    value: None,
                },
                ContractVariant {
                    name: "Balance".to_string(),
                    type_name: Some("Address".to_string()),
                    value: None,
                },
                ContractVariant {
                    name: "Allowance".to_string(),
                    type_name: Some("(Address, Address)".to_string()),
                    value: None,
                },
            ],
        }],
        errors: vec![ContractErrorEnum {
            name: "TransferError".to_string(),
            cases: vec![
                ContractErrorCase {
                    name: "InsufficientFunds".to_string(),
                    value: 1,
                    doc: "Sender balance is lower than the requested amount".to_string(),
                },
                ContractErrorCase {
                    name: "Frozen".to_string(),
                    value: 2,
                    doc: String::new(),
                },
                ContractErrorCase {
                    name: "LimitExceeded".to_string(),
                    value: 10,
                    doc: "Transfer exceeds the configured daily limit".to_string(),
                },
            ],
        }],
    }
}

#[cfg(test)]
pub fn sanitize_ident(input: &str) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            if index == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

#[cfg(not(test))]
fn sanitize_ident(input: &str) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            if index == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

fn snake_case(input: &str) -> String {
    let mut out = String::new();
    for (i, ch) in input.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    sanitize_ident(&out)
}

fn camel_case(input: &str) -> String {
    let mut out = String::new();
    let mut next_upper = false;
    for (i, ch) in input.chars().enumerate() {
        if ch == '_' {
            next_upper = true;
        } else if next_upper || (i == 0 && ch.is_ascii_lowercase()) {
            out.push(ch.to_ascii_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    sanitize_ident(&out)
}

fn pascal_case(input: &str) -> String {
    let mut out = String::new();
    let mut next_upper = true;
    for ch in input.chars() {
        if ch == '_' {
            next_upper = true;
        } else if next_upper {
            out.push(ch.to_ascii_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    sanitize_ident(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_multibyte_leb128() {
        let bytes = [0xe5, 0x8e, 0x26];
        let mut offset = 0;
        assert_eq!(read_var_u32(&bytes, &mut offset).unwrap(), 624485);
        assert_eq!(offset, 3);
    }

    #[test]
    fn rejects_non_wasm() {
        let err = contract_spec_section(b"not wasm").unwrap_err();
        assert!(err.to_string().contains("valid WASM"));
    }

    #[test]
    fn sanitizes_generated_identifiers() {
        assert_eq!(sanitize_ident("transfer-from"), "transfer_from");
        assert_eq!(sanitize_ident("1st"), "_1st");
    }

    #[test]
    fn generates_typed_python_sync_and_async_clients() {
        let generated = generate_python(&complex_metadata());

        assert!(generated.contains("from typing import Any"));
        assert!(generated.contains("def transfer(self, from_: str"));
        assert!(generated.contains("def get_metadata(self) -> TokenMetadata:"));
        assert!(generated.contains("async def balance_of_async(self, owner: str) -> int:"));
        assert!(generated.contains("self._encode_arg(value)"));
        assert!(generated.contains("raise ContractInvocationError"));
        assert!(generated.contains("@dataclass\nclass TokenMetadata:"));
    }
}
