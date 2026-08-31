#!/usr/bin/env bash
set -euo pipefail

: "${STARFORGE_DEPLOY_ENVIRONMENT:?Set STARFORGE_DEPLOY_ENVIRONMENT to the target environment.}"
: "${STARFORGE_ROLLBACK_COMMAND:?Set STARFORGE_ROLLBACK_COMMAND to the protected rollback command.}"

if [[ "${STARFORGE_DEPLOY_ENVIRONMENT}" == "production" && "${STARFORGE_ROLLBACK_APPROVED:-}" != "true" ]]; then
    echo "Production rollback requires STARFORGE_ROLLBACK_APPROVED=true."
    exit 1
fi

echo "Rolling back ${STARFORGE_DEPLOY_ENVIRONMENT}."
bash -o pipefail -c "${STARFORGE_ROLLBACK_COMMAND}"

if [[ -n "${STARFORGE_HEALTHCHECK_URL:-}" ]]; then
    curl --fail --silent --show-error --max-time 10 "${STARFORGE_HEALTHCHECK_URL}" >/dev/null
    echo "Rollback health check passed."
fi

# Send rollback notification
STARFORGE_NOTIFY_STATUS=rollback \
STARFORGE_NOTIFY_ENV="${STARFORGE_DEPLOY_ENVIRONMENT}" \
STARFORGE_NOTIFY_ACTOR="${STARFORGE_NOTIFY_ACTOR:-CI}" \
bash "$(dirname "$0")/ci-notify.sh" || true
