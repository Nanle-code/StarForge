# Telemetry & Privacy

StarForge collects telemetry data to help us understand usage patterns and improve the CLI. This document explains exactly what is collected, how to disable it, and what we do with the data.

## Privacy-First Design

- **Opt-in by default**: Telemetry is disabled by default until you explicitly enable it
- **No personal data**: We never collect personal information, credentials, or sensitive data
- **Anonymous ID**: Usage is tracked with a random UUID, not tied to your identity
- **Local-first**: Telemetry is stored locally in your machine and only sent with explicit consent (future versions)
- **Transparent**: Full source code is available for audit

## What Data is Collected

For each CLI command executed, StarForge collects:

- **Command name**: Which command was run (e.g., `wallet`, `deploy`, `new`)
- **Timestamp**: When the command was executed
- **Success/Failure**: Whether the command completed successfully
- **Duration**: How long the command took in milliseconds
- **Anonymous ID**: A random UUID generated once per machine

### Example Telemetry Event

```json
{
  "timestamp": "2025-01-15T10:30:45Z",
  "event": "deploy",
  "properties": {
    "success": true,
    "duration_ms": 2500
  },
  "anonymous_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

### What We Do NOT Collect

- ❌ Wallet addresses or secret keys
- ❌ Contract code or source files
- ❌ Configuration values (network URLs, custom networks)
- ❌ Error messages or stack traces
- ❌ User identity, email, or personal information
- ❌ File paths or local system information

## How to Disable Telemetry

### Option 1: Configuration Command (Recommended)

Enable telemetry explicitly using the `config` command:

```bash
starforge config set telemetry true
```

View your current telemetry setting:

```bash
starforge config show
# or
starforge telemetry status
```

Disable telemetry again:

```bash
starforge config set telemetry false
```

### Option 2: Environment Variable

Enable telemetry for a single command or session:

```bash
# Enable for a single command
STARFORGE_TELEMETRY=1 starforge deploy --wasm my_contract.wasm

# Enable for the entire shell session
export STARFORGE_TELEMETRY=1
starforge wallet list
starforge deploy --wasm my_contract.wasm
```

Accepted values to disable telemetry:
- `0`, `false`, `off`, `disabled`, `no`

Accepted values to enable telemetry:
- `1`, `true`, `on`, `enabled`, `yes`

### Option 3: CI/CD Pipelines

For automated environments, set the environment variable explicitly:

```bash
# In GitHub Actions
env:
  STARFORGE_TELEMETRY: "1"

# In GitLab CI
script:
  - export STARFORGE_TELEMETRY=1
  - starforge deploy --wasm my_contract.wasm
```

## Configuration Storage

Your telemetry preference is stored in:

```
~/.starforge/config.toml
```

Example:

```toml
network = "testnet"
telemetry_enabled = false

[[wallets]]
name = "deployer"
public_key = "GABC...XYZ"
# ...
```

Telemetry logs are stored in:

```
~/.starforge/data/telemetry.log
~/.starforge/data/anonymous_id
```

These files are created only if telemetry is enabled. You can inspect the exact payload with:

```bash
starforge telemetry payload
```

And fully wipe the local audit trail with:

```bash
starforge telemetry reset
```

## Future: Remote Telemetry

In future versions, StarForge may offer opt-in remote telemetry (sending anonymous data to a analytics service). This will be:

- **Completely optional**: Requires explicit opt-in, never enabled by default
- **Aggregated**: Only high-level statistics are sent (e.g., "10 deployments per day", not individual events)
- **Auditable**: Full details published on what is collected and where it goes
- **Respectable**: Always respects `STARFORGE_TELEMETRY=0` and config settings

## Questions or Concerns?

If you have privacy concerns or questions about telemetry:

1. **Review the code**: Full source available at https://github.com/Josetic224/StarForge
2. **Check the logs**: Inspect `~/.starforge/data/telemetry.log` to see what was collected
3. **Disable it**: Use `starforge config set telemetry false` if you prefer not to participate
4. **Report issues**: Open an issue on GitHub with any privacy concerns

## Additional Privacy Resources

- [PRIVACY_POLICY.md](./PRIVACY_POLICY.md) - Full privacy policy
- [SECURITY_LOGGING_AUDIT.md](./SECURITY_LOGGING_AUDIT.md) - Security logging details
- [docs/DATA_FLOW_INVENTORY.md](docs/DATA_FLOW_INVENTORY.md) - Full inventory of where secrets and PII can flow across the CLI (config, logs, telemetry, AI prompts, stdout/stderr), the controls in place, and known gaps
- [GitHub Repository](https://github.com/Josetic224/StarForge) - Open source, fully auditable
