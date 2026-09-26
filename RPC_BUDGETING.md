# Soroban RPC Request Budgeting and Backpressure

StarForge implements RPC request budgeting and backpressure to prevent overwhelming RPC providers and the CLI itself during orchestration and monitoring operations.

## Overview

The RPC budgeting system provides:

- **QPS (Queries Per Second) limits**: Control the rate of requests to each RPC endpoint
- **Concurrency limits**: Control the maximum number of simultaneous requests
- **Structured errors**: Clear error messages when budgets are exhausted
- **Telemetry hooks**: Optional telemetry for budget saturation monitoring

## Configuration

### Environment Variables

Configure RPC budgets using environment variables:

```bash
# Maximum requests per second (default: 10)
export STARFORGE_RPC_MAX_QPS=10

# Maximum concurrent requests (default: 5)
export STARFORGE_RPC_MAX_CONCURRENT=5

# Enable/disable budgeting (default: true)
export STARFORGE_RPC_BUDGET_ENABLED=true

# Enable telemetry for budget monitoring (optional)
export STARFORGE_RPC_TELEMETRY=1
```

### Preset Configurations

StarForge provides preset configurations for different use cases:

#### Default Configuration
- QPS: 10 requests/second
- Concurrency: 5 simultaneous requests
- Enabled: true

#### CI Configuration
More conservative for automated environments:
- QPS: 5 requests/second
- Concurrency: 3 simultaneous requests
- Enabled: true

#### Interactive Configuration
More permissive for interactive use:
- QPS: 15 requests/second
- Concurrency: 8 simultaneous requests
- Enabled: true

## Usage

### Programmatic Usage

```rust
use starforge::utils::rpc_budget::{RpcBudget, RpcBudgetConfig};

// Create a custom budget
let config = RpcBudgetConfig::new(20, 10, true);
let budget = RpcBudget::new(config);

// Acquire a permit before making an RPC request
let permit = budget.acquire_permit().await?;
// Make RPC request...
// Permit is automatically released when dropped
```

### Using Environment Configuration

```rust
use starforge::utils::rpc_budget::RpcBudget;

// Load configuration from environment variables
let budget = RpcBudget::from_env();
let permit = budget.acquire_permit().await?;
```

## Error Handling

When budgets are exhausted, the system provides clear error messages:

### QPS Limit Exceeded
```
Error: RPC QPS budget exhausted: 10 requests/sec limit reached. 
       Wait 1 second or increase STARFORGE_RPC_MAX_QPS.
```

### Concurrency Limit Exceeded
```
Error: RPC budget exhausted for https://rpc.example.com. 
       Wait or increase STARFORGE_RPC_MAX_CONCURRENT.
```

## Monitoring

### Budget Statistics

Get current budget usage statistics:

```rust
let stats = budget.stats();
println!("{}", stats.summary());
// Output: "RPC Budget: 7/10 QPS (70%), 3/5 concurrent (60%)"
```

### Saturation Detection

Check if budgets are near saturation (above 80% capacity):

```rust
if stats.is_near_saturation() {
    tracing::warn!("RPC budget near saturation: {}", stats.summary());
}
```

### Telemetry

Enable telemetry to track RPC request performance:

```bash
export STARFORGE_RPC_TELEMETRY=1
```

Telemetry logs request completion times when enabled.

## Best Practices

### For CI Environments
Use conservative limits to avoid overwhelming shared RPC infrastructure:

```bash
export STARFORGE_RPC_MAX_QPS=5
export STARFORGE_RPC_MAX_CONCURRENT=3
```

### For Interactive Use
Use higher limits for better responsiveness:

```bash
export STARFORGE_RPC_MAX_QPS=15
export STARFORGE_RPC_MAX_CONCURRENT=8
```

### For Monitoring/Orchestration
Consider enabling telemetry to track budget saturation:

```bash
export STARFORGE_RPC_TELEMETRY=1
```

### Disabling Budgeting
For development or when using dedicated RPC infrastructure:

```bash
export STARFORGE_RPC_BUDGET_ENABLED=false
```

## Implementation Details

### Per-Endpoint Budgeting
Budgets are tracked per RPC endpoint to prevent a single endpoint from monopolizing resources.

### Automatic Permit Release
Permits are automatically released when dropped, ensuring proper cleanup even in error cases.

### Thread-Safe Operation
The budget manager uses thread-safe data structures for concurrent access.

### Integration with Soroban RPC
The budgeting system is integrated into the Soroban RPC client in `src/utils/soroban.rs`, automatically enforcing budgets for all RPC requests.

## Testing

The budgeting system includes comprehensive tests:

```bash
# Run RPC budgeting tests
cargo test --lib utils::rpc_budget
```

## Troubleshooting

### Requests Failing with Budget Errors
1. Check current budget limits: `echo $STARFORGE_RPC_MAX_QPS`
2. Increase limits if needed: `export STARFORGE_RPC_MAX_QPS=20`
3. Wait for QPS window to reset (1 second)
4. Disable budgeting for testing: `export STARFORGE_RPC_BUDGET_ENABLED=false`

### Poor Performance
1. Check if budgets are too restrictive
2. Monitor saturation with telemetry enabled
3. Consider using interactive presets for development
4. Verify RPC endpoint performance independently

## Contributing

When adding new RPC operations:

1. Ensure they use the budgeting system via `rpc_request_with_url`
2. Add appropriate error handling for budget exhaustion
3. Consider the impact on QPS and concurrency limits
4. Update documentation if new configuration options are added
