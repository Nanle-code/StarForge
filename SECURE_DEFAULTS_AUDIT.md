# Secure Defaults Audit Checklist

This checklist verifies that StarForge ships with secure, privacy-respecting defaults before each release. It combines **automated checks** (run in CI) with **manual attestation items** that require maintainer sign-off.

## How to Use

1. **Before tagging a release**, run the automated audit:
   ```bash
   cargo test --test secure_defaults_audit --locked
   ```
2. **Manual items** must be reviewed and attested by a maintainer in the release PR.
3. **Release tagging is blocked** unless the audit checklist evidence (CI output + attestation) is attached to the release PR.

---

## Automated Checks (CI-Enforced)

These items are verified by `tests/secure_defaults_audit.rs` and run in CI on every push/PR.

| # | Check | Expected Default | Status |
|---|-------|-----------------|--------|
| A1 | **Telemetry opt-out respected** — `telemetry_enabled` defaults to `true` (opt-out model) | `Some(true)` | ⬜ |
| A2 | **Telemetry local-only** — no network transmission code paths exist for telemetry | Local file write only | ⬜ |
| A3 | **AI telemetry cloud disabled by default** — `cloud_aggregation_enabled` defaults to `false` | `false` | ⬜ |
| A4 | **AI telemetry cloud endpoint empty** — no default remote endpoint | `None` | ⬜ |
| A5 | **Feature flag metrics enabled** — `metrics_enabled` defaults to `true` | `true` | ⬜ |
| A6 | **Feature flag metrics retention capped** — `metrics_retention_days` defaults to 30 | `30` | ⬜ |
| A7 | **Friendbot absent on mainnet** — mainnet `friendbot_url` is `None` | `None` | ⬜ |
| A8 | **Friendbot present on testnet** — testnet `friendbot_url` points to `friendbot.stellar.org` | `Some("https://friendbot.stellar.org")` | ⬜ |
| A9 | **Default network is testnet** — `network` defaults to `"testnet"` | `"testnet"` | ⬜ |
| A10 | **Plugin trust sources restricted** — default trusted sources match known repos only | 3 known sources | ⬜ |
| A11 | **Wallet encryption opt-in** — `wallet_encryption` defaults to `None` (no encryption unless opted in) | `None` | ⬜ |
| A12 | **Config schema version current** — `version` matches `CURRENT_CONFIG_VERSION` | `"1"` | ⬜ |
| A13 | **File permissions restricted** — sensitive files created with 0600 mode on Unix | `0o600` | ⬜ |
| A14 | **Data directory permissions restricted** — data dir created with 0700 mode on Unix | `0o700` | ⬜ |
| A15 | **Network passphrase validated** — default networks include correct Stellar passphrases | Non-empty | ⬜ |

---

## Manual Attestation Items

These items require a maintainer to review and sign off. They cannot be fully automated.

| # | Check | Attestation Required | Reviewer | Date |
|---|-------|---------------------|----------|------|
| M1 | **No new network endpoints leak user data** — verify no telemetry/analytics endpoints were added without consent | Maintainer confirms no new outbound endpoints | | |
| M2 | **File permission audit on Windows** — verify sensitive files use appropriate Windows ACLs (not world-readable) | Maintainer confirms Windows file security | | |
| M3 | **Release binary does not contain debug symbols or test keys** — verify release builds strip debug info | Maintainer confirms `--release` builds are clean | | |
| M4 | **Dependency security audit clean** — `cargo deny` passes with no advisories | CI output attached | | |
| M5 | **Changelog documents any default changes** — if defaults changed from previous release, changelog reflects this | Maintainer confirms changelog accuracy | | |
| M6 | **Friendbot restrictions verified on custom networks** — custom networks without `friendbot_url` cannot accidentally use testnet Friendbot | Maintainer confirms network isolation | | |
| M7 | **Telemetry payload does not contain PII** — verify telemetry events contain only anonymous usage data | Maintainer confirms payload review | | |
| M8 | **AI telemetry retention pruning works** — verify old records are pruned after `retention_days` | Maintainer confirms pruning behavior | | |

---

## Attestation Template

When opening a release PR, include the following attestation block:

```markdown
## Secure Defaults Attestation

### Automated Checks
- [ ] `cargo test --test secure_defaults_audit --locked` passes (attach CI log link)

### Manual Attestation
- [ ] M1: No new network endpoints leak user data — reviewed by @<reviewer>
- [ ] M2: File permission audit on Windows — reviewed by @<reviewer>
- [ ] M3: Release binary clean of debug symbols — reviewed by @<reviewer>
- [ ] M4: Dependency security audit clean — CI link: <link>
- [ ] M5: Changelog documents default changes — reviewed by @<reviewer>
- [ ] M6: Friendbot restrictions on custom networks — reviewed by @<reviewer>
- [ ] M7: Telemetry payload free of PII — reviewed by @<reviewer>
- [ ] M8: AI telemetry retention pruning — reviewed by @<reviewer>
```

---

## Release Process Integration

### CI Gate

The `secure-defaults` job in `.github/workflows/ci.yml` runs the automated audit on every push and PR. **This job must pass before a release can be tagged.**

### Tag Protection

Release tags (`v*`) require:
1. All CI checks green (including `secure-defaults`)
2. Release PR merged with attestation block completed
3. No merge conflicts with `master`

### Adding New Checks

To add a new automated check:
1. Add the check to `tests/secure_defaults_audit.rs`
2. Update this checklist with the new item
3. The CI gate automatically picks up the new test

To add a new manual check:
1. Add the item to the Manual Attestation table above
2. Update the Attestation Template
3. Document in the release process

---

## References

- [TELEMETRY_PRIVACY.md](TELEMETRY_PRIVACY.md) — Telemetry data collection and privacy
- [SECURITY_LOGGING_GUIDE.md](SECURITY_LOGGING_GUIDE.md) — Security logging requirements
- [CI_ENFORCEMENT.md](CI_ENFORCEMENT.md) — CI pipeline and enforcement
- [CONTRIBUTING.md](CONTRIBUTING.md) — Contribution guidelines

---

*Last updated: 2026-08-30 — Issue #797: Implement secure defaults audit as a release checklist gate*
