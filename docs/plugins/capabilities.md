# Plugin Capabilities and Access Control

## 1. Overview
StarForge plugins operate under an explicit capabilities security model. Plugins must declare all required host capabilities in their `starforge-plugin.toml` manifest.

## 2. Declaring Capabilities in Manifest
```toml
name = "my-plugin"
version = "1.0.0"
starforge_version = "0.1.0"
description = "Demonstrates explicit capabilities"
required_capabilities = [
    "fs:read",
    "network",
]
```

## 3. Supported Capabilities
| Capability | Description |
| :--- | :--- |
| `fs:read` | Read access to host filesystem paths |
| `fs:write` | Write and modification access to host filesystem paths |
| `network` (`net:http`) | Outbound network HTTP/WebSocket connections |
| `contract:invoke` | Ability to execute Soroban contract invocations |
| `ai` | Access to StarForge AI routing and completion subsystems |

## 4. Runtime Enforcement
At runtime, undeclared access attempts are rejected immediately with a descriptive permission denied diagnostic instructing developers to add the missing capability to `starforge-plugin.toml`.

## 5. Testing Capability Enforcement
Capability enforcement is covered by an integration test harness that runs in CI as the **Plugin Capability Enforcement** job:

```bash norun
cargo test --test plugin_capability_integration --locked
```

The harness lives in [`tests/plugin_capability_integration.rs`](../../tests/plugin_capability_integration.rs) and uses two sample plugins under [`tests/fixtures/plugins/capabilities/`](../../tests/fixtures/plugins/capabilities/):

| Fixture | Manifest | Behaviour | Expected result |
| :--- | :--- | :--- | :--- |
| `malicious/` | declares no capabilities | WASM modules import WASI `path_open`, `fd_write` and `sock_send` | Manifest gate denies `fs:read`, `fs:write` and `network`; the WASM sandbox refuses to instantiate each module |
| `benign/` | declares `fs:read` and `network` | Pure-computation WASM module with no host imports | Declared capabilities are granted, undeclared `fs:write` is still denied, and the module runs |

To check your own plugin against the same rules, copy a fixture directory, replace the manifest and `.wat`/`.wasm` module with yours, and add a test that calls `PluginManifest::enforce_filesystem_access` / `enforce_network_access` and `PluginManager::load_wasm_plugin(..).run()` as the existing tests do. Failures print which capability or host import was denied and which plugin requested it.
