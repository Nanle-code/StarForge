# Deploy Policy as Code

Organizations can encode deploy rules — allowed networks, required reviewers,
and release checklists — in a version-controlled policy file. StarForge enforces
the policy at deploy time and provides a CI-friendly validator that never
touches a network.

---

## Policy file

Default discovery order (project root):

1. `starforge-deploy-policy.toml`
2. `starforge-deploy-policy.yaml` / `.yml`

Override with `starforge deploy --policy <PATH>`.

Generate a starter file:

```bash
starforge deploy-policy init starforge-deploy-policy.toml
```

See [starforge-deploy-policy.example.toml](../starforge-deploy-policy.example.toml)
for a complete example.

---

## Schema

| Field | Type | Description |
|-------|------|-------------|
| `organization` | string (optional) | Display name in reports |
| `allowed_networks` | string[] | Networks permitted for deploy (e.g. `testnet`, `mainnet`) |
| `require_execute_flag` | bool | When true, real deploys must pass `--execute` |
| `required_reviewers` | table[] | Each entry: `username`, optional `role` |
| `checklist` | table[] | Each entry: `id`, `description`, `required` (default true) |

TOML and YAML are both supported; use the file extension to select the parser.

### TOML example

```toml
organization = "acme-corp"
allowed_networks = ["testnet"]
require_execute_flag = true

[[required_reviewers]]
username = "security-lead"
role = "security"

[[checklist]]
id = "audit-passed"
description = "Run starforge audit with no critical findings"
required = true
```

### YAML example

```yaml
organization: acme-corp
allowed_networks:
  - testnet
require_execute_flag: true
required_reviewers:
  - username: security-lead
    role: security
checklist:
  - id: audit-passed
    description: Run starforge audit with no critical findings
    required: true
```

---

## Runtime signals

At deploy time, StarForge reads:

| Source | Purpose |
|--------|---------|
| Deploy flags | `--network`, `--execute`, `--checklist` |
| `STARFORGE_DEPLOY_APPROVERS` | Comma-separated identities (must include all required reviewers) |
| `STARFORGE_DEPLOY_CHECKLIST` | Comma-separated checklist ids satisfied for this release |

Example:

```bash
export STARFORGE_DEPLOY_APPROVERS="security-lead,release-manager"
export STARFORGE_DEPLOY_CHECKLIST="audit-passed,changelog-updated"

starforge deploy \
  --wasm ./target/wasm32v1-none/release/contract.wasm \
  --network testnet \
  --execute \
  --wallet deployer
```

Or pass checklist ids on the command line:

```bash
starforge deploy --wasm ./contract.wasm --execute \
  --checklist audit-passed,changelog-updated \
  --policy starforge-deploy-policy.toml
```

Violations produce actionable errors naming the rule, message, and remediation.

---

## CI validation (no deploy)

Validate policy files in CI without deploying:

```bash
starforge deploy-policy check --config starforge-deploy-policy.toml \
  --network testnet \
  --execute \
  --approvers security-lead,release-manager \
  --checklist audit-passed,changelog-updated
```

Exit code is non-zero when any rule would block a deploy. Use `--json` for
machine-readable reports.

Example GitHub Actions step:

```yaml
- name: Validate deploy policy
  run: |
    starforge deploy-policy check \
      --config starforge-deploy-policy.toml \
      --network testnet \
      --execute \
      --approvers security-lead,release-manager \
      --checklist audit-passed,changelog-updated
```

---

## Related docs

- [CONFIRMATION_UX.md](CONFIRMATION_UX.md) — destructive confirmation prompts
- [COMMAND_REFERENCE.md](COMMAND_REFERENCE.md) — deploy flags
