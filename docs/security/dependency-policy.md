# Dependency Security and Audit Policy

## 1. Overview
StarForge automates Rust dependency vulnerability detection on every pull request and weekly scheduled CI runs using `cargo-audit` and `cargo-deny`.

## 2. Advisory Configuration & Exemption Workflow
Vulnerability triage exemptions are defined in:
- `audit.toml` / `.cargo/audit.toml` for `cargo-audit`
- `deny.toml` for `cargo-deny`

Any ignored advisory must include:
1. The RUSTSEC advisory identifier.
2. The dependency path and reason for the exemption (e.g. build-time proc macro, incompatible MSRV constraint, or absence of vulnerable code execution).
3. The planned mitigation or upstream tracking link.

## 3. CI Workflow
- `.github/workflows/audit.yml` runs automated security scans on PRs, master pushes, and a weekly cron schedule.
