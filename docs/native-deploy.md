# Native Soroban deploy (no stellar CLI)

`starforge deploy run --execute` uploads WASM and creates the contract instance
entirely over Soroban RPC:

1. `UploadContractWasm` (skipped when the WASM hash is already on-chain)
2. Simulate → assemble footprint / resource fee / auth
3. Sign with the selected wallet
4. `sendTransaction` + poll to SUCCESS
5. `CreateContract` / `CreateContractV2` (with `--constructor-arg` when needed)

```bash
starforge deploy run --wasm target/wasm32-unknown-unknown/release/hello.wasm \
  --wallet deployer --network testnet --execute

# Machine-readable contract id
starforge deploy run --wasm hello.wasm --wallet deployer --execute --json

# Print the legacy stellar CLI command instead
starforge deploy run --wasm hello.wasm --wallet deployer --print-only
```

Constructor arguments use `type:value` forms (`address:G…`, `u32:1`, `string:hi`,
`xdr:<base64 ScVal>`).
