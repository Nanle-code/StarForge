# StarForge Signing Agent — Security Threat Model

> Document status: **initial draft** (issue #929)  
> Last updated: 2026-09-29  
> Scope: `src/agent/`, `src/commands/agent.rs`, and the signing-path changes in
> `src/utils/wallet_signer.rs`

---

## 1. Purpose and motivation

Encrypted wallets currently require the user to re-enter their passphrase for
every `deploy`, `invoke`, or other signing command.  This friction pushes users
toward storing unencrypted (`S…`) secret keys on disk, which is far worse for
security.

The signing agent (`starforge agent`) is an **ssh-agent–style daemon** that
holds decrypted Ed25519 keys in locked memory for a configurable session
timeout.  Once unlocked, subsequent commands sign through the agent without
re-prompting — similar to `ssh-add` / `ssh-agent`.

---

## 2. Architecture overview

```
 ┌──────────────────────────────────────────────────┐
 │  starforge agent start [--timeout N] [--daemon]  │
 │                                                  │
 │  KeyStore (Arc<Mutex<…>>)                        │
 │  ┌───────────────────────────────────────────┐   │
 │  │  wallet_name → LockedKey                  │   │
 │  │  ┌──────────────────────────────────────┐ │   │
 │  │  │ seed: Zeroizing<[u8;32]>  (mlocked)  │ │   │
 │  │  │ expires_at: Option<Instant>          │ │   │
 │  │  └──────────────────────────────────────┘ │   │
 │  └───────────────────────────────────────────┘   │
 │                                                  │
 │  Listener: Unix socket 0600 / Windows named pipe │
 └──────────────────────┬───────────────────────────┘
                        │ newline-delimited JSON (NDJSON)
                        │ one request → one response per connection
             ┌──────────▼──────────────┐
             │  starforge deploy/invoke │  (AgentClient)
             └─────────────────────────┘
```

---

## 3. Assets and trust boundaries

| Asset | Where it lives | Trust boundary |
|-------|---------------|----------------|
| Ed25519 seed (32 bytes) | `KeyStore::LockedKey::seed` — locked RAM | agent process memory |
| Decrypted secret key (StrKey) | transmitted over the agent socket | local Unix socket / named pipe |
| Argon2 KDF passphrase | never stored by the agent | user input at `agent start` only |
| Encrypted secret key (blob) | `~/.starforge/config.toml` (0600) | filesystem |
| Signed transaction XDR | transmitted over socket, then to Soroban RPC | local network + internet |

---

## 4. Threat analysis

### T1 — Local process reads agent socket

**Threat**: A malicious or compromised local process connects to
`~/.starforge/agent.sock` and issues `Sign` or `AddKey` requests.

**Controls**:
- The socket is created with `0600` permissions (owner read/write only).  
  Any process running as a different UID cannot connect.
- On Linux a future hardening step can use `SO_PEERCRED` to verify the
  connecting PID/UID matches the owner.
- On Windows, the named pipe ACL is restricted to the creating user's SID.

**Residual risk**: A process running as the *same UID* can connect.  This is
the same trust boundary as `ssh-agent`.  Users who share a UID (e.g. containerized
CI) should set `STARFORGE_AGENT_SOCK` to a path with stricter ACLs or disable
the agent.

---

### T2 — Key material swapped to disk

**Threat**: The OS swaps the page containing the Ed25519 seed to a swap
partition, where it persists after agent exit.

**Controls**:
- `mlock(2)` (Linux/macOS) / `VirtualLock` (Windows) is called on every
  `LockedKey` allocation.  Failure is logged as a warning but does not abort —
  the key is still held in memory, just without swap protection.
- `Zeroizing<[u8; 32]>` ensures the bytes are overwritten with zeros on
  `Drop` (zeroize crate's `Zeroize` impl uses compiler-barrier-protected
  memset).
- `munlock(2)` is called in `LockedKey::drop()` before the buffer is freed.

**Residual risk**: Systems with low `RLIMIT_MEMLOCK` (common on containers and
some Linux distros) may silently fail `mlock`.  Users on such systems should
use a dedicated encrypted swap or a swap-disabled system.

---

### T3 — Key not zeroized on agent crash

**Threat**: If the agent process crashes (SIGKILL, OOM) without running
destructors, the key bytes may remain in process address space until the OS
reclaims the pages.

**Controls**:
- `Zeroizing` and the `mlock` pattern are best-effort on unclean exits.
  There is no guaranteed defense against SIGKILL.
- The agent is designed to run only for a bounded session (`--timeout`).
  The smaller the timeout, the smaller the window.
- Core dumps are not enabled by default on most distributions.  The agent
  documentation recommends `ulimit -c 0` in production environments.

**Residual risk**: Unavoidable on forced termination.  Users with strong
requirements should use hardware wallets (`--hardware ledger/trezor`), which
never expose key material to the host OS.

---

### T4 — Privilege escalation via the `Sign` operation

**Threat**: An attacker obtains access to the socket and issues `Sign` with an
arbitrary `transaction_xdr`, including a mainnet `mergeAccount` or large
payment.

**Controls**:
- The `--confirm` flag on `agent start` enables per-request interactive
  confirmation.  The user sees a preview of the transaction before the agent
  signs.
- The signing preview is built from the decoded XDR, not the raw bytes, so a
  crafted base64 encoding cannot hide the true operation.
- The `WalletUsagePolicy` (network allowlist, contract allowlist, fee cap) is
  enforced on every signing request.
- Mainnet signing through the agent respects the same
  `--allow-plaintext-mainnet` and `enforce_mainnet_plaintext_policy` gates as
  direct CLI signing.

**Residual risk**: When `--confirm` is not set (the default for daemon mode),
any process running as the same UID can trigger a signing without interaction.
This mirrors the ssh-agent threat model.  Users should enable `--confirm` for
high-value wallets or mainnet operations.

---

### T5 — Replay of a previously signed transaction

**Threat**: An attacker captures a signed `TransactionEnvelope` from a
previous agent session and re-submits it.

**Controls**:
- Stellar transactions include a `seq_num` that is consumed on submission.
  Replay is prevented at the network protocol layer.
- The agent is stateless with respect to previously signed transactions; it
  does not track what it has signed.

**Residual risk**: None beyond the network-layer protection.

---

### T6 — Protocol downgrade / version confusion

**Threat**: An old client (or a crafted request) sends a
`{"version": 0, ...}` request that the agent mishandles.

**Controls**:
- Every `dispatch_request` call checks `req.version == PROTOCOL_VERSION` and
  returns `AgentErrorCode::VersionMismatch` for any mismatch.
- The client always sends `PROTOCOL_VERSION` (currently `1`).
- Future breaking changes increment `PROTOCOL_VERSION`.

---

### T7 — Socket path injection via `STARFORGE_AGENT_SOCK`

**Threat**: An attacker sets `STARFORGE_AGENT_SOCK` to point to a socket they
control, causing the CLI to sign through a malicious agent.

**Controls**:
- `STARFORGE_AGENT_SOCK` is intended for testing only.  Production users
  should not set it.
- The client verifies the `AgentResponse.version` field and checks `ok: true`
  before trusting the response.  A malicious agent returning a crafted
  `signed_xdr` cannot inject a different signature without controlling the
  XDR serialization.
- The Stellar network independently verifies the signature against the
  account's public key; a forged XDR would simply be rejected.

---

## 5. Key lifecycle summary

```
agent start
  → user enters passphrase once
  → decrypt_secret() → plaintext seed → LockedKey::from_stellar_secret()
      → mlock(seed page)
      → seed stored in Zeroizing<[u8;32]>
      → expires_at set from --timeout

per-command signing
  → CLI → AgentClient::try_sign_via_agent()
      → if socket exists and key loaded:
          → LockedKey::sign() (in-process, no passphrase prompt)
      → else: fall back to resolve_local_secret() + passphrase prompt

background sweep (every 30 s)
  → KeyStore::sweep_expired()
      → expired keys: munlock() + Zeroizing drop (zeros memory)

agent stop / Ctrl-C
  → KeyStore::remove_all()
      → all keys: munlock() + Zeroizing drop
  → socket deleted
  → PID file deleted
```

---

## 6. Recommendations for operators

1. **Use `--timeout`** to limit key exposure.  The default (900 s = 15 min) is
   a reasonable trade-off.  Set lower for mainnet; the agent re-prompts on the
   next `deploy`/`invoke`.

2. **Enable `--confirm`** on the agent for mainnet wallets or any wallet where
   unattended signing is not acceptable.

3. **Use hardware wallets** (`--hardware ledger/trezor`) for highest-value
   accounts.  The signing agent explicitly skips the agent path for hardware
   signing requests.

4. **Restrict swap**.  Run `ulimit -c 0` and consider encrypted swap or
   `swapoff` on machines handling mainnet keys.

5. **Do not share UIDs** between trusted and untrusted processes on the same
   host.

6. **Rotate keys regularly** using `starforge wallet create --encrypt` and
   `starforge wallet import --encrypt`.

---

## 7. Out of scope

- Remote signing agents (the socket is always local).
- Multi-party signing / threshold signatures (separate feature).
- Key attestation / TEE integration.
- Audit log of every individual signing event (tracked separately in
  `src/utils/audit.rs`).
