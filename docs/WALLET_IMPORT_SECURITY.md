# Wallet Import & Backup: Parser Hardening

`starforge wallet import --file` and `starforge backup restore` read files that
came from outside the tool — a colleague's export, a CI artifact, a USB stick.
That makes their parsers a trust boundary, so they live in
[`src/utils/wallet_import.rs`](../src/utils/wallet_import.rs), separated from
prompting, disk access, and the config store, and are driven by three
continuously-run fuzz harnesses.

---

## What is enforced

### Size

| Limit | Value | Checked |
|---|---|---|
| Backup document | 4 MiB | Before the JSON parser runs |
| Encrypted bundle | 8 MiB | Before base64 decoding |
| Wallets per backup | 1,000 | After parsing, before validation |
| Wallet name | 64 characters | Per entry |

Size gates run *first*, so an oversized file cannot drive a large allocation
inside the JSON parser.

### Backup documents

- Must be a JSON object with `version`, `exported_at`, and a non-empty
  `wallets` array.
- `version` `"1"` is accepted with a warning (re-export to gain tamper
  detection). `version` `"2"` is the current format; its HMAC-SHA256 integrity
  tag is verified on import. Any other version is refused with a message naming
  both versions rather than being partially read.
- Wallet names must be unique within the file.
- Each entry's `public_key` must be a 56-character `G…` StrKey; a
  `secret_key`, when present, must be a 56-character `S…` StrKey or a
  well-formed encrypted bundle.
- `network` must be non-empty.

### Encrypted bundles

A bundle is `salt:nonce:ciphertext`, optionally followed by Argon2 parameters
(`:mem:iterations` or `:mem:iterations:parallelism`).

| Field | Requirement |
|---|---|
| `salt` | Valid base64, decoding to exactly 16 bytes |
| `nonce` | Valid base64, decoding to exactly 12 bytes |
| `ciphertext` | Valid base64, at least 16 bytes (one AES-GCM tag) |
| `mem`, `iterations`, `parallelism` | Decimal `u32`, greater than zero |

The structure is checked **before** the passphrase prompt, so a corrupt file
fails immediately instead of after an Argon2 key derivation.

### Unicode

Wallet names are rejected when they contain characters that are invisible or
that reorder rendering — they can make one wallet's name look exactly like
another's:

| Range | Characters |
|---|---|
| `U+0000`–`U+001F`, `U+007F` | Control characters |
| `U+200B`–`U+200F` | Zero-width space through right-to-left mark |
| `U+202A`–`U+202E` | Bidirectional embedding and override |
| `U+2066`–`U+2069` | Bidirectional isolates |
| `U+00AD` | Soft hyphen |
| `U+FEFF` | Zero-width no-break space |

Non-ASCII names are **accepted with a warning**, because earlier releases
allowed any Unicode alphanumeric and rejecting them outright would make old
backups unreadable. The warning flags the homograph risk (Cyrillic `а` renders
like Latin `a`):

```
⚠ wallet 'аlice' has a non-ASCII name, which can render identically to another name
```

### Error messages

A rejection quotes the wallet name and the reason, never the key material —
error text lands in terminals, CI logs, and bug reports.

---

## Backup format versions

| Version | Integrity tag | Accepted | Migration note |
|---|---|---|---|
| 1 | None | Yes, with warning | Re-export to get tamper detection |
| 2 | HMAC-SHA256 | Yes | Tag verified on import; failure means tampering |

The tag is HMAC-SHA256 over the canonical JSON of the backup document (with the
`integrity_tag` field set to `null`), encoded as lowercase hex. The key is the
well-known constant `starforge-wallet-backup-v2`; the MAC provides integrity,
not confidentiality.

## Compatibility

v1 backups written by older CLI versions remain importable. `starforge wallet import`
accepts them with the warning:

```
⚠ backup is version 1 (no integrity tag); re-export to get tamper detection
```

v2 is written by all new exports, including the pre-rotation snapshot created
by `starforge wallet rotate --backup <file>`. If you have critical backups made
by a v1 CLI, re-export them now to gain tamper detection.

---

## Behaviour changes

### Encrypted bundles with custom Argon2 parameters now import correctly

