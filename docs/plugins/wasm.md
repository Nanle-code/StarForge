# Sandboxed WebAssembly plugins

StarForge now provides a Wasmtime host for portable plugins. The host loads
WebAssembly into an isolated store, applies a fuel budget and epoch deadline,
and exposes no ambient imports by default. A plugin that imports WASI filesystem
or networking functions therefore fails during instantiation unless a future
capability-specific WIT host interface is explicitly granted. This behaviour is
verified in CI by the capability test harness described in
[Testing Capability Enforcement](capabilities.md#5-testing-capability-enforcement).

The stable interface is documented in [`wit/starforge-plugin.wit`](../../wit/starforge-plugin.wit).
SDK authors should target that contract rather than the Rust ABI used by the
legacy dynamic-library loader.

## Native plugin migration

Native plugins are retained only for transition compatibility and should be
enabled explicitly with:

```toml
starforge = { version = "0.1", features = ["unsafe-native-plugins"] }
```

New plugins should ship one `.wasm` component for Linux, macOS, and Windows.
Do not request filesystem or network capabilities unless the manifest and the
host deployment both grant the matching WIT interface.
