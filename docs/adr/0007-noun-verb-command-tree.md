# ADR 0007: Noun-Verb Command Tree for the `starforge` CLI

* **Status**: Accepted
* **Deciders**: StarForge Core Maintainers
* **Date**: 2026-09-27
* **Technical Area**: CLI

## Context and Problem Statement

`src/main.rs` registered **86 top-level subcommands**, 21 of which were `ai-*`
prefixed variants (`ai-debug`, `ai-navigate`, `ai-quality-gate`, `ai-profile`,
`ai-ide`, `ai-test`, `ai-test-maintain`, `ai-deployment-test`, `ai-audit`,
`ai-property-test`, `ai-feedback`, `ai-search`, `ai-recommend`, `ai-route`,
`ai-plan`, `ai-accessibility`, `ai-contract-suggest`, `ai-doc-qa`,
`ai-telemetry`, `ai-security-training`, plus the `ai` assistant itself).

The consequences were visible everywhere:

* `starforge --help` printed a wall of 80+ lines that a new user had to read in
  full to guess which command did anything. Discovery was effectively
  brute-force search rather than recall.
* The surface was inconsistent with comparable tools: `stellar-cli`, `cargo`,
  `gh` and `git` all group verbs under a small set of nouns
  (`git remote add`, `cargo build --release`, `gh pr list`).
* A flat namespace cannot be extended without collisions. New AI features
  needed a new `ai-*` top-level name because the obvious names (`ai audit`,
  `ai test`, `ai profile`) were already taken by *different* commands.
* `docs/COMMAND_REFERENCE.md`, the generated cheat sheet, the man pages and the
  generated shell completions all mirrored the same 86 names, so every rename
  was a multi-file, multi-artifact change.

## Decision Drivers

* Top-level `--help` must fit on a screen: **20 commands or fewer**.
* Learnability: nouns a user already thinks in (wallet, contract, deploy,
  network, template, plugin, ai, config) with verbs underneath.
* Backward compatibility: every previously working invocation must keep working
  for **one minor release**, printing a deprecation warning that names the
  replacement.
* Zero duplicated argument definitions — moving a command must not fork its
  flag surface.
* Regenerable artifacts (completions, man pages, cheat sheet) must follow the
  clap tree automatically.

## Considered Options

* **Option 1 — Keep the flat surface, only sort/group `--help`.** No migration
  cost, but does not reduce the command count, does not create room for new
  features, and cannot satisfy the acceptance criterion.
* **Option 2 — Copy the old commands into the new nouns (variant duplication).**
  Each legacy subcommand's flags would be re-declared inside the new parent
  enum, and dispatch would re-bind the payloads. Small diff at first, but every
  flag change afterwards has to be made in two places, and the payloads are
  private in some modules. Rejected: silent drift is exactly the failure mode
  this CLI already suffers from.
* **Option 3 — Group by wrapping whole legacy subtrees one level deeper**
  (`starforge contract ops invoke`, `starforge wallet keys create`). Mechanically
  trivial and never duplicates a flag, but it renames the *most* used commands
  in the tool (`starforge contract invoke` becomes three words) and buys nothing
  in discoverability. Rejected.
* **Option 4 — Adopt a noun-verb tree, keeping each legacy subtree as the body
  of its noun and adding the moved commands as new verbs on that same enum.**
  Legacy verbs keep their spelling (`starforge wallet create`,
  `starforge contract invoke`), the new commands arrive as
  `starforge wallet tx`, `starforge contract storage state`, and no argument
  definition is ever copied. Chosen.

## Decision Outcome

Chosen option: **Option 4**.

### The tree

Twelve visible top-level commands, each a noun with verbs underneath:

| Noun | Verbs |
|---|---|
| `wallet` | `create` `list` `show` `fund` `remove` `rename` `merge` `rotate` `export` `import` `import-shares` `connect` `hw-address` `hw-status` `sign` `derive` `tune-kdf` `multisig` `tx` `auth` `diagnostics` |
| `contract` | `invoke` `invoke-script` `inspect` `build` `upload` `generate-bindings` `call-graph` `deps` `version` `storage` `debug` `repl` `test` `audit` `security` `governance` `upgrade` `verify` `migrate` `generate` `complete` `explain` `lint` `optimize` `gas` `metrics` `profile` `benchmark` `docs` `mutate` `monitor` `health` |
| `deploy` | `run` `history` `env` `schedule` `orchestrate` `pipeline` `approval` `cost` `analytics` `backup` |
| `network` | `show` `switch` `add` `test` `remove` `rename` `node` `simulate` `snapshot` |
| `template` | `search` `list` `show` `install` `publish` `remove` `new` `lint` `info` `fetch` `update` `rollback` `test` `docs` `validate` `audit` `customize` `customize-history` `customize-rollback` `vcs` `registry` |
| `plugin` | `install` `list` `load` `uninstall` `verify` `audit` `update` `commands` `search` |
| `ai` | `local` `debug` `navigate` `gate` `profiling` `ide` `tests` `test-maintain` `deploy-test` `property-test` `feedback` `search` `recommend` `route` `plan` `accessibility` `suggest` `docs` `telemetry` `training` `security-audit` `prompts` `help` |
| `config` | `show` `set` `set-encryption` `doctor` `db` `info` `telemetry` `flags` `privacy` |
| `project` | `new` `collab` `task` `progress` `sprint` `resource` `risk` `timeline` … |
| `tool` | `tutorial` `nl` `pr` `bug-report` |
| `completions` | `bash` `zsh` `fish` `powershell` |
| `man` | `generate` `install` `list` |

