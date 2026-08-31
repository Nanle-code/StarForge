#!/usr/bin/env bash
# ci-notify.sh — send a deployment notification to a webhook (Slack, Discord, Teams, custom)
#
# Required env vars:
#   STARFORGE_NOTIFY_WEBHOOK  URL of the webhook endpoint
#   STARFORGE_NOTIFY_STATUS   "success", "failure", or "rollback"
#
# Optional env vars:
#   STARFORGE_NOTIFY_ENV      Environment name (e.g. "staging", "production")
#   STARFORGE_NOTIFY_CONTRACT Contract ID that was deployed/rolled back
#   STARFORGE_NOTIFY_HASH     Transaction hash or WASM hash
#   STARFORGE_NOTIFY_ACTOR    Username or CI actor that triggered the action
#   STARFORGE_NOTIFY_URL      Link to CI run or deployment dashboard

set -euo pipefail

WEBHOOK="${STARFORGE_NOTIFY_WEBHOOK:-}"
STATUS="${STARFORGE_NOTIFY_STATUS:-unknown}"
ENV_NAME="${STARFORGE_NOTIFY_ENV:-}"
CONTRACT="${STARFORGE_NOTIFY_CONTRACT:-}"
HASH="${STARFORGE_NOTIFY_HASH:-}"
ACTOR="${STARFORGE_NOTIFY_ACTOR:-CI}"
RUN_URL="${STARFORGE_NOTIFY_URL:-}"

if [[ -z "$WEBHOOK" ]]; then
    echo "STARFORGE_NOTIFY_WEBHOOK not set — skipping notification."
    exit 0
fi

# Choose emoji and color based on status
case "$STATUS" in
    success)  EMOJI="✅"; COLOR="good"    ;;
    failure)  EMOJI="❌"; COLOR="danger"  ;;
    rollback) EMOJI="⏪"; COLOR="warning" ;;
    *)        EMOJI="ℹ️";  COLOR="#cccccc" ;;
esac

TITLE="${EMOJI} StarForge deployment ${STATUS}"
[[ -n "$ENV_NAME" ]] && TITLE="${EMOJI} StarForge ${ENV_NAME} deployment ${STATUS}"

# Build fields array
FIELDS="[]"
add_field() {
    local title="$1" value="$2" short="${3:-true}"
    FIELDS=$(echo "$FIELDS" | jq --arg t "$title" --arg v "$value" --argjson s "$short" \
        '. += [{"title": $t, "value": $v, "short": $s}]')
}

[[ -n "$ENV_NAME"  ]] && add_field "Environment"   "$ENV_NAME"  true
[[ -n "$CONTRACT"  ]] && add_field "Contract ID"   "$CONTRACT"  false
[[ -n "$HASH"      ]] && add_field "Hash"          "$HASH"      false
[[ -n "$ACTOR"     ]] && add_field "Triggered by"  "$ACTOR"     true
[[ -n "$RUN_URL"   ]] && add_field "Details"       "$RUN_URL"   false

PAYLOAD=$(jq -n \
    --arg text "$TITLE" \
    --arg color "$COLOR" \
    --argjson fields "$FIELDS" \
    '{text: $text, attachments: [{color: $color, fields: $fields}]}')

HTTP_STATUS=$(curl \
    --silent \
    --output /dev/null \
    --write-out "%{http_code}" \
    --max-time 10 \
    --header "Content-Type: application/json" \
    --data "$PAYLOAD" \
    "$WEBHOOK")

if [[ "$HTTP_STATUS" -ge 200 && "$HTTP_STATUS" -lt 300 ]]; then
    echo "Notification sent (HTTP $HTTP_STATUS)."
else
    echo "Notification webhook returned HTTP $HTTP_STATUS." >&2
    exit 1
fi
