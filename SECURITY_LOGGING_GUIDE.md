# Security Logging and Audit Guide

This guide explains how to properly log security-relevant operations in StarForge to maintain reliable audit trails and debugging capabilities.

## Table of Contents

1. [Philosophy](#philosophy)
2. [Sensitivity Levels](#sensitivity-levels)
3. [What to Log](#what-to-log)
4. [What NOT to Log](#what-not-to-log)
5. [Structured Logging Format](#structured-logging-format)
6. [Security-Relevant Operations](#security-relevant-operations)
7. [Audit Trail Requirements](#audit-trail-requirements)
8. [Code Examples](#code-examples)
9. [Verification and Testing](#verification-and-testing)

---

## Philosophy

Security and auditability are critical in blockchain tooling where users interact with real assets and sensitive keys. Every security-relevant operation should:

1. **Be logged** - Operations that affect security state must be recorded
2. **Be traceable** - Include enough context to understand what happened
3. **Be sanitized** - Remove sensitive data from logs
4. **Be useful** - Provide actionable information for debugging and auditing

StarForge uses **structured logging** (JSON format available) with the `tracing` crate to ensure logs are machine-parseable and can be aggregated for analysis.

All configured terminal and rotating-file tracing outputs pass through a central
redaction boundary. Secret keys, public key material, mnemonic phrases, signed
XDR, JWT-style tokens, and values assigned to sensitive field names are replaced
with `[REDACTED]`, including values in formatted error chains. Invalid or
unsupported input is reported without echoing the rejected value. This applies
to both human and JSON formats and does not depend on the log level.

When adding diagnostics, keep sensitive material out of error/context strings
where possible; the boundary is defense in depth, not a substitute for avoiding
secret-bearing values. Run `cargo check --lib`, `cargo test --test security_logging_audit`,
and `cargo test --test correlation_logging` after changing logging code.

---

## Sensitivity Levels

### Public Information
Safe to log in all contexts:
- User-provided wallet/contract names
- Public keys (Stellar G-accounts)
- Network names (testnet, mainnet)
- Operation types (create, deploy, inspect)
- Success/failure status
- Timestamps

Example:
```json
{
  "timestamp": "2024-01-15T10:30:45Z",
  "operation": "wallet_create",
  "wallet_name": "alice",
  "network": "testnet",
  "encrypted": true,
  "success": true
}
```

### Private Information
Log only at DEBUG/TRACE level or never:
- Account addresses in some contexts
- Gas costs and fees (operational)
- Algorithm parameters (KDF iterations, salt size)
- Error messages with technical details

Example (only at DEBUG level):
```rust
debug!(address = %redact_public_key(addr, Level::DEBUG), "Account balance query");
```

### Sensitive Information
**NEVER log under any circumstances:**
- Private keys / secret keys
- Passphrases
- Encryption keys or derived keys
- Signed transactions or XDR payloads
- Wallet backup data
- Plugin source code or binaries
- Authentication tokens

---

## What to Log

### For Every Security-Relevant Operation

1. **Operation Type** - What is being done (create, encrypt, deploy, load, execute)
2. **Operation Status** - Success or failure
3. **Operation Context** - Who/what triggered it, which account/resource
4. **Timestamp** - When it happened
5. **Duration** - How long it took (important for crypto operations)
6. **Error Details** - If failed, what went wrong (without revealing secrets)

Example structure:
```rust
info!(
    operation = "wallet_encrypt",
    wallet = "alice",
    status = "success",
    duration_ms = 245,
    "Wallet encryption completed"
);
```

### Specific Operation Categories

#### Wallet Operations
```
Operation: wallet_create
Logs:
  - wallet name, network, encryption enabled, success
  
Operation: wallet_encrypt
Logs:
  - wallet name, encryption algorithm, KDF iterations, success
  
Operation: wallet_fund
Logs:
  - wallet name, network, requested amount, balance after, success
  
Operation: wallet_remove
Logs:
  - wallet name, network, confirmation status, success
  
Operation: wallet_show
Logs:
  - wallet name, reveal_requested, balance_query_status
```

#### Plugin Operations
```
Operation: plugin_load
Logs:
  - plugin name, plugin version, source path, compatibility check result
  
Operation: plugin_execute
Logs:
  - plugin name, command args (sanitized), execution status, duration
  
Operation: plugin_unload
Logs:
  - plugin name, cleanup status, success
```

#### Deployment Operations
```
Operation: contract_deploy
Logs:
  - contract path, network, account, wasm hash, deployment status, gas
  
Operation: contract_validate
Logs:
  - contract path, validation status, file size, warnings/errors
  
Operation: contract_inspect
Logs:
  - contract address, network, inspection status, metadata retrieved
```

#### Network Operations
```
Operation: network_switch
Logs:
  - from_network, to_network, connectivity_check_status
  
Operation: network_test
Logs:
  - network name, horizon_status, soroban_status, latency
```

---

## What NOT to Log

### Absolutely Never Log

```rust
// ❌ WRONG - Never log secret keys
warn!("Secret key: {}", secret_key);

// ❌ WRONG - Never log passphrases
info!("User entered passphrase: {}", passphrase);

// ❌ WRONG - Never log private/sensitive material
debug!("Encryption key: {}", derived_key);

// ❌ WRONG - Never log full XDR payloads
trace!("Signed transaction: {}", xdr_envelope);
```

### Avoid Logging

```rust
// ❌ Unnecessary - Don't log function entry/exit for every function
trace!("Entering validate_key()");

// ❌ Noisy - Don't log every iteration
for item in items {
    trace!("Processing: {:?}", item);  // Too verbose
}

// ❌ Sensitive context - Don't log full error messages with context
error!("Failed to decrypt wallet {} with passphrase {}: {}", name, pass, err);
```

### Safe to Log (with Redaction)

```rust
// ✅ Redact public keys at INFO level
info!(key = %redact_public_key(addr, Level::INFO), "Account queried");

// ✅ Never redact error messages (no secrets)
error!("Network connection failed: {}", err);

// ✅ Log operation metadata
info!(
    wallet = "alice",
    network = "testnet",
    operation = "fund",
    status = "success"
);
```

---

## Structured Logging Format

StarForge uses structured logging for machine-parseability. Each log entry includes:

### Human-Readable Format
```
2024-01-15T10:30:45.123Z [INFO] starforge::commands::wallet: Wallet encrypted
    wallet: alice
    algorithm: aes-256-gcm
    iterations: 100000
    duration_ms: 245
```

### JSON Format
```json
{
  "timestamp": "2024-01-15T10:30:45.123Z",
  "level": "INFO",
  "target": "starforge::commands::wallet",
  "message": "Wallet encrypted",
  "fields": {
    "wallet": "alice",
    "algorithm": "aes-256-gcm",
    "iterations": 100000,
    "duration_ms": 245
  }
}
```

### Key Guidelines

1. **Use structured fields** - Not free-text messages
2. **Use consistent names** - `wallet`, `network`, `operation`, `status`
3. **Use appropriate types** - Timestamps as ISO-8601, durations as milliseconds
4. **Be specific** - `status: "success"` not `message: "OK"`

---

## Security-Relevant Operations

### Wallet Management

| Operation | Logs | Sensitivity |
|-----------|------|-------------|
| Create | name, network, encryption enabled, result | Public |
| Encrypt | name, algorithm, KDF params (size, not value) | Public |
| Decrypt | name, attempt status | Public |
| Fund | name, network, amount, balance after | Public |
| Show | name, reveal_requested, balance_status | Public |
| Remove | name, confirmation, result | Public |
| Rotate | name, new_account_created, result | Public |

**Never log:** Secret keys, passphrases, private key material

### Plugin Management

| Operation | Logs | Sensitivity |
|-----------|------|-------------|
| Load | name, version, source_path, verify status | Public |
| Execute | name, args (sanitized), duration, result | Public |
| Unload | name, cleanup_status | Public |
| Compatibility Check | name, starforge_version, plugin_version, compatible | Public |

**Never log:** Plugin source code, loaded binaries, execution results with sensitive data

### Contract Operations

| Operation | Logs | Sensitivity |
|-----------|------|-------------|
| Validate | file_path, file_size, validation_result | Public |
| Deploy | network, account (redacted), hash, gas, result | Public |
| Inspect | address (redacted), network, inspection_result | Public |
| Invoke | address (redacted), function, param_count, result | Public |

**Never log:** Private contract state, function arguments with secrets, signed XDR

### Network Operations

| Operation | Logs | Sensitivity |
|-----------|------|-------------|
| Switch | from_network, to_network, verify_status | Public |
| Test | network, endpoint_status, latency_ms | Public |
| Add Custom | network_name, urls (redacted), result | Public |

**Never log:** API keys for custom networks

---

## Audit Trail Requirements

### Timestamp Precision

All security operations must include precise timestamps for audit correlation:

```rust
use chrono::Utc;

info!(
    timestamp = %Utc::now(),
    operation = "wallet_create",
    "New wallet created"
);
```

### Contextual Information

Each security operation should include enough context to understand the full flow:

```rust
info!(
    session_id = uuid,
    user_action = "import_wallet",
    wallet_name = "alice",
    network = "testnet",
    encrypted = true,
    duration_ms = 150,
    status = "success",
    "Wallet import completed"
);
```

### Error Details Without Secrets

When logging errors, include diagnostic information but not sensitive data:

```rust
// ✅ Good - Specific error type without secrets
error!(
    wallet = "alice",
    error_type = "DecryptionFailed",
    error_code = "ERR_WRONG_PASSPHRASE",
    attempts = 3,
    "Failed to decrypt wallet"
);

// ❌ Bad - Too specific about secrets
error!(
    wallet = "alice",
    error = "Failed to decrypt with provided passphrase",
    passphrase_length = 8,
    "Decryption failed"
);
```

### Result Summaries

Always log the result of security operations:

```rust
info!(
    operation = "deploy",
    account = redact_public_key(addr, Level::INFO),
    network = "testnet",
    wasm_hash = hash,
    gas_used = 12345,
    result = "success",
    "Contract deployed"
);
```

---

## Code Examples

### Logging a Wallet Creation

```rust
use tracing::info;

fn create_wallet(name: &str, network: &str, encrypt: bool) -> Result<Wallet> {
    let start = std::time::Instant::now();
    
    let wallet = Wallet::new(name, network)?;
    
    if encrypt {
        wallet.encrypt(passphrase)?;
    }
    
    let duration_ms = start.elapsed().as_millis() as u64;
    
    info!(
        wallet = name,
        network = network,
        encrypted = encrypt,
        duration_ms = duration_ms,
        status = "success",
        "Wallet created"
    );
    
    Ok(wallet)
}
```

### Logging a Plugin Load

```rust
use tracing::info;

fn load_plugin(plugin_path: &str, name: &str) -> Result<Plugin> {
    let plugin = unsafe { pm.load_plugin(plugin_path)? };
    
    let plugin_version = plugin.version();
    let starforge_version = env!("CARGO_PKG_VERSION");
    
    info!(
        plugin_name = name,
        plugin_version = plugin_version,
        starforge_version = starforge_version,
        source = plugin_path,
        status = "success",
        "Plugin loaded"
    );
    
    Ok(plugin)
}
```

### Logging a Contract Deployment

```rust
use tracing::info;

fn deploy_contract(wasm_path: &str, network: &str, account: &str) -> Result<String> {
    let wasm_hash = calculate_wasm_hash(wasm_path)?;
    
    let result = execute_deployment(wasm_path, network, account)?;
    
    info!(
        wasm_hash = %wasm_hash,
        network = network,
        account = %redact_public_key(account, Level::INFO),
        contract_address = %result.address,
        gas_used = result.gas_used,
        status = "success",
        "Contract deployed"
    );
    
    Ok(result.address)
}
```

### Logging with Error Handling

```rust
use tracing::{info, error};

fn fund_wallet(wallet: &str, network: &str, amount: u64) -> Result<()> {
    match request_funds(wallet, network, amount) {
        Ok(tx_hash) => {
            info!(
                wallet = wallet,
                network = network,
                amount = amount,
                transaction_hash = tx_hash,
                status = "success",
                "Wallet funded"
            );
            Ok(())
        }
        Err(e) => {
            error!(
                wallet = wallet,
                network = network,
                error_type = "FundingFailed",
                error_message = %e,
                status = "failed",
                "Failed to fund wallet"
            );
            Err(e)
        }
    }
}
```

---

## Verification and Testing

### Testing Security Logging

Always test that security operations log correctly:

```rust
#[test]
fn test_wallet_creation_is_logged() {
    // Setup logging capture
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .finish();
    
    let _guard = tracing::subscriber::set_default(subscriber);
    
    // Perform security operation
    let wallet = Wallet::new("test", "testnet").unwrap();
    
    // Verify logs were emitted (integration test would check log output)
    assert_eq!(wallet.name, "test");
}
```

### Checking Logs in CI

Enable JSON logging in CI for better parsing:

```bash
# In CI environment
cargo test --log-format json --log-dir ./logs

# Verify logs contain required fields
jq '.fields | has("wallet") and has("status")' logs/*.log | grep true
```

### Log Audit Review

Regular review of security logs:

1. **Weekly** - Check for failed authentication attempts
2. **On deployment** - Verify deployment logs are complete
3. **On incidents** - Full log trace of affected operations
4. **On access** - Who accessed sensitive operations and when

---

## Further Reading

- [AUDIT_TRAIL_DOCUMENTATION.md](AUDIT_TRAIL_DOCUMENTATION.md) - Detailed audit trail setup
- [SECURITY_LOGGING_BEST_PRACTICES.md](SECURITY_LOGGING_BEST_PRACTICES.md) - Implementation patterns
- [src/utils/logging.rs](src/utils/logging.rs) - Logging infrastructure
- [tracing crate](https://docs.rs/tracing/) - Official tracing documentation

---

*Last updated: 2026-06-01*  
*Issue #223: Improve security logging and auditability*
