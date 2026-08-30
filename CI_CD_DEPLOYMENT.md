# CI/CD Deployment Integration

StarForge provides a consistent CI/CD deployment interface for GitHub Actions, GitLab CI, and Jenkins. Each provider runs the same quality gate before it can deploy and delegates infrastructure-specific work to protected CI secrets.

## Quality Gate

The deployment pipelines require:

- `cargo fmt --all --check`
- `cargo build --locked`
- `cargo test --locked`
- `cargo clippy --all-features --locked -- -D warnings`
- `cargo test --test cli_smoke --locked`

The existing deployment verification and rollback harness can be added to project release pipelines when contract artifacts and rollback scenarios are available. See `ROLLBACK_TESTING.md`.

## Non-Interactive / Headless Mode

Several `starforge` subcommands (wallet decryption, backup encryption, hardware wallet confirmations, registry login/signup) normally prompt on stdin. Running one of those unattended used to hang the job until it timed out. The CLI now detects a non-interactive environment and fails fast with a clear error instead, and accepts headless alternatives for every value it would otherwise prompt for.

**Detection** (any one triggers it):

- `--non-interactive` passed on the command line
- `$CI` is set (set automatically by GitHub Actions, GitLab CI, and Jenkins)
- `$STARFORGE_NON_INTERACTIVE` is set to a truthy value (`1`, `true`, `yes`, `on`)
- stdin isn't a terminal (piped input, `< /dev/null`, etc.)

**Headless alternatives**, checked before any prompt would otherwise appear:

| Prompt | Alternative |
| --- | --- |
| Confirmation prompts (deploy, upgrade, hardware signing, etc.) | `--yes` / `-y` |
| Wallet or backup password (decrypt) | `$STARFORGE_PASSWORD` |
| New wallet or backup passphrase (create) | `$STARFORGE_PASSPHRASE` — still enforced against the minimum length and, with `--strict`, the strength requirements; a value that fails validation errors immediately instead of looping |
| Registry login/signup email | `--email` flag, or `$STARFORGE_REGISTRY_EMAIL` |
| Registry signup username | `--username` flag, or `$STARFORGE_REGISTRY_USERNAME` |
| Registry login/signup password | `$STARFORGE_REGISTRY_PASSWORD` |

**Migration note**: a pipeline that previously supplied `--yes` for every state-changing command needs no changes. A pipeline that relied on piping an answer into stdin (`echo "yes" | starforge ...`) should switch to `--yes`/the env vars above — piped, non-interactive stdin is exactly the case this change now rejects, since there's no way to tell a real answer from an empty pipe. Treat the `STARFORGE_PASSWORD`, `STARFORGE_PASSPHRASE`, and `STARFORGE_REGISTRY_*` variables as secrets: set them via your CI provider's masked/protected secret store, never in a committed file.

## Required CI Secrets

Configure these values as masked/protected secrets (GitHub environment secrets, GitLab protected variables, or Jenkins credentials):

| Name | Purpose |
| --- | --- |
| `STARFORGE_DEPLOY_COMMAND` | Command that deploys the immutable artifact for the selected environment. |
| `STARFORGE_ROLLBACK_COMMAND` | Command that restores the last known-good artifact. |
| `STARFORGE_HEALTHCHECK_URL` | Optional endpoint returning a successful HTTP status once the release is healthy. |

Commands are executed only by manually approved deployment jobs. Do not put tokens directly in the command; use the provider's secret mechanism or workload identity.

## GitHub Actions

Run **Safe Deployment** with `deploy` or `rollback` and choose `staging` or `production`. Create matching GitHub environments and configure required reviewers for `production`; the workflow waits for that approval, serializes work per environment, then uses the environment's secrets.

## GitLab CI

`.gitlab-ci.yml` defines manual staging and default-branch production deployment/rollback jobs. Mark the production environment and variables as protected. `resource_group` prevents overlapping environment changes.

## Jenkins

The `Jenkinsfile` accepts `verify`, `deploy`, or `rollback`. Add Jenkins Secret Text credentials using these IDs:

- `starforge-deploy-command`
- `starforge-rollback-command`
- `starforge-healthcheck-url`

Production runs pause for an explicit Jenkins approval.

## Monitoring and Rollback

`scripts/ci-deploy.sh` polls `STARFORGE_HEALTHCHECK_URL` after deployment. A failed health check fails the job and makes the separately manual rollback action immediately available. The rollback job executes `STARFORGE_ROLLBACK_COMMAND` and checks health once more. Configure an external alert against the same endpoint so operators receive failures beyond CI logs.

## Local Validation

```bash
bash -n scripts/ci-deploy.sh scripts/ci-rollback.sh
```
