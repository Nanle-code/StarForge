## Summary

Implements four Advancement issues focused on Soroban state archival operations and CI quality gates.

- **#908 — Contract TTL commands:** `starforge contract ttl show` lists instance/code/persistent live-until ledgers with ETA; `--warn-below` exits non-zero for cron/monitors; `ttl extend --ledgers N` simulates cost then builds/submits `ExtendFootprintTTL`.
- **#907 — Auto-restore before invoke:** Detects `restorePreamble` in simulation, shows restore cost, prompts or honors `--auto-restore` / `--yes`, submits `RestoreFootprintOp`, re-simulates; `--json` reports `restored` / `restore_tx_hash`. Documented in `docs/guides/INVOKE.md`.
- **#904 — Coverage ratchet:** `coverage.yml` uploads LLVM coverage artifacts + Codecov; enforces `coverage-ratchet.toml` global floor and higher critical-path floors for crypto/wallet/config; `codecov.yml` patch/project/component status; README coverage badge.
- **#898 — CI workflow enforcement:** `rustfmt` is a hard gate (tree formatted); `secure-defaults` remains its own job; added `actionlint` job over all workflows. Also repairs truncated `home_lock` test helpers and a corrupted `build.rs` Commands enum that blocked compilation of the cheat-sheet generator.

closes #908
closes #907
closes #904
closes #898

## Test plan

- [ ] `cargo fmt --all --check`
- [ ] `actionlint -color` (or CI Actionlint job)
- [ ] `cargo test -p starforge contract_ttl --lib`
- [ ] `cargo test -p starforge simulation_resources --lib`
- [ ] Local/quickstart: `starforge contract ttl show <C…> --network docker-testnet`
- [ ] `starforge contract ttl extend <C…> --ledgers 100000 --wallet …` shows fee before `--submit`
- [ ] Force archival locally, then `starforge contract invoke … --auto-restore --json` reports `"restored": true`
- [ ] Coverage workflow attaches `coverage-report` artifact and ratchet step passes
- [ ] Unformatted file makes Rustfmt CI fail
