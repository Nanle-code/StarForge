# StarForge composite action

Install [StarForge](https://github.com/Nanle-code/StarForge) and run build, test,
deploy or verify in a single step. The action installs a **pinned, checksum-verified
release**, caches the binary between runs, can import a testnet wallet from a
repository secret, and exposes the deployed contract ID as an output.

## Usage

```yaml
name: Soroban CI
on: [push, pull_request]

jobs:
  contract:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32v1-none

      # Build the contract however your project prefers, then let StarForge test it.
      - name: Build contract
        run: cargo build --release --target wasm32v1-none

      - name: Test with StarForge
        uses: Nanle-code/StarForge/.github/actions/starforge@v0.1.0
        with:
          command: test --wasm target/wasm32v1-none/release/token.wasm --coverage
```

### PR testnet preview deploy

```yaml
name: Preview deploy
on:
  pull_request:

jobs:
  preview:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32v1-none
      - run: cargo build --release --target wasm32v1-none

      - name: Deploy preview to testnet
        id: deploy
        uses: Nanle-code/StarForge/.github/actions/starforge@v0.1.0
        with:
          network: testnet
          wallet: ${{ secrets.STARFORGE_TESTNET_SECRET }}
          skip-confirmation: "true"
          command: >-
            deploy --wasm target/wasm32v1-none/release/token.wasm
            --wallet ci-wallet --network testnet --optimize --execute --yes

      - name: Comment the preview contract ID
        uses: actions/github-script@v7
        with:
          script: |
            github.rest.issues.createComment({
              ...context.repo,
              issue_number: context.issue.number,
              body: `Preview deployed: \`${{ steps.deploy.outputs.contract-id }}\``,
            })
```

## Inputs

| Input | Default | Description |
| --- | --- | --- |
| `command` | `--version` | StarForge command and flags to run. |
| `network` | `testnet` | Target network; exported as `STARFORGE_NETWORK`. |
| `wallet` | *(none)* | Raw Stellar secret key (`S...`); imported as a wallet. Pass a repository secret. |
| `wallet-name` | `ci-wallet` | Name the imported wallet is stored under. |
| `version` | `0.1.0` | Release to install in `release` mode, or `latest`. |
| `install-mode` | `release` | `release` downloads + verifies a signed release; `source` builds the checkout. |
| `working-directory` | `.` | Directory the command runs from. |
| `skip-confirmation` | `false` | Export `STARFORGE_UNSAFE_SKIP_CONFIRMATION=1` for non-interactive runs. |

## Outputs

| Output | Description |
| --- | --- |
| `contract-id` | Contract ID (`C...`) parsed from the command output, when present. |
| `wasm-hash` | 64-character WASM hash parsed from the command output, when present. |
| `exit-code` | Exit code of the StarForge command. |

## Security notes

- Only release archives from the canonical
  `Nanle-code/StarForge` repository are downloaded, and the SHA-256 checksum is
  verified against the release's `SHA256SUMS.txt` before the binary is used.
- Wallet secrets are masked in the logs and passed to
  `starforge wallet import --key` through the environment, never as a command
  argument on the process command line of the download step.
- Prefer `skip-confirmation: "false"` (the default) except for disposable
  testnet previews.

See [`docs/GITHUB_ACTION.md`](../../../docs/GITHUB_ACTION.md) for the full guide.
