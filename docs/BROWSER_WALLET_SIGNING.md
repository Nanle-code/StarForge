# Browser wallet signing (`--signer browser`)

`starforge wallet sign-tx --signer browser` signs a transaction envelope XDR
with a browser wallet (Freighter, xBull, Rabet, …) instead of importing a
secret key into the CLI. The CLI starts a one-time HTTP server on
`127.0.0.1`, opens a page that uses
[Stellar Wallets Kit](https://github.com/Creit-Tech/Stellar-Wallets-Kit), shows
a decoded view of the transaction, and returns the signed XDR to the CLI.

## Prerequisites

- A Stellar Wallets Kit compatible browser wallet installed (this checklist
  uses **Freighter**).
- The wallet switched to **Testnet** and holding a funded testnet account
  (fund it through Friendbot: `starforge wallet fund <name>` or
  <https://friendbot.stellar.org>).
- An unsigned, base64-encoded transaction envelope XDR in a file. Any source
  works (for example a multi-sig setup payload or a transaction exported from
  another tool).
- A machine with a browser that can reach `http://127.0.0.1:<port>` (a local
  desktop session; the port is printed to the terminal and the CLI tries to
  open the page for you).

## Command

```bash
starforge wallet sign-tx \
  --transaction unsigned.xdr \
  --signer browser \
  --network testnet \
  --timeout 120 \
  --output signed.xdr
```

- `--transaction` — file containing the base64 transaction envelope XDR.
- `--signer browser` — use the localhost handoff (the default).
- `--signer local --wallet <name>` — sign with a locally stored secret key.
- `--network` — `testnet` or `mainnet`; the passphrase is resolved from config.
- `--timeout` — seconds to wait for the wallet (default `120`).
- `--output` — file to write the signed XDR to; omit it to print to stdout.

## Manual test checklist (Freighter on testnet)

Run the command above, then verify each item. Tick a box only when the checkbox
was observed on a real run.

- [ ] The CLI prints a `http://127.0.0.1:<port>/?nonce=<hex>` URL and opens it
      in the default browser (or tells you to open it manually).
- [ ] The page shows the network (`testnet`), the envelope type, the byte
      length, and a SHA-256 digest for the unsigned transaction.
- [ ] The "Decoded transaction" panel renders the transaction summary.
- [ ] **Browser check:** open the page with the nonce removed or changed
      (`http://127.0.0.1:<port>/` and `...?nonce=deadbeef`); both are rejected
      (HTTP 400 / 403) and no signing UI can be used.
- [ ] **CSP check:** the browser devtools console shows no CSP violations when
      the page loads, and the network response for `/` carries a
      `Content-Security-Policy` header starting with `default-src 'none'`.
- [ ] Clicking **Sign with browser wallet** opens Freighter, which shows the
      same transaction and network as the page.
- [ ] Approving in Freighter returns the signed XDR to the CLI; the terminal
      exits successfully and, when `--output` is set, the file contains the
      signed XDR.
- [ ] Reloading the page after signing and trying to submit again is rejected
      (the nonce was consumed, HTTP 410).
- [ ] Running the command again with a fresh invocation prints a **new** nonce
      and a **new** port.
- [ ] Letting the command sit without approving until `--timeout` elapses makes
      the CLI exit with a timeout error, and the port is no longer listening.
- [ ] Rejecting the request in Freighter leaves the CLI waiting until the
      timeout, and no `signed.xdr` is written.
- [ ] `--network mainnet` shows the mainnet passphrase on the page and in
      Freighter, and the CLI never submits the transaction itself.

### Expected results

- Success: the signed XDR written to `--output` differs from the input XDR and
  decodes as base64. Submitting it is a separate, explicit step.
- Timeout: `browser handoff timed out after <n>s; no signature was returned`,
  with a non-zero exit status.
- Bad payload: if the wallet returns something that is not base64, the CLI
  reports `browser wallet returned an invalid signature: …` and does not write
  output.

## Troubleshooting

- **The browser did not open.** Copy the printed URL into a browser manually.
  The server is only reachable from the same machine.
- **"handoff rejected the page: HTTP 400/403".** The URL lost its `?nonce=…`
  parameter, or it is from an expired earlier run.
- **Nothing happens after clicking Sign.** Confirm the wallet extension is
  installed, unlocked, and set to Testnet, then check the browser console for
  wallet errors.
- **The page fails to load Stellar Wallets Kit.** The page imports a pinned
  bundle from `https://cdn.jsdelivr.net`; a restrictive network or ad blocker
  can block it. This is the only external resource the page loads.

## Security notes

See `docs/SECURITY_THREAT_MODEL.md` → "Browser-wallet localhost handoff" for the
threats and mitigations: loopback-only bind, a single-use 32-byte nonce, a
strict `Content-Security-Policy` scoped to a script nonce, a short timeout, and
base64 validation of the returned envelope XDR. The CLI never reads a secret
key for `--signer browser`.
