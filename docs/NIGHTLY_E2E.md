# Nightly end-to-end suite (real testnet)

Most StarForge tests use fixtures and mocks. Real network behaviour — fees,
archival, protocol upgrades, Friendbot, RPC drift — only shows up on a live
network, so a scheduled workflow exercises the real Stellar **testnet** every
night:

- workflow: [`.github/workflows/nightly-e2e.yml`](../.github/workflows/nightly-e2e.yml)
- suite: [`.github/scripts/nightly-e2e.sh`](../.github/scripts/nightly-e2e.sh)

Each run:

1. creates an **ephemeral** keypair (`stellar keys generate`), imports it into
   StarForge (`starforge wallet import --from-stellar-cli`) and funds it with
   **Friendbot** (`starforge wallet fund`);
2. for every built-in template (`hello-world`, `token`, `voting`, `nft`) runs
   scaffold → build → deploy → invoke → upgrade → verify against testnet;
3. writes a per-phase summary to `e2e-results/summary.md` plus one log per phase;
4. on failure, opens or updates a **single** tracking issue titled
   `[nightly-e2e] End-to-end suite failing against testnet` (label `nightly-e2e`).

Keys are generated per run, removed on exit, and the results directory is
scrubbed for secret-looking strings before it is uploaded. Nothing ever prints
a secret key.

## Running it locally

Prerequisites:

```bash norun
# 1. Build StarForge (the workflow does `cargo build --release --locked`).
cargo build --release --locked

# 2. Install the Stellar CLI (the suite signs, deploys and uploads through it).
curl -fsSL https://raw.githubusercontent.com/stellar/stellar-cli/main/install.sh | sh
# Installs to /usr/local/bin with sudo, otherwise to ~/.local/bin:
export PATH="$PATH:$HOME/.local/bin"

# 3. Contract build target.
rustup target add wasm32-unknown-unknown wasm32v1-none
```

Then run the suite exactly as CI does:

```bash norun
STARFORGE_BIN="$PWD/target/release/starforge" \
  bash ./.github/scripts/nightly-e2e.sh
```

Useful overrides (all optional):

| Variable | Default | Purpose |
| --- | --- | --- |
| `STARFORGE_BIN` | `target/release/starforge`, then `target/debug/starforge`, then `PATH` | StarForge binary to exercise |
| `STARFORGE_E2E_NETWORK` | `testnet` | Only `testnet` is supported (Friendbot funding) |
| `STARFORGE_E2E_TEMPLATES` | `hello-world,token,voting,nft` | Comma-separated built-in templates |
| `STARFORGE_E2E_MAX_SECONDS` | `720` | Wall-clock budget for the suite |
| `STARFORGE_E2E_CMD_TIMEOUT` | `300` | Per-command timeout |
| `STARFORGE_E2E_UPGRADE_INVOKE` | `1` | Set `0` to prepare/upload an upgrade without invoking it |
| `STARFORGE_E2E_KEEP_KEYS` | `0` | Keep the ephemeral keys (debugging only) |
| `STARFORGE_E2E_RESULTS_DIR` | `./e2e-results` | Where the summary and logs are written |

A single template, quickly:

```bash norun
STARFORGE_BIN="$PWD/target/release/starforge" \
STARFORGE_E2E_TEMPLATES=hello-world \
  bash ./.github/scripts/nightly-e2e.sh
cat e2e-results/summary.md
```

The suite exits non-zero on real failures (a failed deploy, invoke, upgrade
preparation or verification). A template that cannot be scaffolded or built is
reported as `SKIPPED` and, on its own, does not fail the run — but if **every**
configured template is skipped the run fails, because then the suite exercised
nothing.

## Adjusting the schedule

The cron lives in `.github/workflows/nightly-e2e.yml`:

```yaml
on:
  schedule:
    - cron: '17 3 * * *'   # nightly at 03:17 UTC
  workflow_dispatch: {}
```

Cron is always **UTC**. It is deliberately `:17` rather than `:00` because
GitHub's scheduler is busiest on the hour and delays scheduled runs there. Edit
that one line to change the cadence (for example `'0 */6 * * *'` for every six
hours, or `'17 3 * * 1'` for Mondays only). You can also trigger a run by hand
from **Actions → Nightly End-to-End (Testnet) → Run workflow**, optionally
overriding the template list and keeping the keys for debugging.

The job has a hard `timeout-minutes: 20` and a concurrency group
(`nightly-e2e-${{ github.ref }}`, `cancel-in-progress: true`), so a new run
supersedes an old one instead of piling up.

## Tracking failures

When the suite fails, the workflow files exactly one issue (searching open
issues by title, then updating it in place) with the run link and the suite
summary. Fix the failure and the next green run leaves the issue open for a
human to close; a later failure appends a comment rather than opening a new
issue.

## Notes and limitations

- The built-in templates do not currently export an `upgrade` entrypoint, so
  the upgrade phase validates the new wasm and uploads it to the ledger
  (`starforge upgrade prepare` + `stellar contract upload`) and reports the
  on-chain invocation as `SKIPPED`. Templates that do export `upgrade` are
  invoked automatically.
- The suite signs, deploys and uploads through the Stellar CLI because
  StarForge delegates those operations to it (`stellar` is used by
  `starforge deploy --execute`, `contract upload`, and the upgrade flow).
- Only the testnet network is supported, since funding goes through Friendbot.
