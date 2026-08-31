pub mod ai;
pub mod interface;
pub mod loader;
pub mod manifest;
pub mod registry;
pub mod verifier;

pub use ai::{
    AICapability, AIPlugin, AIPluginDeclaration, AIPluginRegistrar, AIRequest, AIResponse,
};
pub use interface::{Plugin, PluginDeclaration, PluginRegistrar};
pub use loader::{PluginLoadError, PluginManager};
pub use verifier::{verify_plugin_signature, VerificationResult, VerificationStatus};
