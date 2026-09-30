# {{PROJECT_NAME}}

A custom Soroban smart wallet: ed25519 and passkey (secp256r1) signers with a
per-period spending limit.

Built-in Stellar accounts delegate every authorization decision to the host - a
key signs, the host checks, done. A **custom account** takes that decision back
by implementing `__check_auth`, which the host invokes for every call that needs
the account's authorization. Whatever `__check_auth` accepts is authorized and
whatever it rejects fails the whole transaction, which makes that one function
the entire security boundary of the account.

This template is the smallest complete example of the three things custom
accounts are usually made of: several signature schemes, a policy that limits
what a signature can do, and explicit rejection instead of silent approval.

## Features

- Signers of mixed scheme - classic ed25519 keys and secp256r1 passkeys in the
  same account
- A spending limit of at most `amount` per `period_seconds`, enforced against the
  invocation being authorized
- `transfer` and `balance`, so the account is a usable wallet and not only an
  auth module
- Every failure path returns a distinct `Error`, so a rejection is diagnosable

## Build

```bash
stellar contract build
```

## Test

```bash
# Resolve the dependency drift described under "Build note" below, then test.
cargo update -p ed25519-dalek@3.0.0 --precise 2.1.1
cargo test

# the passkey happy path is ignored by default; see "Known limitation"
cargo test -- --ignored
```

## Deploy

```bash
starforge deploy \
  --wasm target/wasm32-unknown-unknown/release/{{PROJECT_NAME_SNAKE}}.wasm \
  --network testnet
```

## Security model

**The signer set is fixed once.** `initialize` may be called a single time; a
second call returns `AlreadyInitialized`. There is deliberately no
add-signer/remove-signer path, because every one of those is a place where a bug
becomes permanent key loss. A real deployment wanting key rotation would deploy
a new account and migrate the balance.

**Any one signer can authorize.** This is a multi-device wallet, not a
multi-signature vault: a valid signature from a single registered signer is
enough. That is what makes it practical to keep a hardware key and a phone
passkey on the same account, and it is the reason the spending limit is not
optional - see below.

**The spending limit is the compensating control.** Because one key is enough, a
single compromised key would otherwise be able to drain the whole balance. The
limit bounds that: at most `amount` per rolling `period_seconds` window, and a
single transfer may not exceed `amount` even when the window is empty, so
waiting for a reset does not raise the per-transaction ceiling.

The limit is read from the **authorized invocation's own arguments**, not from
anything the caller supplies. An account that trusted a caller-provided amount
would be trivially bypassed by authorizing a different call, so the value the
host is authorizing and the value the limit is applied to are the same value.

Anything `__check_auth` cannot reason about is refused rather than allowed: a
context for a different contract, a different function, a context with an
unexpected argument count, or a deployment context all return
`UnknownInvocation` / `MissingAmount`. A policy that approves what it does not
understand is not a policy.

**Both signature schemes are verified, and they hash differently.** This is the
detail most likely to be got wrong:

- ed25519 signs the **message**, so `ed25519_verify` receives the payload as-is.
- secp256r1 signs a **32-byte digest**, so `secp256r1_verify` receives
  `sha256(payload)`.

That matches what a real passkey produces, since WebAuthn signs the SHA-256
hash of the signed data. Passing the raw payload to the secp256r1 verifier
rejects every genuine passkey, and hashing for ed25519 rejects every genuine
ed25519 key.

A secp256r1 key is stored as the **65-byte uncompressed SEC-1** point, which is
what the host's verifier takes. A passkey's usual 33-byte compressed key must be
decompressed before it is registered.

**The signature names its key.** `__check_auth` receives one opaque signature
value but the account may hold several keys, so the signature is an
`AccountSignature { signer, signature }` naming an index into the signer set.
Without it the account would have to try every key against every signature, and
64 bytes of noise would be indistinguishable from a real signature.

**No fallback.** There is no path on which a failed check leads to approval. A
custom account that degrades to permissive is an account with no security model,
so unknown signer, malformed signature, bad amount and exceeded limit are each
distinct hard errors.

### Changing the policy

The two knobs worth changing, and what each costs:

