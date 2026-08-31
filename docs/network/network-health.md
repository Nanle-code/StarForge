# Machine-Readable Network Health Output

## 1. Overview
The `starforge network test` command supports machine-readable JSON outputs via the `--json` flag or global `--json` mode.

## 2. Command Usage
```bash
# Test default active network with JSON output
starforge network test --json

# Test specific network with JSON output
starforge network test testnet --json
```

## 3. JSON Output Schema
```json
{
  "network": "testnet",
  "healthy": true,
  "timestamp": "2026-08-29T12:00:00Z",
  "horizon": {
    "url": "https://horizon-testnet.stellar.org",
    "reachable": true,
    "latency_ms": 142,
    "latest_ledger": 1234567,
    "protocol_version": 20,
    "horizon_version": "2.28.0",
    "error": null
  },
  "soroban_rpc": {
    "url": "https://soroban-testnet.stellar.org",
    "reachable": true,
    "latency_ms": 180,
    "status": "healthy",
    "error": null
  },
  "friendbot": {
    "url": "https://friendbot.stellar.org",
    "reachable": true,
    "latency_ms": 95,
    "status": "healthy",
    "error": null
  }
}
```
