use crate::plugins::ai::{AICapability, AIPlugin, AIPluginDeclaration, AIPluginRegistrar};
use crate::plugins::interface::{
    is_core_version_compatible, Plugin, PluginDeclaration, PluginRegistrar, CORE_VERSION,
    RUSTC_VERSION,
};
use crate::plugins::manifest;
use crate::plugins::registry::{load_registry, TrustLevel};
use anyhow::Result;
use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::rc::Rc;

/// Structured diagnostic for a plugin loading failure.
///
/// Each variant maps to a distinct root cause so callers can surface
/// actionable guidance rather than a raw error string.
#[derive(Debug)]
pub enum PluginLoadError {
    /// The file could not be opened as a shared library (wrong format, missing
    /// file, OS-level load error, etc.).
    InvalidLibrary { path: String, detail: String },
    /// The `PLUGIN_DECLARATION` symbol was absent — the binary is not a
    /// StarForge plugin or was stripped.
    MissingRequiredSymbol { path: String, symbol: String },
    /// The plugin was compiled with a different `rustc` version, making the
    /// Rust ABI incompatible.
    AbiBuildMismatch {
        path: String,
        plugin_rustc: String,
        required_rustc: String,
    },
    /// The plugin targets a different StarForge major version.
    UnsupportedCoreVersion {
        path: String,
        plugin_core: String,
        running_core: String,
    },
    /// The `starforge-plugin.toml` manifest failed validation.
    ManifestIncompatible { path: String, detail: String },
    /// The plugin panicked or crashed unexpectedly during active registration routines.
    RegistrationRuntimePanic { path: String, detail: String },
    /// The plugin requested capabilities not permitted by its trust level.
    PermissionDenied { path: String, capabilities: String },
}

impl PluginLoadError {
    /// A short label identifying the failure category.
    pub fn category(&self) -> &'static str {
        match self {
            Self::InvalidLibrary { .. } => "invalid_library",
            Self::MissingRequiredSymbol { .. } => "missing_symbol",
            Self::AbiBuildMismatch { .. } => "abi_mismatch",
            Self::UnsupportedCoreVersion { .. } => "unsupported_core_version",
            Self::ManifestIncompatible { .. } => "manifest_incompatible",
            Self::RegistrationRuntimePanic { .. } => "runtime_panic",
            Self::PermissionDenied { .. } => "permission_denied",
        }
    }

    /// Human-readable explanation with a suggested fix.
    pub fn diagnostic(&self) -> String {
        match self {
            Self::InvalidLibrary { path, detail } => format!(
                "Cannot load shared library '{path}'.\n  \
                 Cause: {detail}\n  \
                 Fix: Verify the file is a valid .so/.dylib/.dll built for this platform.",
            ),
            Self::MissingRequiredSymbol { path, symbol } => format!(
                "Required symbol '{symbol}' not found in '{path}'.\n  \
                 Cause: The binary is not a StarForge plugin or was built without `export_plugin!`.\n  \
                 Fix: Ensure the plugin crate calls `starforge_plugin_sdk::export_plugin!(register_fn)` \
                 and is compiled as a `cdylib`.",
            ),
            Self::AbiBuildMismatch { path, plugin_rustc, required_rustc } => format!(
                "ABI mismatch in '{path}'.\n  \
                 Plugin rustc : {plugin_rustc}\n  \
                 Required rustc: {required_rustc}\n  \
                 Fix: Rebuild the plugin with the same Rust toolchain used to build StarForge \
                 (`rustup override set <toolchain>`).",
            ),
            Self::UnsupportedCoreVersion { path, plugin_core, running_core } => format!(
                "Unsupported StarForge core version in '{path}'.\n  \
                 Plugin targets : StarForge {plugin_core}\n  \
                 Running        : StarForge {running_core}\n  \
                 Fix: Rebuild the plugin for StarForge {running_core}, or add a \
                 'starforge-plugin.toml' with `starforge_version = \"{running_core}\"` \
                 and rebuild.",
            ),
            Self::ManifestIncompatible { path, detail } => format!(
                "Plugin manifest incompatible for '{path}'.\n  \
                 Detail: {detail}\n  \
                 Fix: Update 'starforge-plugin.toml' to match the running StarForge version.",
            ),
            Self::RegistrationRuntimePanic { path, detail } => format!(
                "Plugin crashed during registration for '{path}'.\n  \
                 Detail: {detail}\n  \
                 Fix: Review third-party plugin internal setup safety rules or contact the maintainer.",
            ),
            Self::PermissionDenied { path, capabilities } => format!(
                "Plugin at '{path}' requested denied capabilities: {capabilities}.\n  \
                 Fix: Install from a trusted source or adjust its manifest requirements.",
            ),
        }
    }
}

