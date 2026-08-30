# CLI JSON output stability

StarForge commands that expose `--json` output must declare the stability of
their documented JSON fields. The contract is tracked in
`docs/contracts/cli-json-fields.json` and enforced by
`tests/json_contract_stability.rs`.

## Stability tiers

| Tier | Meaning | Change rules |
| --- | --- | --- |
| `stable` | Field is safe for scripts and integrations to depend on. | Do not remove or rename the field in the same major version. To remove it, first change the field to `deprecated` and document the replacement or removal reason. |
| `experimental` | Field is available for early feedback, but may change. | The field may be renamed, removed, or have its shape changed in a minor release. Promote it to `stable` once consumers can depend on it. |
| `deprecated` | Field is still emitted for compatibility but scheduled for removal. | Keep the field documented with a `deprecated_since` version and `removal_guidance`. Announce the removal in the changelog before deleting it. |

## Changelog rules

- Stable additions: note the command, field path, and intended consumer use.
- Experimental additions or changes: note that the field is not yet a
  compatibility promise.
- Deprecations: include the field path, `deprecated_since` version, replacement
  if any, and the earliest version where removal can happen.
- Stable removals: only allowed after a prior deprecation entry and an updated
  JSON contract that marks the field `deprecated`.

## Maintaining the contract

When changing JSON output for a command:

1. Update `docs/contracts/cli-json-fields.json`.
2. Add new stable fields to
   `tests/fixtures/json_contracts/stable-fields-baseline.json`.
3. If a stable field is being retired, keep it in the contract and mark it
   `deprecated`; do not delete it directly.
4. Run `cargo test --test json_contract_stability --locked`.

