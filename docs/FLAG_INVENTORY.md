# Flag Inventory Report

> **Auto-generated** (simulated) from clap metadata.

| Flag | Description |
|------|-------------|
| `--network` | The network to connect to. |
| `--wallet` / `--source` | The wallet or source account to use. |
| `--yes` | Skip confirmation prompts. |
| `--dry-run` | Simulate operations. |
| `--verbose` | Enable verbose output. |
| `--json` | Enable machine-readable JSON output. |

## Migration Status
A test `tests/cli_global_flags.rs` has been added to ensure that the above flags are standardized across the codebase using `#[command(flatten)] pub network: NetworkFlag` etc.
