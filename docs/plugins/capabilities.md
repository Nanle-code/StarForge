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