`autocomplete` and plugin-provided external commands remain registered but
hidden, exactly as before. The feature-gated `ui` command is untouched: it is
still listed by `--help` when built with `--features ui`, which the help test
accounts for, and it still counts against the 20-command budget.

Three implementation rules follow from Option 4:

1. **A noun that already had a body keeps it.** `wallet`, `contract`, `network`,
   `template`, `config` and `project` gained new variants in their existing
   `src/commands/*.rs` enums; `ai`, `deploy` and `tool` are new enums in
   `src/commands/tree.rs` because their old top-level form was a leaf argument
   struct (`deploy --wasm …`) or a single subtree (`ai status`).
2. **Moved commands are never renamed twice.** `starforge inspect state …`
   becomes `starforge contract storage state …`; `starforge audit` becomes
   `starforge contract audit`; `starforge simulate` becomes
   `starforge network simulate`.
3. **Ambiguity is resolved by renaming the new verb, not the old one.** The old
   `starforge ai <verb>` verbs (`status`, `models`, `pull`, `ask`, `audit`,
   `test`, `profile`, `optimise`, …) live under `starforge ai local`, so the
   new subtrees could take the clearer names `ai tests`, `ai profiling` and
   `ai security-audit` without stealing a legacy spelling.

### Deprecation strategy

`src/commands/deprecations.rs` holds the single `DEPRECATED_COMMANDS` table
(old top-level token → new path). Before `Cli::parse_from`, `main` normalises
argv: a leading deprecated token is rewritten to its new path and a warning
naming both spellings is written to **stderr** (never stdout, so `--json`,
`completions` and piping stay clean).

This is deliberately an argv shim rather than a clap `#[command(alias)]`:

* `alias` cannot express `starforge deploy --wasm x` → `starforge deploy run
  --wasm x`, because the alias and the new noun would both want the name
  `deploy` at the top level.
* Aliases would leak the old spellings into generated completions, which is
  precisely what this ADR is removing.
* The shim is a single, testable, table-driven function, so the migration table
  in `docs/CLI_COMMAND_TREE.md` and the code cannot drift apart.

The shim is scheduled for removal in the next minor release; the table is the
single place to delete.

### Positive Consequences

* `starforge --help` lists 12 commands instead of 86.
* New AI features can be added as `ai <verb>` without inventing `ai-*` names.
* No flag definition is duplicated, so there is nothing to drift.
* Completions, man pages and the cheat sheet are regenerated from clap
  metadata by `build.rs`, so the artifacts follow the tree automatically.

### Negative Consequences / Trade-offs

* One more level of nesting for moved commands (`starforge contract storage
  state`). *Mitigation*: the migration table, and the deprecation warning that
  prints the exact replacement command.
* `starforge ai` changed shape (`ai pull` → `ai local pull`). *Mitigation*: the
  argv shim covers the legacy verbs, which is why the new subtrees were renamed
  to `ai tests` / `ai profiling` / `ai security-audit` instead of colliding.
* `telemetry` event names and contextual-help keys change to the noun. *Mitigation*:
  telemetry is opt-in and versioned per release; the new names are the noun
  alone, so the metric cardinality does not grow.

## Pros and Cons of the Options

### Option 1 — Sort/group help only

* Good, because it is a one-file change.
* Bad, because the command count is unchanged and the namespace stays
  un-extendable; fails the acceptance criterion.

### Option 2 — Duplicate variants into the new nouns

* Good, because each noun's `handle` is a flat, obvious match.
* Bad, because flags must be edited twice, and some payloads are private in
  their defining module.

### Option 3 — Wrap legacy subtrees one level deeper

* Good, because nothing is duplicated.
* Bad, because it renames the highest-traffic commands and does not improve
  discoverability.

### Option 4 — Noun-verb tree, legacy subtree becomes the noun's body (chosen)

* Good, because legacy verbs keep their spelling and no flag is duplicated.
* Good, because the top-level surface is 12 commands and new features get room.
* Bad, because moved commands gain a level of nesting, and the argv shim is
  bespoke machinery that must be deleted later.

## Links and References

* Issue [#936](https://github.com/Nanle-code/StarForge/issues/936)
* Migration table: [`docs/CLI_COMMAND_TREE.md`](../CLI_COMMAND_TREE.md)
* Command reference: [`docs/COMMAND_REFERENCE.md`](../COMMAND_REFERENCE.md)
* Generated cheat sheet: [`docs/COMMAND_CHEATSHEET.md`](../COMMAND_CHEATSHEET.md)
* Deprecation table source: `src/commands/deprecations.rs`
* Command tree source: `src/commands/tree.rs`
