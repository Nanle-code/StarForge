# StarForge GitHub Action

`starforge-action` is a composite GitHub Action that installs StarForge into a
workflow and runs a build, test, deploy or verify step. It replaces hand-written
download/caching steps with one pinned, checksum-verified action.

The action lives in this repository at
[`.github/actions/starforge`](https://github.com/Nanle-code/StarForge/tree/master/.github/actions/starforge).

## What it does

1. Resolves the runner OS/architecture and the requested StarForge version.
2. Restores a cached StarForge binary, keyed by OS, architecture and version.
3. On a cache miss, downloads `starforge-<os>-<arch>.tar.gz` **and**
   `SHA256SUMS.txt` from the canonical release and verifies the checksum before
   installing.
4. Optionally imports a wallet from a repository secret and selects the network.
5. Runs your `command` and exposes the parsed contract ID, WASM hash and exit
   code as outputs.

`install-mode: source` skips the download and builds the checked-out repository
instead; StarForge's own CI uses this so the action can be validated without
depending on a published release artifact.

## Quick start

```yaml
- uses: actions/checkout@v4
- uses: dtolnay/rust-toolchain@stable
  with:
    targets: wasm32v1-none
- run: cargo build --release --target wasm32v1-none

- uses: Nanle-code/StarForge/.github/actions/starforge@v0.1.0
  with:
    command: test --wasm target/wasm32v1-none/release/token.wasm
```

Pin the action to a released tag (`@v0.1.0`) rather than a moving branch so CI
reproducibility matches the pinned StarForge version.

## PR testnet preview deploys

A common pattern is to deploy every pull request to testnet and comment the
contract ID on the PR:

```yaml
name: Preview deploy
on: pull_request

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

Store the testnet secret as the `STARFORGE_TESTNET_SECRET` repository secret.
The action masks it in the logs; never commit it to the repository.

## Inputs

| Input | Default | Description |
| --- | --- | --- |
| `command` | `--version` | StarForge command and flags to run. |
| `network` | `testnet` | Target network, exported as `STARFORGE_NETWORK`. |
| `wallet` | *(none)* | Raw Stellar secret key (`S...`) imported as a wallet. |
| `wallet-name` | `ci-wallet` | Name the imported wallet is stored under. |
| `version` | `0.1.0` | Release to install in `release` mode, or `latest`. |
| `install-mode` | `release` | `release` or `source`. |
| `working-directory` | `.` | Directory the command runs from. |
| `skip-confirmation` | `false` | Set `true` to bypass interactive confirmations. |

## Outputs

| Output | Description |
| --- | --- |
| `contract-id` | Contract ID (`C...`) parsed from the command output, when present. |
| `wasm-hash` | 64-character WASM hash parsed from the command output, when present. |
| `exit-code` | Exit code of the StarForge command. |

## Caching and pinning

The cache key is `starforge-<os>-<arch>-<version>`, so bumping `version`
invalidates the cache and re-downloads (and re-verifies) the pinned release.
Combining a pinned action tag with a pinned `version` input gives reproducible
install behaviour across runs.

## Related documentation

- [Installing StarForge](INSTALL.md) — manual install, Homebrew and build from source.
- [Command reference](COMMAND_REFERENCE.md) — commands and flags the `command`
  input accepts.
- [Deploy policy](DEPLOY_POLICY.md) — gating networks and reviewers in CI.
