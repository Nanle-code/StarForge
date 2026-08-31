# Contract Upgrade Governance System

## 1. Overview
StarForge provides an on-chain/off-chain contract upgrade governance system for Soroban smart contracts. It guarantees safe, auditable, and multi-step upgrade lifecycles.

## 2. Governance Workflow
```
[Propose Upgrade] ──> [Voting Period] ──> [Threshold Reached (Passed)] ──> [Timelock Delay] ──> [Execution Ready] ──> [Execute Upgrade]
                                     └──> [Quorum Failed (Rejected)]
```

## 3. CLI Commands
- `starforge governance propose --contract-id <ID> --wasm <PATH> --description <DESC> --threshold <N> --timelock <SECS>`
- `starforge governance vote --proposal-id <ID> --for / --against`
- `starforge governance show --proposal-id <ID>`
- `starforge governance execute --proposal-id <ID>`
- `starforge governance emergency --proposal-id <ID> --reason <REASON>`
- `starforge governance audit --proposal-id <ID>`
- `starforge governance dashboard`
- `starforge governance config show / set`

## 4. Emergency Upgrades
Emergency upgrades bypass the standard timelock delay but require explicit cryptographic multi-guardian quorum approval (`emergency_quorum`). All emergency actions generate high-priority audit log entries.
