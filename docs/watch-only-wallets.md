# Watch-only wallets

Track balances, multisig members, and address-book aliases **without** storing
secret keys.

```bash
starforge wallet watch treasury --address G... --network testnet
starforge wallet list --json   # includes "watch_only": true
```

Watch-only wallets:

- Appear in `wallet list` / `wallet show` and can be referenced by name
- May be registered into the alias book with `--alias`
- Are valid cosigner *addresses* in multisig builders
- **Cannot sign** — signing commands fail with a clear `watch-only` error

Hardware-imported wallets (`wallet import --hardware …`) are also stored without
a local secret; they can still sign when you pass `--hardware ledger|trezor`.
Pure watch entries (no derivation path) reject `--hardware` as well.
