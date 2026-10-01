# Stellar Asset Contracts (SAC)

Compute the deterministic Soroban contract id for a classic asset, or wrap
(deploy) the SAC when it is missing.

```bash
# Native XLM
starforge asset contract-id XLM --network testnet

# Issued asset
starforge asset contract-id USDC:G... --network testnet --json

# Deploy / wrap if absent (pays fees from --wallet)
starforge asset wrap USDC:G... --wallet deployer --network testnet --yes
```

Use the SAC id with token templates and DeFi contracts that speak the SEP-41
token interface — for example after wrapping USDC, pass the `C…` id into a
token client instead of hand-computing the preimage.

See also: `starforge deploy run --execute` for native WASM deploy without the
stellar CLI.
