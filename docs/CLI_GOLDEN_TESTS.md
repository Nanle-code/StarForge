# Golden CLI output tests

StarForge exposes ~85 top-level commands, so an accidental change to help
text, success output, or error output is easy to miss in review. The golden CLI
tests make every visible output change an explicit diff in the pull request.

- Harness: [`tests/cli_golden.rs`](../tests/cli_golden.rs)
- Corpus: [`tests/cmd/`](../tests/cmd)
- Regeneration script: [`scripts/gen-cli-snapshots.sh`](../scripts/gen-cli-snapshots.sh)

## Why not `trycmd` / `snapbox`?

The upstream issue suggested `trycmd` (or `snapbox`). Those crates are **not**
used here on purpose: `Cargo.lock` is checked in and CI asserts
`git diff --exit-code Cargo.lock`, so adding a crate that is not already in the
lockfile would fail CI. `tests/cli_golden.rs` is a small re-implementation of
the same workflow — same `TRYCMD=overwrite` UX — built on `std` plus
dependencies the crate already has (`toml`, `serde`, `tempfile`).

## Corpus layout

```
tests/cmd/<case>.toml     # case definition: args, optional stdin/env/timeout
tests/cmd/<case>.stdout   # expected stdout
tests/cmd/<case>.stderr   # expected stderr
tests/cmd/<case>.code     # expected exit code, e.g. "0\n"
```

Example case file:

```toml
description = "starforge wallet --help"
args = ["wallet", "--help"]

# Optional keys:
# stdin = "y\n"
# timeout_secs = 30
# [env]
# NO_COLOR = "0"
```

Run just this suite with:

```bash
cargo test --locked --test cli_golden
```

## Refreshing snapshots (`TRYCMD=overwrite`)

When a change to CLI output is intentional, rewrite the affected snapshots:

```bash
TRYCMD=overwrite cargo test --locked --test cli_golden
# or:
bash scripts/gen-cli-snapshots.sh
```

`GOLDEN_OVERWRITE=1` and `UPDATE_SNAPSHOTS=1` are accepted as equivalents, for
consistency with `tests/bindings_snapshots.rs`. Then review the diff and commit
it with the code change:

```bash
git diff -- tests/cmd
```

## Missing snapshots are generated, not invented

A case whose snapshot file does not exist yet is **generated and the case
passes**, with a notice on stderr:

```
[cli_golden] generated missing snapshot .../tests/cmd/help_wallet.stdout (review and commit it, ...)
```

This keeps CI green on a checkout that contains case definitions but not yet
their snapshots. It also means a freshly added `tests/cmd/<case>.toml` does not
turn red until the snapshot has been committed.

> **Committed vs generated.** Only snapshots whose exact bytes are known without
> executing the binary are committed (currently `--version` and `--help-all`,
> both derived from `Cargo.toml` / `src/main.rs`). The rest of the corpus —
> including the full `--help` output for every top-level command — is populated
> the first time the suite runs. Maintainers should run
> `TRYCMD=overwrite cargo test --test cli_golden` and commit `tests/cmd/` to
> freeze the full corpus in the repository.

## Determinism

Each invocation runs with:

- an isolated HOME/config tree (`HOME`, `USERPROFILE`, `XDG_*`, `APPDATA`, and
  `STARFORGE_CONFIG_DIR` all point inside a fresh temp dir);
- a scrubbed environment (inherited `STARFORGE_*`, `COLUMNS`, and colour toggles
  are removed), so a developer's shell cannot leak into a snapshot;
- `NO_COLOR=1`, `TERM=dumb`, `LANG=C`, a closed stdin, and a hard timeout;
- the crate root as the working directory, so bundled paths such as
  `templates/registry.json` resolve.

Captured output is normalized before writing/comparing: CRLF becomes LF, ANSI
escape sequences are stripped, and the isolated temp paths are replaced with
`$HOME`. Cases must therefore be read-only with respect to the repository.

## Adding a case

1. Add `tests/cmd/<case>.toml` with the `args` to run.
2. Run `TRYCMD=overwrite cargo test --locked --test cli_golden`.
3. Review the generated `.stdout` / `.stderr` / `.code` files (they are the
   golden output) and commit everything together.
