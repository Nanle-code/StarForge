# Signing with browser wallets

`starforge` normally signs with a key stored locally (encrypted or not). That is
convenient but it means a funded secret has to live on the machine running the
CLI. If your keys are already in Freighter, xBull, Rabet, or another
[Stellar Wallets Kit](https://github.com/Creit-Tech/Stellar-Wallets-Kit)
compatible wallet, you can keep them there and still sign with the CLI.

```bash
starforge wallet sign-tx --transaction unsigned.xdr --signer browser
```

## How the localhost handoff works

`--signer browser` never loads a secret key into the CLI:

1. The CLI reads the base64 transaction envelope XDR from `--transaction`.
2. It binds a one-time HTTP server to `127.0.0.1` on an ephemeral port, generates
   a 32-byte nonce from the OS RNG, and opens
   `http://127.0.0.1:<port>/?nonce=<nonce>` in your browser.
3. The page renders the unsigned envelope, its decoded summary, and its SHA-256
   digest so you can compare it with what your wallet displays.
4. Pressing **Sign with browser wallet** loads the pinned Stellar Wallets Kit
   bundle and asks your wallet to sign the envelope against the network
   passphrase.
5. The signed XDR is POSTed back to the same server. The server validates that
   it is a non-empty base64 envelope, consumes the nonce, and returns the signed
   XDR to the CLI.
6. The CLI writes the signed XDR to `--output`, or prints it to stdout.

The server stops listening as soon as a signature arrives or after `--timeout`
seconds (default `120`). The nonce is single-use: a replay gets HTTP `410`, and
a malformed payload is rejected without burning the nonce.

### Flags

| Flag | Meaning |
| --- | --- |
| `--transaction <PATH>` | File containing the base64 transaction envelope XDR (required). |
| `--signer <browser\|local>` | `browser` for the handoff, `local` for a stored key (default `browser`). |
| `--wallet <NAME>` | Wallet name; required when `--signer local`. |
| `--network <testnet\|mainnet>` | Network passphrase to sign against (default `testnet`). |
| `--output <PATH>` | Write the signed XDR here instead of stdout. |
| `--timeout <SECONDS>` | How long to wait for the wallet (default `120`). |

## Manual test checklist (Freighter on testnet)

Run this once against a real wallet before relying on the command. Everything
below uses testnet only.

- [ ] Install the [Freighter extension](https://www.freighter.app/) and switch it
      to **Testnet**.
- [ ] Create or import a funded testnet account in Freighter and copy its public
      key.
- [ ] Build an unsigned transaction envelope for that account, for example with
      a payment built by your own flow, and save the base64 XDR to
      `unsigned.xdr`.
- [ ] Run:

      starforge wallet sign-tx --transaction unsigned.xdr --signer browser --network testnet --timeout 120

- [ ] Confirm the CLI prints a `http://127.0.0.1:<port>/?nonce=<nonce>` URL and
      that your browser opens it.
- [ ] Confirm the page shows the network, the envelope type, the byte length,
      the SHA-256 digest, the decoded summary, and the raw XDR.
- [ ] Confirm the page's digest and byte length match the local envelope (for
      example `base64 -d unsigned.xdr | shasum -a 256`).
- [ ] Click **Sign with browser wallet**, approve in Freighter, and check that
      the page reports the signature was returned to the CLI.
- [ ] Confirm the CLI prints the signed XDR (or writes `--output`).
- [ ] Submit the signed XDR to testnet Horizon and confirm it succeeds.
- [ ] Negative checks:
  - [ ] Closing the page and waiting produces a timeout error, not a hang.
  - [ ] Reloading the page after signing does not allow a second signature
        (nonce already consumed).
  - [ ] Rejecting in Freighter leaves the CLI waiting until the timeout.
  - [ ] Visiting `http://127.0.0.1:<port>/` **without** the nonce returns
        `400`; with a wrong nonce it returns `403`.
  - [ ] A request with `Host: attacker.example` returns `403`.
  - [ ] A POST with a foreign `Origin` header returns `403`.

## Automated tests

The handoff is covered without a browser:

- `src/utils/browser_signer.rs` unit tests cover nonce single-use, CSP contents,
  nonce gating, loopback `Host` enforcement, cross-origin rejection, nonce
  preservation on malformed payloads, replay (`410`), the mocked signer, the
  decoded summary, and a full raw-TCP handshake.
- `tests/browser_wallet_handoff.rs` drives the public API with a mocked
  `WalletSigner` (a fake wallet and a declining wallet) and with a raw TCP "mock
  browser wallet", including the rebound-host rejection path.

Run them with:

```bash
cargo test --test browser_wallet_handoff
cargo test utils::browser_signer
```

## Security notes

The nonce, loopback bind, strict CSP, origin/host checks, request size cap, and
short timeout are documented in
[`docs/SECURITY_THREAT_MODEL.md`](SECURITY_THREAT_MODEL.md) under
"Browser-wallet localhost handoff". In short:

- the page is only reachable from `127.0.0.1` and only with the one-time nonce;
- requests whose `Host` is not the loopback handoff are refused, which closes
  the DNS-rebinding window on the ephemeral port;
- cross-origin signature submissions are refused;
- inline script only runs with the per-response CSP nonce;
- the CLI never reads a secret key for `--signer browser` — only the signed XDR
  crosses the handoff.