| Change | Effect | Trade-off |
| --- | --- | --- |
| Require **all** signers | One stolen key cannot spend anything | N keys means N signatures and N devices; losing one bricks the account |
| Require a **threshold** (M-of-N) | Tolerates losing M-1 keys | More code in `__check_auth`, and a smaller N makes a threshold close to "all" anyway |
| Raise or drop the **limit** | Larger transfers become possible | The limit is what makes "any one signer" safe; removing it without adding a threshold turns one compromised key into total loss |

A single-signer account can use this template unchanged. For an M-of-N variant,
`verify_signature` becomes `verify_signatures`, and the `AccountSignature` type
grows a bitfield or a list of signer indices so several keys can sign the same
payload.

## Interface

| Function | Purpose |
| --- | --- |
| `initialize(signers, limit)` | Configure the account. Callable once. |
| `transfer(token, to, amount)` | Send tokens. Authorized by `__check_auth`. |
| `balance(token)` | The account's balance for a token. |
| `signers()` | The registered signers. |
| `limit()` | The configured spending limit. |
| `spent()` | Amount spent in the current window. |
| `remaining()` | Allowance left in the current window. |
| `__check_auth(...)` | The host's authorization entrypoint. |

## Errors

| Variant | Meaning |
| --- | --- |
| `AlreadyInitialized` | `initialize` called twice |
| `NoSigners` | Empty signer set |
| `MalformedSigner` | Key length does not match its scheme |
| `DuplicateSigner` | Same key registered twice |
| `InvalidLimit` | Non-positive amount, or zero period |
| `NotInitialized` | Account has no signer set stored |
| `UnknownSigner` | Signature named a signer index that does not exist |
| `MalformedSignature` | Signature was not a 64-byte value |
| `MissingAmount` | Authorized invocation had no readable amount |
| `MalformedAmount` | Amount was not an `i128` |
| `SpendingLimitExceeded` | Transfer exceeds the allowance for the window |
| `UnknownInvocation` | Not this account's `transfer`, or a context with no amount |
| `NonPositiveAmount` | Amount was zero or negative |

## Known limitation

`check_auth_accepts_a_passkey_signer` is `#[ignore]`d because
`soroban-env-host` 22.1.3 rejects valid secp256r1 signatures with
`Crypto / InvalidInput`. This is a host defect, not a template one:

- `p256` 0.13.2 - the version the host itself depends on - verifies the same
  signature and key locally
- the digest handed to the host is `sha256(payload)`, which the host confirms
  matches `sha256` of the signed message
- the key is 65-byte uncompressed SEC-1 and the signature 64 bytes, the exact
  layout of the host's own NIST P-256 test vector
- it is not a budget limit: `cost_estimate().budget().reset_default()` does not
  change the result, and the host's own unit test resets the budget for the same
  reason

The host passes its own NIST vector when `verify_sig_ecdsa_secp256r1` is called
directly, so the fault is in the path the SDK dispatches through. The ed25519
path and all policy checks are unaffected and are covered. Run
`cargo test -- --ignored` once the host is fixed.

## CLI signing

Signing an authorization entry for a non-source signer is **not** included,
because it does not exist in StarForge yet - it is tracked separately in
[#906](https://github.com/Nanle-code/StarForge/issues/906). Until that lands,
an account like this can be exercised through `stellar contract invoke` and
Soroban test utilities, which is what the test suite does.

## Build note

This template pins `soroban-sdk = "22.0.0"`, matching the SDK the CLI itself
builds against.

`cargo build` works as-is. `cargo test` needs one extra step, because of
dependency drift that is **not** specific to this template - every bundled
template is affected, and `escrow` fails identically on `master`:

- `soroban-env-host` declares `ed25519-dalek = ">=2.0.0"`, which is open-ended,
  so Cargo now resolves it to 2.2.0/3.0.0
- the same host pins `rand_chacha = "0.3.1"`, whose PRNG implements the older
  `rand_core` 0.6 `CryptoRng`
- `ed25519-dalek` 3.x expects the newer `rand_core` 0.10 trait, so
  `soroban-env-host`'s own `testutils` module fails to compile

Adding a direct `ed25519-dalek` dependency does not fix it, because Cargo
resolves the host's `>=2.0.0` requirement independently and ends up with two
copies. The lockfile pin above is the reliable fix. It affects only the test
harness: the `testutils` feature is what pulls the broken module in, so
`stellar contract build` and deployment are unaffected.

The fix belongs upstream in the host's dependency spec, or in a workspace-level
lockfile; it is called out in this template's pull request rather than patched
here.
