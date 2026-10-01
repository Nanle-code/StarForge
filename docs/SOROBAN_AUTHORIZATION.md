# Soroban Authorization Entries

Soroban transaction source accounts pay fees and authorize the transaction
envelope. A contract can separately call `require_auth` for another address.
The RPC simulation response returns those non-source authorizations as XDR
`SorobanAuthorizationEntry` values. Each entry is scoped to an invocation tree
and nonce; it is not a signature over the entire transaction.

StarForge parses address credentials from simulation, sets their signature
expiration to ten ledgers after the reported latest ledger, and computes the
network-specific Soroban authorization payload. Local wallet secrets (including
encrypted wallet entries) and Ledger devices can sign this payload. Source
account credentials are left to the transaction signer.

## Local signer

Create or import a wallet for the address required by the contract, then name
that wallet with the repeatable `--auth-signer` option:

```bash
starforge contract invoke C... transfer \
  --arg GTOKEN_OWNER... --type address \
  --arg GRECIPIENT... --type address \
  --arg 100 --type int \
  --wallet fee-payer \
  --auth-signer token-owner \
  --submit
```

`--wallet` is the transaction source and pays the network fee. `--auth-signer`
selects the wallet whose public key must match the address in the authorization
entry. If an entry has no matching signer, StarForge stops with an error naming
the required address. Repeat `--auth-signer` for contracts requiring multiple
addresses. The example assumes the contract's amount parameter is a signed
64-bit integer, matching `--type int`. For hardware signing, select `--hardware ledger` and use an
authorization address derived from that device's configured HD path.

## Remote signing

Export the simulated entries, transfer the JSON file to the signer, sign it,
then import the signed file on the submitting machine:

```bash
starforge contract invoke C... transfer \
  --arg GTOKEN_OWNER... --type address \
  --arg GRECIPIENT... --type address \
  --arg 100 --type i128 \
  --wallet fee-payer --submit \
  --auth-export auth-request.json

# On the signer machine, after configuring the matching wallet:
starforge contract auth-sign --file auth-request.json \
  --auth-signer token-owner --output auth-signed.json

# On the submitting machine:
starforge contract invoke C... transfer \
  --arg GTOKEN_OWNER... --type address \
  --arg GRECIPIENT... --type address \
  --arg 100 --type i128 \
  --wallet fee-payer --submit \
  --auth-import auth-signed.json
```

The offline signer validates the network, XDR entry, expiration, and Ed25519
signature against the authorization address and payload before import. Auth
signatures are short-lived; restart the export/sign/import flow when the
expiration ledger has passed or the invocation's required authorization changes.

## Protocol notes

An address credential signs the XDR `HashIdPreimageSorobanAuthorization` made
from the network ID, nonce, signature expiration ledger, and authorized
invocation. The resulting SHA-256 digest is signed with Ed25519, and the
signature is encoded into the credential's `SCVal` signature field. Contract
invocations can include multiple authorization entries, and each entry must be
signed by the address named in its own credentials.