impl std::fmt::Display for PluginLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.diagnostic())
    }
}

impl std::error::Error for PluginLoadError {}

pub struct PluginManager {
    /// Maps plugin name → (plugin, core_version it was built against).
    plugins: HashMap<String, (Box<dyn Plugin>, String)>,
    /// Maps AI plugin name → (plugin, core_version it was built against).
    pub ai_plugins: HashMap<String, (Box<dyn AIPlugin>, String)>,
    libraries: Vec<Rc<Library>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            ai_plugins: HashMap::new(),
            libraries: Vec::new(),
        }
    }

    /// # Safety
    /// The caller must ensure the plugin at `path` is a valid StarForge plugin
    /// compiled with a compatible Rust toolchain and ABI.
    pub unsafe fn load_plugin<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.load_plugin_diagnosed(path)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Like [`load_plugin`] but returns a structured [`PluginLoadError`] on
    /// failure so callers can display category-specific diagnostics.
    ///
    /// # Safety
    /// Same contract as [`load_plugin`].
    pub unsafe fn load_plugin_diagnosed<P: AsRef<Path>>(
        &mut self,
        path: P,
    ) -> std::result::Result<(), PluginLoadError> {
        let path_ref = path.as_ref();
        let path_display = path_ref.to_string_lossy().to_string();

        // ── Pre-load manifest compatibility validation ───────────────────────
        // Inspect and validate manifest *before* opening binary with Library::new()
        // to prevent OS-level linker panics or ABI crashes on incompatible libraries.
        if let Ok(Some(mf)) = manifest::load_manifest_for_library(Path::new(path_ref)) {
            mf.validate()
                .map_err(|e| PluginLoadError::ManifestIncompatible {
                    path: path_display.clone(),
                    detail: e.to_string(),
                })?;
        }

        // ── Open the shared library ──────────────────────────────────────────
        let library =
            Library::new(path_ref.as_os_str()).map_err(|e| PluginLoadError::InvalidLibrary {
                path: path_display.clone(),
                detail: e.to_string(),
            })?;
        let library = Rc::new(library);

        // ── Locate the required export symbol ────────────────────────────────
        let mut standard_decl = None;
        let mut ai_decl = None;

        if let Ok(d) = library.get::<*mut PluginDeclaration>(b"PLUGIN_DECLARATION") {
            standard_decl = Some(*d);
        } else if let Ok(d) = library.get::<*mut AIPluginDeclaration>(b"AI_PLUGIN_DECLARATION") {
            ai_decl = Some(*d);
        } else {
            return Err(PluginLoadError::MissingRequiredSymbol {
                path: path_display.clone(),
                symbol: "PLUGIN_DECLARATION or AI_PLUGIN_DECLARATION".to_string(),
            });
        }

        let (rustc_version, core_version) = if let Some(d) = standard_decl {
            let d = unsafe { &*d };
            (d.rustc_version, d.core_version)
        } else if let Some(d) = ai_decl {
            let d = unsafe { &*d };
            (d.rustc_version, d.core_version)
        } else {
            unreachable!()
        };

        // ── rustc ABI check ──────────────────────────────────────────────────
        if rustc_version != RUSTC_VERSION {
            return Err(PluginLoadError::AbiBuildMismatch {
                path: path_display,
                plugin_rustc: rustc_version.to_string(),
                required_rustc: RUSTC_VERSION.to_string(),
            });
        }

        // ── StarForge core version check ─────────────────────────────────────
        if !is_core_version_compatible(core_version) {
            return Err(PluginLoadError::UnsupportedCoreVersion {
                path: path_display,
                plugin_core: core_version.to_string(),
                running_core: CORE_VERSION.to_string(),
            });
        }

        let registry = load_registry().unwrap_or_default();
        let plugin_trust = registry
            .plugins
            .iter()
            .find(|p| p.path == path_display)
            .map(|p| p.trust.clone())
            .unwrap_or(TrustLevel::Unknown);

        if let Some(decl) = standard_decl {
            let decl = unsafe { &*decl };
            let mut registrar = ProxyRegistrar::new();

            // Protect the system execution loop from third-party registration panics
            let register_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (decl.register)(&mut registrar);
            }));

            if let Err(panic_payload) = register_result {
                let detail = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown closure panic origin".to_string()
                };
                return Err(PluginLoadError::RegistrationRuntimePanic {
                    path: path_display,
                    detail,
                });
            }

            let plugin_core_version = decl.core_version.to_string();
            for plugin in registrar.plugins {
                let name = plugin.name().to_string();
                plugin.on_load();
                self.plugins
                    .insert(name, (plugin, plugin_core_version.clone()));
            }
        } else if let Some(decl) = ai_decl {
            let decl = unsafe { &*decl };
            let mut registrar = AIProxyRegistrar::new();

            let register_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (decl.register)(&mut registrar);
            }));

            if let Err(panic_payload) = register_result {
                let detail = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown closure panic origin".to_string()
                };
                return Err(PluginLoadError::RegistrationRuntimePanic {
                    path: path_display,
                    detail,
                });
            }

            let plugin_core_version = decl.core_version.to_string();
            for plugin in registrar.plugins {
                let capabilities = plugin.capabilities();

                // Permission sandbox enforcement
                if plugin_trust == TrustLevel::Unknown {
                    let mut denied = Vec::new();
                    for cap in &capabilities {
                        match cap {
                            AICapability::NetworkAccess
                            | AICapability::FileSystemAccess
                            | AICapability::ExecuteCode => {
                                denied.push(format!("{:?}", cap));
                            }
                            _ => {}
                        }
                    }
                    if !denied.is_empty() {
                        return Err(PluginLoadError::PermissionDenied {
                            path: path_display,
                            capabilities: denied.join(", "),
                        });
                    }
                }

                let name = plugin.name().to_string();
                self.ai_plugins
                    .insert(name, (plugin, plugin_core_version.clone()));
            }
        }

        self.libraries.push(library);

        Ok(())
    }

    /// Returns `(name, description, built_for_core_version)` for every loaded plugin.
    pub fn list_plugins(&self) -> Vec<(&str, &str, &str)> {
        self.plugins
            .iter()
            .map(|(n, (p, cv))| (n.as_str(), p.description(), cv.as_str()))
            .collect()
    }

    /// Returns all `PluginCommand`s advertised by every loaded plugin.
    pub fn list_commands(&self) -> Vec<crate::plugins::interface::PluginCommand> {
        self.plugins
            .values()
            .flat_map(|(p, _)| p.commands())
            .collect()
    }

    pub fn execute(&self, name: &str, args: &[String]) -> Result<(), String> {
        if let Some((plugin, _)) = self.plugins.get(name) {
            plugin.execute(args)
        } else {
            return Err(format!("Plugin '{}' not found", name));
        }
    }
}

