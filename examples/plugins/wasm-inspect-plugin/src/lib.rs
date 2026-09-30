//! StarForge plugin that summarises a compiled Soroban contract, used by the
//! plugin authoring cookbook (`docs/plugins/cookbook.md`).
//!
//! ```text
//! starforge wasm-inspect target/wasm32-unknown-unknown/release/my_contract.wasm
//! ```
//!
//! The plugin only reads the file it is given, so its manifest declares the
//! `fs:read` capability and nothing else. Parsing is done on a byte slice
//! ([`inspect`]) so it can be unit tested without touching the filesystem.

use starforge::plugins::{Plugin, PluginRegistrar};
use std::fmt;

/// `\0asm`, the first four bytes of every WebAssembly module.
pub const WASM_MAGIC: [u8; 4] = *b"\0asm";

/// Custom section that Soroban embeds with the contract's interface spec.
pub const SOROBAN_SPEC_SECTION: &str = "contractspecv0";

const USAGE: &str = "usage: starforge wasm-inspect <FILE.wasm>";

/// One section of a WebAssembly module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Section id from the WebAssembly spec (0 = custom).
    pub id: u8,
    /// Standard section name, or the embedded name of a custom section.
    pub name: String,
    /// Payload size in bytes.
    pub size: u32,
}

/// Result of inspecting a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmSummary {
    pub version: u32,
    pub size: usize,
    pub sections: Vec<Section>,
}

impl WasmSummary {
    pub fn has_custom_section(&self, name: &str) -> bool {
        self.sections.iter().any(|s| s.id == 0 && s.name == name)
    }

    /// Soroban contracts carry their interface in a `contractspecv0` section.
    pub fn is_soroban_contract(&self) -> bool {
        self.has_custom_section(SOROBAN_SPEC_SECTION)
    }
}

/// Why a byte slice could not be inspected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectError {
    /// The input does not start with the WebAssembly magic number.
    NotWasm,
    /// The input ends in the middle of a header or section.
    Truncated { offset: usize },
    /// A section id outside the WebAssembly 2.0 range.
    UnknownSection { id: u8, offset: usize },
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWasm => write!(f, "not a WebAssembly module (missing \\0asm header)"),
            Self::Truncated { offset } => write!(f, "module is truncated at byte {offset}"),
            Self::UnknownSection { id, offset } => {
                write!(f, "unknown section id {id} at byte {offset}")
            }
        }
    }
}

impl std::error::Error for InspectError {}

/// Standard name of a non-custom section id.
pub fn section_kind(id: u8) -> Option<&'static str> {
    Some(match id {
        0 => "custom",
        1 => "type",
        2 => "import",
        3 => "function",
        4 => "table",
        5 => "memory",
        6 => "global",
        7 => "export",
        8 => "start",
        9 => "element",
        10 => "code",
        11 => "data",
        12 => "datacount",
        _ => return None,
    })
}

fn read_u32_leb(bytes: &[u8], pos: &mut usize) -> Result<u32, InspectError> {
    let mut result: u32 = 0;
    for shift in (0..35).step_by(7) {
        let byte = *bytes
            .get(*pos)
            .ok_or(InspectError::Truncated { offset: *pos })?;
        *pos += 1;
        result |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(InspectError::Truncated { offset: *pos })
}

/// Parses the module header and section table of `bytes`.
pub fn inspect(bytes: &[u8]) -> Result<WasmSummary, InspectError> {
    if bytes.len() < 4 || bytes[..4] != WASM_MAGIC {
        return Err(InspectError::NotWasm);
    }
    let version_bytes: [u8; 4] = bytes
        .get(4..8)
        .and_then(|b| b.try_into().ok())
        .ok_or(InspectError::Truncated { offset: 4 })?;

    let mut sections = Vec::new();
    let mut pos = 8;
    while pos < bytes.len() {
        let offset = pos;
        let id = bytes[pos];
        pos += 1;
        let kind = section_kind(id).ok_or(InspectError::UnknownSection { id, offset })?;
        let size = read_u32_leb(bytes, &mut pos)?;
        let end = pos
            .checked_add(size as usize)
            .filter(|end| *end <= bytes.len())
            .ok_or(InspectError::Truncated { offset: pos })?;

        let name = if id == 0 {
            let mut name_pos = pos;
            let len = read_u32_leb(&bytes[..end], &mut name_pos)? as usize;
            let raw = bytes
                .get(name_pos..name_pos + len)
                .filter(|_| name_pos + len <= end)
                .ok_or(InspectError::Truncated { offset: name_pos })?;
            String::from_utf8_lossy(raw).into_owned()
        } else {
            kind.to_string()
        };

        sections.push(Section { id, name, size });
        pos = end;
    }

    Ok(WasmSummary {
        version: u32::from_le_bytes(version_bytes),
        size: bytes.len(),
        sections,
    })
}

/// Human-readable report printed by `starforge wasm-inspect`.
pub fn render(path: &str, summary: &WasmSummary) -> String {
    let mut out = format!(
        "{path}\n  size:     {} bytes\n  version:  {}\n  soroban:  {}\n  sections:\n",
        summary.size,
        summary.version,
        if summary.is_soroban_contract() {
            "yes"
        } else {
            "no"
        },
    );
    for section in &summary.sections {
        let label = if section.id == 0 {
            format!("custom \"{}\"", section.name)
        } else {
            section.name.clone()
        };
        out.push_str(&format!("    {label:<28} {:>8} bytes\n", section.size));
    }
    out
}

#[derive(Debug, Default)]
pub struct WasmInspectPlugin;

impl Plugin for WasmInspectPlugin {
    fn name(&self) -> &'static str {
        "wasm-inspect"
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &'static str {
        "Summarises the sections of a compiled Soroban contract WASM"
    }

    fn execute(&self, args: &[String]) -> Result<(), String> {
        let path = match args.first().map(String::as_str) {
            Some("-h") | Some("--help") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some(path) => path,
            None => return Err(USAGE.to_string()),
        };
        // Reading the input file is covered by the `fs:read` capability in
        // starforge-plugin.toml. The plugin never writes or opens sockets.
        let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let summary = inspect(&bytes).map_err(|e| format!("{path}: {e}"))?;
        print!("{}", render(path, &summary));
        Ok(())
    }
}

pub fn register(registrar: &mut dyn PluginRegistrar) {
    registrar.register_plugin(Box::new(WasmInspectPlugin));
}

starforge::export_plugin!(register);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_multi_byte_leb128() {
        let mut pos = 0;
        assert_eq!(read_u32_leb(&[0xe5, 0x8e, 0x26], &mut pos), Ok(624_485));
        assert_eq!(pos, 3);
    }

    #[test]
    fn rejects_unterminated_leb128() {
        let mut pos = 0;
        assert!(read_u32_leb(&[0x80, 0x80], &mut pos).is_err());
    }

    #[test]
    fn empty_module_has_no_sections() {
        let summary = inspect(b"\0asm\x01\0\0\0").unwrap();
        assert_eq!(summary.version, 1);
        assert!(summary.sections.is_empty());
        assert!(!summary.is_soroban_contract());
    }

    #[test]
    fn section_names_cover_the_standard_ids() {
        assert_eq!(section_kind(10), Some("code"));
        assert_eq!(section_kind(13), None);
    }
}
