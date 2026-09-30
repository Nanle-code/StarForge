//! Minimal StarForge plugin used by the plugin authoring cookbook
//! (`docs/plugins/cookbook.md`).
//!
//! Once installed, `starforge hello [NAME]` prints a greeting.

use starforge::plugins::{Plugin, PluginRegistrar};

#[derive(Debug, Default)]
pub struct HelloPlugin;

impl HelloPlugin {
    /// Builds the greeting for the given CLI arguments.
    pub fn greeting(args: &[String]) -> String {
        let who = args.first().map(String::as_str).unwrap_or("world");
        format!("Hello, {who}!")
    }
}

impl Plugin for HelloPlugin {
    fn name(&self) -> &'static str {
        "hello"
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &'static str {
        "Prints a greeting (plugin cookbook example)"
    }

    fn execute(&self, args: &[String]) -> Result<(), String> {
        println!("{}", Self::greeting(args));
        Ok(())
    }
}

/// Called once by the StarForge loader after the library is opened.
pub fn register(registrar: &mut dyn PluginRegistrar) {
    registrar.register_plugin(Box::new(HelloPlugin));
}

starforge::export_plugin!(register);

#[cfg(test)]
mod tests {
    use super::*;
    use starforge::plugins::manifest::load_manifest_for_library;
    use std::path::Path;

    /// Collects registered plugins the same way the host's registrar does.
    #[derive(Default)]
    struct TestRegistrar(Vec<Box<dyn Plugin>>);

    impl PluginRegistrar for TestRegistrar {
        fn register_plugin(&mut self, plugin: Box<dyn Plugin>) {
            self.0.push(plugin);
        }
    }

    #[test]
    fn greets_world_by_default() {
        assert_eq!(HelloPlugin::greeting(&[]), "Hello, world!");
    }

    #[test]
    fn greets_first_argument() {
        let args = vec!["Stellar".to_string(), "ignored".to_string()];
        assert_eq!(HelloPlugin::greeting(&args), "Hello, Stellar!");
    }

    #[test]
    fn execute_succeeds() {
        assert_eq!(HelloPlugin.execute(&[]), Ok(()));
    }

    #[test]
    fn register_exposes_one_command_named_after_the_plugin() {
        let mut registrar = TestRegistrar::default();
        register(&mut registrar);

        assert_eq!(registrar.0.len(), 1);
        let commands = registrar.0[0].commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "hello");
    }

    #[test]
    fn manifest_matches_plugin_and_running_core() {
        // `load_manifest_for_library` looks beside the library path, exactly
        // as `starforge plugin install` does.
        let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("libstarforge_hello.so");
        let manifest = load_manifest_for_library(&lib)
            .expect("manifest parses")
            .expect("starforge-plugin.toml exists");

        assert_eq!(manifest.name, HelloPlugin.name());
        assert_eq!(manifest.version, HelloPlugin.version());
        assert_eq!(manifest.description, HelloPlugin.description());
        manifest
            .validate()
            .expect("manifest is compatible with the StarForge core it is built against");
    }
}
