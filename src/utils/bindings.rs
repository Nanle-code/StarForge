use anyhow::{Context, Result};
use std::io::Cursor;
use std::path::Path;
use stellar_xdr::curr::{
    Limited, Limits, ReadXdr, ScSpecEntry, ScSpecFunctionV0, ScSpecTypeDef, ScSpecUdtEnumV0,
    ScSpecUdtStructV0, ScSpecUdtUnionV0,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractFunction {
    pub name: String,
    pub inputs: Vec<ContractInput>,
    pub output: Option<String>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractEvent {
    pub name: String,
    pub fields: Vec<ContractField>,
}

pub fn generate_bindings(wasm_path: &Path, language: BindingLanguage) -> Result<String> {
    let wasm = std::fs::read(wasm_path)
        .with_context(|| format!("Failed to read WASM file {}", wasm_path.display()))?;
    let entries = read_spec_entries(&wasm)?;
    let metadata = parse_spec_entries(&entries);

    if metadata.functions.is_empty() {
        anyhow::bail!("No contract functions found in WASM metadata");
    }

    generate_from_metadata(&metadata, language)
}

fn read_spec_entries(wasm: &[u8]) -> Result<Vec<ScSpecEntry>> {
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

fn parse_spec_entries(entries: &[ScSpecEntry]) -> ContractMetadata {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut events = Vec::new();

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
                // Extract error enums as events
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
            ScSpecEntry::UdtUnionV0(_) => {}
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    ContractMetadata {
        functions,
        structs,
        enums,
        events,
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

fn contract_spec_section(wasm: &[u8]) -> Result<&[u8]> {
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

fn read_var_u32(bytes: &[u8], offset: &mut usize) -> Result<u32> {
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
            anyhow::bail!("Invalid u32 LEB128 value in WASM");
        }
    }
}

fn generate_rust(metadata: &ContractMetadata) -> String {
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

fn generate_typescript(metadata: &ContractMetadata) -> String {
    let mut out = String::from(
        "export type ContractClientOptions = {\n\
         \tcontractId: string;\n\
         \tnetwork?: string;\n\
         \twallet?: string;\n\
         };\n\n\
         export class ContractClient {\n\
         \tconstructor(private readonly options: ContractClientOptions) {}\n\n\
         \tprivate invokeArgs(functionName: string, args: Array<[unknown, string]>): string[] {\n\
         \t\tconst cli = [\"contract\", \"invoke\", this.options.contractId, functionName, \"--network\", this.options.network ?? \"testnet\"];\n\
         \t\tfor (const [value, typeName] of args) cli.push(\"--arg\", String(value), \"--type\", typeName);\n\
         \t\tif (this.options.wallet) cli.push(\"--wallet\", this.options.wallet, \"--submit\");\n\
         \t\treturn cli;\n\
         \t}\n\n",
    );

    for function in &metadata.functions {
        let ts_name = sanitize_ident(&function.name);
        let params = function
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{}: {}",
                    sanitize_ident(&input.name),
                    ts_type(&input.type_name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let return_type = function
            .output
            .as_deref()
            .map(ts_type)
            .unwrap_or_else(|| "void".to_string());
        out.push_str(&format!(
            "\t{name}({params}): string[] /* returns CLI args; expected result: {return_type} */ {{\n\
             \t\treturn this.invokeArgs(\"{source}\", [",
            name = ts_name,
            source = function.name
        ));
        out.push_str(
            &function
                .inputs
                .iter()
                .map(|input| format!("[{}, \"{}\"]", sanitize_ident(&input.name), input.type_name))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("]);\n\t}\n\n");
    }

    out.push_str("}\n\n");

    for struct_def in &metadata.structs {
        let struct_name = pascal_case(&struct_def.name);
        out.push_str(&format!("export interface {} {{\n", struct_name));
        for field in &struct_def.fields {
            let field_name = camel_case(&field.name);
            let ts_ty = ts_type(&field.type_name);
            out.push_str(&format!("\t{}: {};\n", field_name, ts_ty));
        }
        out.push_str("}\n\n");
    }

    for enum_def in &metadata.enums {
        let enum_name = pascal_case(&enum_def.name);
        out.push_str(&format!("export type {} = \n", enum_name));
        for (i, variant) in enum_def.variants.iter().enumerate() {
            let variant_name = camel_case(&variant.name);
            let variant_type = if let Some(ty) = &variant.type_name {
                format!("{{ type: \"{}\"; value: {} }}", variant_name, ts_type(ty))
            } else {
                format!("{{ type: \"{}\" }}", variant_name)
            };
            if i == enum_def.variants.len() - 1 {
                out.push_str(&format!("\t{};\n", variant_type));
            } else {
                out.push_str(&format!("\t{} |\n", variant_type));
            }
        }
        out.push('\n');
    }

    // Generate event type definitions
    if !metadata.events.is_empty() {
        out.push_str("// Event type definitions\n");
        for event in &metadata.events {
            let event_name = pascal_case(&event.name);
            out.push_str(&format!("export interface {}Event {{\n", event_name));
            for field in &event.fields {
                let field_name = camel_case(&field.name);
                let ts_ty = ts_type(&field.type_name);
                out.push_str(&format!("\t{}: {};\n", field_name, ts_ty));
            }
            out.push_str("}\n\n");
        }
    }

    out
}

fn generate_python(metadata: &ContractMetadata) -> String {
    let mut out = String::from(
        "from dataclasses import dataclass\n\
         from typing import List, Dict, Optional, Union, Tuple\n\
         import subprocess\n\n\
         @dataclass\n\
         class ContractClientOptions:\n\
             contract_id: str\n\
             network: str = \"testnet\"\n\
             wallet: Optional[str] = None\n\n\
         class ContractClient:\n\
             def __init__(self, options: ContractClientOptions):\n\
                 self.options = options\n\n\
             def _invoke_args(self, function_name: str, args: List[Tuple[str, str]]) -> List[str]:\n\
                 cli = [\"starforge\", \"contract\", \"invoke\", self.options.contract_id, function_name, \"--network\", self.options.network]\n\
                 for value, type_name in args:\n\
                     cli.extend([\"--arg\", str(value), \"--type\", type_name])\n\
                 if self.options.wallet:\n\
                     cli.extend([\"--wallet\", self.options.wallet, \"--submit\"])\n\
                 return cli\n\n",
    );

    for function in &metadata.functions {
        let py_name = sanitize_ident(&function.name);
        let params = function
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{}: {}",
                    sanitize_ident(&input.name),
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
        out.push_str(&format!(
            "    def {}(self, {}) -> List[str]:\n\
             \"\"\"Returns CLI args; expected result type: {}\"\"\"\n\
             args = [\n",
            py_name, params, return_type
        ));
        for (i, input) in function.inputs.iter().enumerate() {
            if i == function.inputs.len() - 1 {
                out.push_str(&format!(
                    "                ({}, \"{}\")\n",
                    sanitize_ident(&input.name),
                    input.type_name
                ));
            } else {
                out.push_str(&format!(
                    "                ({}, \"{}\"),\n",
                    sanitize_ident(&input.name),
                    input.type_name
                ));
            }
        }
        out.push_str(&format!(
            "            ]\n\
             return self._invoke_args(\"{}\", args)\n\n",
            function.name
        ));
    }

    out.push('\n');

    for struct_def in &metadata.structs {
        let struct_name = pascal_case(&struct_def.name);
        out.push_str(&format!("@dataclass\nclass {}:\n", struct_name));
        for field in &struct_def.fields {
            let field_name = snake_case(&field.name);
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

fn ts_type(type_name: &str) -> String {
    match type_name {
        "bool" => "boolean".to_string(),
        "u32" | "i32" | "u64" | "i64" | "u128" | "i128" => "number | bigint".to_string(),
        "String" | "Symbol" | "Address" => "string".to_string(),
        "Bytes" => "Uint8Array".to_string(),
        "()" => "void".to_string(),
        "Val" => "number".to_string(),
        "Error" => "string".to_string(),
        "U256" | "I256" => "string".to_string(),
        _ => {
            // Handle complex types
            if type_name.starts_with("Option<") {
                let inner = &type_name[7..type_name.len() - 1]; // Remove "Option<>"
                format!("{} | null", ts_type(inner))
            } else if type_name.starts_with("Result<") {
                "any".to_string()
            } else if type_name.starts_with("Vec<") {
                let inner = &type_name[4..type_name.len() - 1]; // Remove "Vec<>"
                format!("Array<{}>", ts_type(inner))
            } else if type_name.starts_with("Map<") {
                "Record<string, any>".to_string()
            } else if type_name.starts_with("BytesN<") {
                "Uint8Array".to_string()
            } else if type_name.starts_with("(") && type_name.ends_with(")") {
                // Tuple type
                "any[]".to_string()
            } else {
                // Custom type
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
            },
            ContractFunction {
                name: "balance_of".to_string(),
                inputs: vec![ContractInput {
                    name: "owner".to_string(),
                    type_name: "Address".to_string(),
                }],
                output: Some("u128".to_string()),
            },
            ContractFunction {
                name: "get_metadata".to_string(),
                inputs: vec![],
                output: Some("TokenMetadata".to_string()),
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
                },
                ContractVariant {
                    name: "Unauthorized".to_string(),
                    type_name: Some("Address".to_string()),
                },
                ContractVariant {
                    name: "InvalidAmount".to_string(),
                    type_name: Some("u128".to_string()),
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
}
