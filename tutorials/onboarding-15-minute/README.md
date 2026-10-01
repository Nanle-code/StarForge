# 15-Minute StarForge Onboarding

Get StarForge installed, create a local wallet, scaffold a Soroban contract, and run a deterministic simulation. The checkpoints verify local results; they do not fund the wallet or submit a transaction.

## Start

From a working directory where you want the sample contract created, run:

```bash
starforge tool tutorial start onboarding-15-minute --demo
```

After completing each displayed command, verify and advance with:

```bash
starforge tool tutorial next
starforge tool tutorial status
```

Progress is saved locally in StarForge's configuration directory. The tutorial does not send completion data to a server.

## Checkpoints

1. **Install (2 minutes)** - `starforge --version` succeeds.
2. **Wallet (3 minutes)** - a local wallet named `onboarding` exists. Do not add `--fund`; the tutorial needs no testnet funds.
3. **Scaffold (6 minutes)** - `onboarding-contract/Cargo.toml` and `onboarding-contract/src/lib.rs` exist.
4. **Simulate (4 minutes)** - the deterministic `simple-counter` scenario returns accounts and contracts without contacting a network.

If a checkpoint fails, `tutorial next` reports what was missing and a repair hint. Fix the issue and retry; progress does not advance on a failed checkpoint.

The contract scaffold is created in the current working directory. Choose an empty, writable directory, or change the project name in the commands if `onboarding` or `onboarding-contract` already exists.
