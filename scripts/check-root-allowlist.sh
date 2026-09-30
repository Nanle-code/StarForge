#!/usr/bin/env bash
set -euo pipefail

violations=()

while IFS= read -r -d '' file; do
  case "$file" in
    .cargo-mutants.toml|.dockerignore|.gitattributes|.gitignore|.gitlab-ci.yml|\
    AI_DEVELOPER_INTELLIGENCE.md|AI_INTEGRATION_GUIDE.md|AI_PROMPT_GUIDE.md|\
    API_REFERENCE.md|ARCHITECTURE.md|ARCHITECTURE_AI.md|audit.toml|\
    AUDIT_TRAIL_DOCUMENTATION.md|BENCHMARKS.md|build.rs|\
    BUILD_BASELINE_VERIFICATION.md|BUILD_TROUBLESHOOTING.md|Cargo.lock|Cargo.toml|\
    CHANGELOG.md|CI_CD_DEPLOYMENT.md|CI_ENFORCEMENT.md|CLI_LATENCY_BUDGETS.md|\
    CODE_STYLE_STANDARDS.md|CONTRIBUTING.md|CONTRIBUTOR_QUICK_REFERENCE.md|\
    CORPUS_HYGIENE.md|deny.toml|DEPLOYMENT_E2E_TESTS_QUICK_REFERENCE.txt|\
    DEPLOYMENT_PREPARATION_TEST_COVERAGE.md|DEVELOPER_GUIDE.md|\
    DEVELOPMENT_WORKFLOW.md|docker-compose.yml|Dockerfile|Dockerfile.dev|\
    DOCKER_SETUP.md|DOCTEST_GUIDELINES.md|Documentation.md|\
    ENCRYPTION_FIX_SUMMARY.txt|install.sh|Jenkinsfile|LICENSE|plan.md|\
    PROGRESSIVE_DISCLOSURE.md|pr_body.md|QUICK_START_TEMPLATES.md|README.md|\
    RPC_BUDGETING.md|STABILITY.md|starforge-deploy-policy.example.toml|\
    starforge-gates.example.toml|starforge-project.example.toml|\
    starforge-simulation-profiles.toml|starforge-size-budget.example.toml|\
    TEMPLATE_COMPATIBILITY_MATRIX.md|TEMPLATE_TESTS_QUICK_REFERENCE.txt|\
    TEST_WITH_REAL_CONTRACT.md|WALLET_E2E_TESTS_QUICK_REFERENCE.txt)
      ;;
    *)
      violations+=("$file")
      ;;
  esac
done < <(find . -maxdepth 1 -type f -printf '%f\0')

if ((${#violations[@]})); then
  printf 'Unexpected root-level file(s):\n' >&2
  printf '  %s\n' "${violations[@]}" >&2
  exit 1
fi

printf 'Root-level file allowlist check passed.\n'