struct ProxyRegistrar {
    plugins: Vec<Box<dyn Plugin>>,
}

impl ProxyRegistrar {
    fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }
}

impl PluginRegistrar for ProxyRegistrar {
    fn register_plugin(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.push(plugin);
    }
}

struct AIProxyRegistrar {
    plugins: Vec<Box<dyn AIPlugin>>,
}

impl AIProxyRegistrar {
    fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }
}

impl AIPluginRegistrar for AIProxyRegistrar {
    fn register_ai_plugin(&mut self, plugin: Box<dyn AIPlugin>) {
        self.plugins.push(plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Category labels ──────────────────────────────────────────────────────

    #[test]
    fn invalid_library_category() {
        let e = PluginLoadError::InvalidLibrary {
            path: "/tmp/bad.so".into(),
            detail: "No such file".into(),
        };
        assert_eq!(e.category(), "invalid_library");
    }

    #[test]
    fn missing_symbol_category() {
        let e = PluginLoadError::MissingRequiredSymbol {
            path: "/tmp/plugin.so".into(),
            symbol: "PLUGIN_DECLARATION".into(),
        };
        assert_eq!(e.category(), "missing_symbol");
    }

    #[test]
    fn abi_mismatch_category() {
        let e = PluginLoadError::AbiBuildMismatch {
            path: "/tmp/plugin.so".into(),
            plugin_rustc: "rustc 1.70.0".into(),
            required_rustc: "rustc 1.80.0".into(),
        };
        assert_eq!(e.category(), "abi_mismatch");
    }

    #[test]
    fn unsupported_core_version_category() {
        let e = PluginLoadError::UnsupportedCoreVersion {
            path: "/tmp/plugin.so".into(),
            plugin_core: "0.1.0".into(),
            running_core: "1.0.0".into(),
        };
        assert_eq!(e.category(), "unsupported_core_version");
    }

    #[test]
    fn manifest_incompatible_category() {
        let e = PluginLoadError::ManifestIncompatible {
            path: "/tmp/plugin.so".into(),
            detail: "major version mismatch".into(),
        };
        assert_eq!(e.category(), "manifest_incompatible");
    }

    #[test]
    fn registration_runtime_panic_category() {
        let e = PluginLoadError::RegistrationRuntimePanic {
            path: "/tmp/plugin.so".into(),
            detail: "poisoned pointer access".into(),
        };
        assert_eq!(e.category(), "runtime_panic");
    }

    // ── Diagnostic messages contain actionable guidance ──────────────────────

    #[test]
    fn invalid_library_diagnostic_mentions_fix() {
        let e = PluginLoadError::InvalidLibrary {
            path: "/tmp/bad.so".into(),
            detail: "invalid ELF header".into(),
        };
        let msg = e.diagnostic();
        assert!(msg.contains("/tmp/bad.so"));
        assert!(msg.contains("invalid ELF header"));
        assert!(msg.contains(".so") || msg.contains(".dylib") || msg.contains(".dll"));
    }

    #[test]
    fn missing_symbol_diagnostic_mentions_export_macro() {
        let e = PluginLoadError::MissingRequiredSymbol {
            path: "/tmp/plugin.so".into(),
            symbol: "PLUGIN_DECLARATION".into(),
        };
        let msg = e.diagnostic();
        assert!(msg.contains("PLUGIN_DECLARATION"));
        assert!(msg.contains("export_plugin"));
        assert!(msg.contains("cdylib"));
    }

    #[test]
    fn abi_mismatch_diagnostic_shows_both_versions() {
        let e = PluginLoadError::AbiBuildMismatch {
            path: "/tmp/plugin.so".into(),
            plugin_rustc: "rustc 1.70.0".into(),
            required_rustc: "rustc 1.80.0".into(),
        };
        let msg = e.diagnostic();
        assert!(msg.contains("rustc 1.70.0"));
        assert!(msg.contains("rustc 1.80.0"));
        assert!(msg.contains("rustup"));
    }

    #[test]
    fn unsupported_core_version_diagnostic_shows_both_versions() {
        let e = PluginLoadError::UnsupportedCoreVersion {
            path: "/tmp/plugin.so".into(),
            plugin_core: "0.1.0".into(),
            running_core: "1.0.0".into(),
        };
        let msg = e.diagnostic();
        assert!(msg.contains("0.1.0"));
        assert!(msg.contains("1.0.0"));
        assert!(msg.contains("starforge-plugin.toml") || msg.contains("Rebuild"));
    }

    #[test]
    fn manifest_incompatible_diagnostic_mentions_toml() {
        let e = PluginLoadError::ManifestIncompatible {
            path: "/tmp/plugin.so".into(),
            detail: "Plugin targets StarForge 0.1.0 but running 1.0.0".into(),
        };
        let msg = e.diagnostic();
        assert!(msg.contains("starforge-plugin.toml"));
        assert!(msg.contains("0.1.0"));
    }

    #[test]
    fn registration_runtime_panic_diagnostic_mentions_rules() {
        let e = PluginLoadError::RegistrationRuntimePanic {
            path: "/tmp/plugin.so".into(),
            detail: "forced assertion failure".into(),
        };
        let msg = e.diagnostic();
        assert!(msg.contains("crashed during registration"));
        assert!(msg.contains("forced assertion failure"));
    }

    #[test]
    fn display_matches_diagnostic() {
        let e = PluginLoadError::InvalidLibrary {
            path: "/tmp/bad.so".into(),
            detail: "os error 2".into(),
        };
        assert_eq!(format!("{}", e), e.diagnostic());
    }

    // ── load_plugin_diagnosed on a nonexistent path → InvalidLibrary ─────────

    #[test]
    fn nonexistent_path_returns_invalid_library() {
        let mut pm = PluginManager::new();
        let result = unsafe { pm.load_plugin_diagnosed(Path::new("/nonexistent/path/plugin.so")) };
        match result {
            Err(PluginLoadError::InvalidLibrary { path, .. }) => {
                assert!(path.contains("plugin.so"));
            }
            other => panic!("Expected InvalidLibrary, got {:?}", other),
        }
    }
}
