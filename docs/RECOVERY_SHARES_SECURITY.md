# Recovery Shares: Threat Model & Custody Recommendations

## Overview

StarForge supports **opt-in** Shamir's Secret Sharing (SSS) for encrypted wallet
backups. This splits an encrypted backup into `N` recovery shares such that any
`M` of them (the **threshold**) can reconstruct the original backup, but `M-1`
shares reveal **zero information** about it.

The default remains single-passphrase encryption — recovery shares are strictly
opt-in.

```
starforge wallet export --name alice --output backup.json --shares 5 --threshold 3
```

## Threat model

### What recovery shares protect against

| Threat | Protection |
|--------|-----------|
| **Single point of failure** (lost passphrase, dead custodian, destroyed drive) | Any `M` shares reconstruct the backup; losing up to `N - M` shares is tolerable |
| **Individual share compromise** | Fewer than `M` shares reveal nothing about the encrypted data (information-theoretic security) |
| **Accidental data corruption** of one share | The integrity hash in each share detects corruption; any `M` valid shares suffice |

### What recovery shares do NOT protect against

| Threat | Limitation |
|--------|-----------|
| **Compromise of `M` or more custodians** | An adversary with `M` shares can reconstruct the backup |
| **Compromise of the original passphrase** | If the adversary has the passphrase, they can decrypt directly; shares are a backup mechanism, not a replacement |
| **Social engineering of custodians** | Share holders must protect their shares as carefully as the original passphrase |
| **Evil maid attacks on share generation** | If the machine generating shares is compromised at creation time, shares may be intercepted |

### Security properties

- **Information-theoretic secrecy**: Fewer than `M` shares provide zero
  information about the encrypted backup (Shamir's original proof, 1979).
- **No master secret**: There is no "master share" or "dealer key". All shares
  are symmetric — any `M` of them are equivalent.
- **Integrity verification**: Each share contains a SHA-256 hash of the
  reconstructed secret, allowing detection of corrupted or tampered shares
  without the passphrase.
- **Per-byte polynomial**: Each byte of the encrypted bundle is independently
  split across a random polynomial, ensuring strong mixing.

## Architecture

### Flow

```
                          ┌─────────────┐
  Wallet backup JSON  ──> │   Encrypt   │ ──> Encrypted bundle
                          │  (passphrase │     (salt:nonce:ciphertext)
                          │   optional)  │
                          └─────────────┘
                                │
                                ▼
                          ┌─────────────┐
                          │  Shamir     │ ──> Share 1 (JSON)
                          │  Split      │ ──> Share 2 (JSON)
                          │  (M-of-N)   │ ──> ...
                          └─────────────┘ ──> Share N (JSON)
```

### Reconstruction

```
  Share files (≥ M of them)
        │
        ▼
  ┌─────────────┐
  │  Lagrange    │ ──> Reconstructed encrypted bundle
  │  Interp.     │
  └─────────────┘
        │
        ▼
  ┌─────────────┐
  │  Decrypt     │ ──> Wallet backup JSON
  │  (passphrase)│
  └─────────────┘
```

### Share file format

Each share is an independent JSON file:

```json
{
  "index": 1,
  "payload": "a1b2c3d4...",
  "secret_hash": "e5f6a7b8...",
  "total_shares": 5,
  "threshold": 3
}
```

| Field | Description |
|-------|------------|
| `index` | 1-based share identifier (unique per share set) |
| `payload` | Hex-encoded GF(256) polynomial evaluations |
| `secret_hash` | SHA-256 hash of the data being split (for integrity verification) |
| `total_shares` | N — total number of shares created |
| `threshold` | M — minimum shares needed for reconstruction |

## Custody recommendations

### Share distribution

1. **Separate physical locations**: Store each share in a different physical
   location (safe deposit box, separate office, trusted family member's home).
2. **Separate custodians**: No single custodian should hold more than one share
   unless explicitly intended for redundancy.
3. **Separate access controls**: Shares held by the same entity with the same
   authentication bypass the M-of-N protection.

### Operational guidelines

| Recommendation | Rationale |
|---------------|-----------|
| **Write down shares, don't digitalize** | Paper is not subject to remote hacking; use fireproof safes for physical storage |
| **Verify reconstruction periodically** | Test that M shares can reconstruct the backup at least once per quarter |
| **Document the M-of-N scheme** | Record which custodians hold which share indices (but never the share contents) |
| **Use a manifest file** | The `--shares` command generates a manifest listing share file paths (not contents) |
| **Rotate if a custodian is compromised** | If a custodian's share may have been exposed, re-split with new random polynomials |
| **Consider geographic redundancy** | Distribute across cities or countries for disaster resilience |

### Recommended configurations

| Scenario | N | M | Rationale |
|----------|---|---|-----------|
| Individual with backup | 3 | 2 | Survives losing 1 share |
| Small team (2-5 people) | 5 | 3 | Any 3 team members can recover |
| Organization (6+ people) | 7 | 4 | Requires quorum of custodians |
| Maximum security | N | N | All shares required; no redundancy |

### What NOT to do

- **Don't store all shares in the same location** — defeats the purpose.
- **Don't use M = 1** — this is equivalent to single-passphrase mode; the
  threshold must be at least 2.
- **Don't email shares** — email is not end-to-end encrypted by default and
  is a common attack vector.
- **Don't store shares alongside the encrypted backup** — an attacker with the
  backup file and M shares can decrypt immediately.
- **Don't skip integrity verification** — always verify the `secret_hash`
  matches before trusting reconstructed data.

## Algebraic details

### GF(256) construction

- **Irreducible polynomial**: x⁸ + x⁴ + x³ + x + 1 (0x11B), the same used by
  AES.
- **Primitive element**: 2 (the generator for the log/exp tables).
- **Implementation**: Constant-time log/exp tables generated at compile time.

### Polynomial scheme

For a secret `s` of length `L` bytes:
1. For each byte position `i` in `[0, L)`:
   - Generate a random polynomial `f_i(x) = s[i] + c_1*x + c_2*x² + ... + c_{M-1}*x^{M-1}`
   - Share `j` gets `f_i(j)` for all `i`.
2. Reconstruction uses Lagrange interpolation at `x = 0`.

### Why GF(256)?

- Byte-aligned: secrets are naturally byte sequences.
- Fixed-size field: no variable-length arithmetic.
- Battle-tested: same field as AES, with well-understood properties.

## References

- [Shamir, A. (1979). How to Share a Secret. *Communications of the ACM*, 22(11), 612-613.](https://dl.acm.org/doi/10.1145/359168.359176)
- [RFC 9381 — Threshold Secret Sharing](https://datatracker.ietf.org/doc/rfc9381/)
- [GF(256) in AES](https://en.wikipedia.org/wiki/Finite_field_arithmetic#Rijndael_Galois_field)
