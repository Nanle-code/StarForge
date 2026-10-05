# {{PROJECT_NAME}}

A simple counter smart contract for Soroban.

## Features

- Increment counter
- Get current count
- Reset counter to zero

## Build

```bash
stellar contract build
```

## Test

```bash
cargo test
```

## Deploy

```bash
starforge deploy \
  --wasm target/wasm32v1-none/release/{{PROJECT_NAME_SNAKE}}.wasm \
  --network testnet
```

## Usage

```bash
# Increment the counter
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- increment

# Get current count
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- get_count

# Reset counter
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- reset
```
