# Adding contract components

## Use

Create a project with `starforge new contract my-token --template token`, enter its directory, and run `starforge add ownable`. Use `starforge add --list` to list components, `--path <dir>` to target another project, or `--dry-run` to inspect the generated diff. A failed preflight reports the conflict and leaves project files untouched.

| Component | API | Composition |
| --- | --- | --- |
| `ownable` | `owner()`, `transfer_ownership(current, next)` | Stores `OWNER`; ownership starts on the first transfer call. |
| `access-control` | `grant_role(admin, account, role)`, `revoke_role(...)`, `has_role(account, role)` | Uses independent persistent role entries. |
| `pausable` | `pause(admin)`, `unpause(admin)`, `is_paused()` | Uses `PAUSED`. Business methods must check `is_paused`. |
| `upgradeable` | `version()`, `upgrade(admin, wasm_hash)` | Uses `VERSION` and Soroban's current contract WASM upgrade. |

The administrator arguments on access-control, pausable, and upgradeable require Soroban authorization. These management methods do not by themselves enforce business method policy; callers should add checks to protected operations. Components can be combined because each uses independent storage. Existing function-name collisions and repeat applications are refused.

## Contributor guide

Add a small Rust methods template under `templates/components/` and one entry in `REGISTRY` in `src/commands/add.rs`. The CLI parses the existing Rust source with `syn`, discovers the contract struct and its `#[contractimpl]` block, checks API function collisions, then inserts the methods into that block and adds an application marker. Do not rewrite the user's source with a formatter or AST printer. Declare file, dependency, prerequisite, and conflict metadata in the registry; add tests and document expected composition. All project edits are prepared before writing, and failed writes restore prior contents.
