# Migrating from stellar-cli

Most Soroban developers already use [stellar-cli](https://developers.stellar.org/docs/tools/cli)
(`stellar`). This guide explains how StarForge fits next to it: which commands
map to which, how to bring your existing identities over, and what stellar-cli
still does better.

**Summary:** StarForge doesn't replace stellar-cli, it sits on top of it.
StarForge adds project scaffolding and templates, encrypted local wallets,
deployment planning, history and policy checks, scripted invocations and
stable JSON output. stellar-cli still compiles contracts, signs the final
deploy transaction, and covers the low-level transaction and XDR tooling.
Keep both installed.

## When to use which

| You want to… | Use |
|---|---|
| Start a contract from a template (token, NFT, voting, marketplace) | StarForge |
| Keep secret keys encrypted at rest, back them up, split them into recovery shares, use a Ledger/Trezor | StarForge |
| Preview a deployment (size, balance, simulated fees, auth) before sending anything | StarForge (`deploy --dry-run`) |
| Track deployment history, roll back, enforce deploy policies in CI | StarForge |
| Run a repeatable sequence of contract calls from YAML with assertions | StarForge (`contract invoke-script`) |
| Compile a contract to `.wasm` | stellar-cli (`stellar contract build`) |
| Invoke contract functions and read their results | stellar-cli (see [what StarForge doesn't do](#what-stellar-cli-does-that-starforge-doesnt)) |
| Build, sign or inspect raw transactions and XDR | stellar-cli |

## Command mapping

`—` means StarForge has no equivalent. Keep using the stellar-cli command.

### Identities and wallets

| stellar-cli | StarForge | Notes |
|---|---|---|
| `stellar keys generate alice` | `starforge wallet create alice` | Add `--mnemonic` for a BIP39 phrase, `--encrypt` to encrypt the secret at rest. |
| `stellar keys generate alice --fund` | `starforge wallet create alice --fund` | Friendbot on testnet. |
| `stellar keys add alice` (secret or phrase) | `starforge wallet import alice --key S…` / `--mnemonic` | |
| *existing stellar-cli identity* | `starforge wallet import --from-stellar-cli alice` | See [Importing identities](#importing-stellar-cli-identities). |
| `stellar keys ls` | `starforge wallet list` | |
| `stellar keys address alice` | `starforge wallet show alice` | Also shows the live balance. |
| `stellar keys secret alice` | `starforge wallet show alice --reveal` | |
| `stellar keys fund alice` | `starforge wallet fund alice` | |
| `stellar keys rm alice` | `starforge wallet remove alice` | |
| `stellar keys use alice` | — | Pass `--wallet alice` to each command. |
| `stellar keys add --ledger` | `starforge wallet import alice --hardware ledger` | Also supports Trezor. |
| `stellar message sign` | `starforge wallet sign` | Not documented as SEP-53, so check interoperability before relying on it. |

### Networks

| stellar-cli | StarForge | Notes |
|---|---|---|
| `stellar network ls` | `starforge network show` | |
| `stellar network use testnet` | `starforge network switch testnet` | |
| `stellar network add NAME --rpc-url … --network-passphrase …` | `starforge network add NAME --horizon-url … --soroban-rpc-url … --passphrase …` | StarForge also needs a Horizon URL. |
| `stellar network rm NAME` | `starforge network remove NAME` | |
| `stellar network health` | `starforge network test` | |
| `stellar container start local` | `starforge node start` | Both run the `stellar/quickstart` image in Docker. StarForge names the network `docker-testnet`. |

### Contracts

| stellar-cli | StarForge | Notes |
|---|---|---|
| `stellar contract init NAME` | `starforge new contract NAME [--template …]` | Built-in `hello-world`, `token`, `nft`, `voting`, plus marketplace templates. |
| `stellar contract build` | — | Keep using stellar-cli (or `cargo build --target wasm32v1-none --release`). |
| `stellar contract build --optimize` | `starforge optimize` / `starforge deploy --optimize` | |
| `stellar contract upload --wasm …` | `starforge contract upload --wasm … --wallet NAME` | Runs `stellar contract upload` under the hood. |
| `stellar contract deploy --wasm … --source NAME` | `starforge deploy --wasm … --wallet NAME --execute` | StarForge validates the WASM, checks the balance and records history, then runs `stellar contract deploy`. Without `--execute` it only prints the plan. |
| `stellar contract invoke --id C… -- fn --arg x` | `starforge contract invoke C… fn --arg x --type symbol` | Incomplete: keep using stellar-cli (see [below](#what-stellar-cli-does-that-starforge-doesnt)). `starforge contract invoke-script` validates YAML call plans. |
| `stellar contract info interface --wasm …` | `starforge contract inspect C…` | StarForge inspects a *deployed* instance. |
| `stellar contract bindings rust\|typescript\|python` | `starforge contract generate-bindings … --lang rust\|ts\|python\|go` | stellar-cli also has Java, Flutter, Swift and PHP. |
| `stellar contract read` | `starforge inspect` | Storage inspection. |
| `stellar events` | `starforge monitor` | |
| `stellar completion --shell bash` | `starforge completions bash` | |

Run `starforge <command> --help` for every flag.

## Importing stellar-cli identities

`starforge wallet import --from-stellar-cli <identity>` reads the identity file
that `stellar keys generate` / `stellar keys add` wrote, and saves it as a
StarForge wallet **with the same name**. Keeping the names the same matters:
`starforge deploy --execute` and `starforge contract upload` hand signing to
stellar-cli using the wallet name, so the pair has to match.

```bash norun
stellar keys generate alice --fund                 # or an identity you already have
starforge wallet import --from-stellar-cli alice   # same name on both sides
starforge wallet import --from-stellar-cli alice --encrypt   # encrypt it at rest in StarForge

starforge wallet import ops --from-stellar-cli alice         # different local name
```

Where StarForge looks, in order (the first match wins):

1. `./.stellar/identity/<name>.toml`: project-local identities.
2. `$STELLAR_CONFIG_HOME/identity/<name>.toml` if set. Otherwise
   `$XDG_CONFIG_HOME/stellar/identity/` and `~/.config/stellar/identity/`.
3. The legacy `soroban` directories: `./.soroban/identity/` and
   `~/.config/soroban/identity/`.

What can be imported:

| Identity kind | Imported? |
|---|---|
| `secret_key = "S…"` | ✅ |
| `seed_phrase = "…"` (the default for `stellar keys generate`) | ✅ Derives account index 0, like stellar-cli. Use `--account-index N` for another. |
| Public key only (`stellar keys add --public-key`) | ❌ There's nothing to sign with. |
| OS keychain (`--secure-store`) | ❌ StarForge can't read the keychain. Export the secret and use `wallet import --key`. |
| Ledger | ❌ Use `starforge wallet import NAME --hardware ledger`. |

The import leaves the stellar-cli identity untouched and never prints the
secret. [`tests/stellar_cli_identity_import.rs`](https://github.com/Nanle-code/StarForge/blob/master/tests/stellar_cli_identity_import.rs)
tests it end to end.

### Going the other way

To make a StarForge-created wallet usable by stellar-cli (needed for
`deploy --execute`), add it to stellar-cli under the same name:

```bash norun
starforge wallet show alice --reveal   # copy the S… secret
stellar keys add alice --secret-key    # paste it at the prompt
```

## Configuration side by side

The two tools keep separate state and don't interfere:

| | stellar-cli | StarForge |
|---|---|---|
| Location | `~/.config/stellar/` (or `.stellar/` in a project) | `~/.starforge/` |
| Identities / wallets | one TOML file per identity, plaintext unless secure-store | SQLite `starforge.db`; secrets optionally encrypted with Argon2id + AES-256-GCM |
| Networks | `network/<name>.toml` | `starforge network add` |
| Default network | `stellar network use` / `STELLAR_NETWORK` | `starforge network switch` |

A local standalone network (for example from `stellar container start local`)
can be added to StarForge like this:

```bash run
starforge network add local \
  --horizon-url http://localhost:8000 \
  --soroban-rpc-url http://localhost:8000/soroban/rpc \
  --friendbot-url http://localhost:8000/friendbot \
  --passphrase "Standalone Network ; February 2017"
starforge network show
```

With the node running, the usual wallet workflow works against it:

```bash run local
starforge network switch local
starforge wallet create dev --fund     # funded by the local Friendbot
starforge wallet show dev              # live balance from the local Horizon
starforge network test local
```

## What stellar-cli does that StarForge doesn't

StarForge is young and moves fast. This is the honest list as of this
writing. Check `starforge <command> --help` for anything that has changed.

- **Building contracts.** StarForge has no `build` command. Use
  `stellar contract build`.
- **Signing deploys itself.** `starforge deploy --execute` runs
  `stellar contract deploy` for the final step, so stellar-cli must be
  installed and hold an identity with the wallet's name. Constructor arguments
  (`stellar contract deploy … -- --arg …`) can't be passed through yet.
- **Invoking contracts.** `starforge contract invoke` runs a simulation, but
  it doesn't yet decode the contract's real return value (it shows `void`),
  and `--submit` doesn't sign with local wallets yet. Use
  `stellar contract invoke` for any call whose result or effect matters.
- **Custom networks for deploy and invoke.** `starforge deploy` and
  `starforge contract invoke` accept only `--network testnet|mainnet`. For a
  local or custom network, use stellar-cli.
- **Low-level transaction tooling.** `stellar tx new/sign/simulate/send/edit`,
  `stellar xdr`, `stellar strkey`, `stellar ledger`, `stellar snapshot` and
  `stellar cache` have no StarForge equivalent. (`starforge tx` covers
  payments, batches, history and fee stats.)
- **State maintenance.** `stellar contract extend`, `restore`, `fetch`, `id`,
  `alias` has a StarForge equivalent (`starforge alias`). Stellar Asset Contracts
  use `starforge asset contract-id` / `starforge asset wrap`.
- **Default identity.** There's no `keys use`: pass `--wallet` explicitly.
- **More binding languages.** stellar-cli also generates Java, Flutter, Swift
  and PHP clients.

## A combined workflow

```bash norun
starforge new contract hello                 # scaffold (StarForge)
cd hello && stellar contract build           # compile (stellar-cli)

stellar keys generate deployer               # identity (stellar-cli)
starforge wallet import --from-stellar-cli deployer   # same wallet in StarForge
starforge wallet fund deployer               # Friendbot (testnet)

WASM=target/wasm32v1-none/release/hello.wasm
starforge deploy --wasm "$WASM" --wallet deployer --dry-run    # plan + fees
starforge deploy --wasm "$WASM" --wallet deployer --yes --execute
stellar contract invoke --id <CONTRACT_ID> --source deployer --network testnet \
  -- hello --to Stellar                                  # call it (stellar-cli)
```
