//! StarForge Plugin SDK
//!
//! Implement the [`StarForgePlugin`] trait and use [`export_plugin!`] to
//! expose your plugin to the StarForge CLI loader.

/// Metadata every plugin must provide.
#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub starforge_version: &'static str,
}

impl PluginMeta {
    pub fn new(name: &'static str, version: &'static str, description: &'static str) -> Self {
        Self {
            name,
            version,
            description,
            starforge_version: env!("CARGO_PKG_VERSION"),
        }
    }

    pub fn with_starforge_version(
        name: &'static str,
        version: &'static str,
        description: &'static str,
        starforge_version: &'static str,
    ) -> Self {
        Self {
            name,
            version,
            description,
            starforge_version,
        }
    }

    /// Checks if this plugin's target starforge_version has matching major version with running CLI core.
    pub fn is_compatible_with(&self, running_core_version: &str) -> bool {
        let plugin_major = self.starforge_version.split('.').next().unwrap_or("0");
        let core_major = running_core_version.split('.').next().unwrap_or("0");
        plugin_major == core_major
    }
}

/// Core trait all StarForge plugins must implement.
pub trait StarForgePlugin {
    fn meta(&self) -> PluginMeta;
    fn run(&self, args: &[String]) -> Result<(), String>;
}

/// Exports a plugin so the StarForge CLI can load it via `libloading`.
///
/// # Example
/// ```rust,ignore
/// use starforge_plugin_sdk::{export_plugin, PluginMeta, StarForgePlugin};
///
/// struct MyPlugin;
///
/// impl StarForgePlugin for MyPlugin {
///     fn meta(&self) -> PluginMeta {
///         PluginMeta::new("my-plugin", "0.1.0", "Does something cool")
///     }
///     fn run(&self, args: &[String]) -> Result<(), String> {
///         println!("my-plugin args: {:?}", args);
///         Ok(())
///     }
/// }
///
/// export_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        #[no_mangle]
        pub extern "C" fn _starforge_plugin_create(
        ) -> *mut dyn $crate::StarForgePlugin {
            let plugin = <$plugin_type>::default();
            Box::into_raw(Box::new(plugin))
        }
    };
}