Encryption detection used to be `raw.matches(':').count() == 2`, which only
recognises the 3-part bundle. A backup encrypted with custom Argon2 parameters
(`starforge wallet export` after `--mem` / `--iterations` / `--parallelism`) has
5 or 6 parts, so it was handed to the JSON parser and failed with a misleading
`Invalid backup JSON format`.

Detection now follows the bundle grammar. **If you have a backup that failed to
import with that error, retry it — no re-export is needed.**

### A JSON document is never treated as a bundle

Classification returns "plaintext" for anything starting with `{` or `[`,
regardless of how many colons the values contain. Previously a JSON document
with exactly two colons could trigger a passphrase prompt for a passphrase that
does not exist.

### New rejections

Files that previously imported and now do not:

| Input | Now rejected because |
|---|---|
| A backup with more than 1,000 wallets | Above the per-file limit |
| A wallet name longer than 64 characters | Above the name limit |
| A wallet name containing bidi or zero-width characters | Deceptive name |
| A backup document above 4 MiB | Above the size limit |

These were previously accepted, so a file hitting one of them was already
unusual. Split an oversized backup, or rename the offending wallet before
exporting.

---

## Fuzzing

```bash
cargo fuzz run fuzz_wallet_backup_parse       -- -dict=fuzz/dicts/wallet_backup.dict
cargo fuzz run fuzz_wallet_import_envelope    -- -dict=fuzz/dicts/wallet_backup.dict
cargo fuzz run fuzz_wallet_backup_structured
```

`cargo fuzz` needs a nightly toolchain. The same invariants run on stable in
[`tests/wallet_import_property_tests.rs`](../tests/wallet_import_property_tests.rs),
so every PR checks them without nightly:

```bash
cargo test --test wallet_import_property_tests
PROPTEST_CASES=10000 cargo test --test wallet_import_property_tests
```

See [FUZZING_GUIDE.md](../FUZZING_GUIDE.md) for the harness inventory, the seed
corpora, and the invariants each target asserts.

---

## See also

- [WALLET_ENCRYPTION_FIX.md](../WALLET_ENCRYPTION_FIX.md) — the encryption format itself
- [SECURITY_LOGGING_GUIDE.md](../SECURITY_LOGGING_GUIDE.md) — what may be logged
- [docs/COMMAND_REFERENCE.md](COMMAND_REFERENCE.md) — the `wallet` command

---

## Secret material lifetime and zeroization

StarForge uses the [`zeroize`](https://docs.rs/zeroize) crate to overwrite
sensitive bytes with zeros before they are freed. The following are zeroized
immediately after use:

| Material | File | Mechanism |
|---|---|---|
| Argon2-derived AES key (32 bytes) | `utils/crypto.rs` | `Zeroizing<[u8; 32]>` drops on scope exit |
| BIP39 seed (64 bytes) | `utils/mnemonic.rs` | `Zeroizing<[u8; 64]>` drops on scope exit |
| SLIP-0010 intermediate key + chain (2 × 32 bytes per derivation step) | `utils/mnemonic.rs` | `Zeroizing<[u8; 32]>` drops after each child derivation |
| Raw ed25519 private key bytes | `utils/mnemonic.rs` | `Zeroizing<[u8; 32]>` drops on scope exit |
| Decrypted Stellar secret key string | `utils/wallet_signer.rs` | `Zeroizing<String>` drops with `SigningRequest` |
| Passphrase / password from terminal prompt | `utils/crypto.rs` | `Zeroizing<String>` drops when caller is done |

### Compatibility notes

`zeroize` v1.9.0 was already an indirect dependency (pulled in by `argon2`); this
change makes it direct and enables the derive feature. No migration is needed.

### Security caveats

- **WASM builds**: `zeroize` uses `volatile_write` and a compiler fence on native
  targets. WebAssembly JIT runtimes do not guarantee that volatile semantics survive
  compilation; the `starforge-wasm` crate does not handle raw secret material directly,
  so this is informational rather than a gap.
- **Heap realloc**: `Zeroizing<String>` zeros the final heap allocation. If the
  allocator grew the string via realloc, earlier copies of the bytes in freed heap
  blocks are not covered. For the highest assurance, use a locked-memory allocator.
- **Swap**: Pages written to swap before the zero pass occur are not retroactively
  cleared. Use full-disk encryption or an encrypted swap partition on machines
  handling mainnet keys.

