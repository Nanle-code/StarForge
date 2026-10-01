# Deploy a Soroban Contract

Structured tutorial for building, uploading, and instantiating your first Soroban smart contract on Stellar testnet with StarForge.

## Milestones

1. **Balance check** — confirm your deployer wallet has enough XLM for fees
2. **Build** — compile your contract to WASM with Stellar CLI
3. **Simulate** — dry-run to validate size, fees, and output before broadcasting
4. **Upload** — push the WASM to the Stellar network and record the contract hash
5. **Verify** — confirm the deployment appears in your local deploy history

## Prerequisites

- Completed the [hello-world](../hello-world/README.md) tutorial (wallet created and funded)
- A Soroban contract project in your current directory
- Stellar CLI installed (`stellar --version`)

## Interactive flow

```bash
starforge tool tutorial start deploy
starforge tool tutorial next    # after each milestone
starforge tool tutorial status
```

Step definitions live in `tutorial.json` beside this README.
