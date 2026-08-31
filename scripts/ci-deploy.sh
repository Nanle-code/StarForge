#!/usr/bin/env bash
set -euo pipefail

: "${STARFORGE_DEPLOY_ENVIRONMENT:?Set STARFORGE_DEPLOY_ENVIRONMENT to the target environment.}"
: "${STARFORGE_DEPLOY_COMMAND:?Set STARFORGE_DEPLOY_COMMAND to the protected deployment command.}"

if [[ "${STARFORGE_DEPLOY_ENVIRONMENT}" == "production" && "${STARFORGE_DEPLOY_APPROVED:-}" != "true" ]]; then
    echo "Production deployment requires STARFORGE_DEPLOY_APPROVED=true."
    exit 1
fi

echo "Deploying to ${STARFORGE_DEPLOY_ENVIRONMENT}."
if bash -o pipefail -c "${STARFORGE_DEPLOY_COMMAND}"; then
    DEPLOY_STATUS=success
else
    DEPLOY_STATUS=failure
fi

if [[ -n "${STARFORGE_HEALTHCHECK_URL:-}" && "${DEPLOY_STATUS}" == "success" ]]; then
    max_attempts="${STARFORGE_HEALTHCHECK_ATTEMPTS:-12}"
    interval_seconds="${STARFORGE_HEALTHCHECK_INTERVAL_SECONDS:-10}"

    for ((attempt = 1; attempt <= max_attempts; attempt++)); do
        if curl --fail --silent --show-error --max-time 10 "${STARFORGE_HEALTHCHECK_URL}" >/dev/null; then
            echo "Deployment health check passed."
            break
        fi

        echo "Health check ${attempt}/${max_attempts} failed."
        if (( attempt == max_attempts )); then
            echo "Deployment completed but did not become healthy. Run the rollback job."
            DEPLOY_STATUS=failure
        fi
        sleep "${interval_seconds}"
    done
fi

# Send deployment notification
STARFORGE_NOTIFY_STATUS="${DEPLOY_STATUS}" \
STARFORGE_NOTIFY_ENV="${STARFORGE_DEPLOY_ENVIRONMENT}" \
STARFORGE_NOTIFY_ACTOR="${STARFORGE_NOTIFY_ACTOR:-CI}" \
bash "$(dirname "$0")/ci-notify.sh" || true

if [[ "${DEPLOY_STATUS}" != "success" ]]; then
    exit 1
fi
