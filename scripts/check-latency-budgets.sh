#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# check-latency-budgets.sh
#
# Parses Criterion benchmark output (default or bencher format) and checks
# each measured latency against the project's latency budgets.  Writes a
# JSON report to stdout (or a file via --report-path).
#
# Usage:
#   # Criterion takes a single positional filter, treated as a regex:
#   cargo bench -- 'cli_cold_start|cli_command_latency' 2>&1 \
#     | tee target/criterion/latency-raw.txt
#   bash scripts/check-latency-budgets.sh \
#     --input target/criterion/latency-raw.txt \
#     --report-path target/criterion/latency-budget-report.json
#
# Environment variable overrides (same as LatencyBudgets::apply_env_overrides):
#   STARFORGE_LATENCY_BUDGET_<UPPER_LABEL>=<ms|off>
# ---------------------------------------------------------------------------
set -euo pipefail

INPUT_FILE=""
REPORT_PATH=""

# ---- Argument parsing -------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --input) INPUT_FILE="$2"; shift 2 ;;
        --report-path) REPORT_PATH="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---- Default budgets (must match LatencyBudgets::default in Rust) ----------
declare -A BUDGETS
BUDGETS[cli_cold_start_info]=500
BUDGETS[cli_cold_start_help]=350
BUDGETS[cli_cold_start_version]=300
BUDGETS[cli_wallet_list]=300
BUDGETS[cli_wallet_show]=350
BUDGETS[cli_network_show]=250
BUDGETS[cli_network_switch]=300
BUDGETS[cli_config_show]=300
BUDGETS[cli_info]=300
BUDGETS[cli_template_list]=350
BUDGETS[cli_template_search]=400
BUDGETS[cli_deploy_help]=350
BUDGETS[cli_benchmark_wasm]=500

# ---- Override budgets from environment variables ---------------------------
for label in "${!BUDGETS[@]}"; do
    env_key="STARFORGE_LATENCY_BUDGET_${label^^}"
    if [ -n "${!env_key:-}" ]; then
        val="${!env_key}"
        if [ "$val" = "off" ] || [ "$val" = "0" ]; then
            unset BUDGETS[$label]
            echo "  [info] Budget '$label' deactivated via $env_key" >&2
        elif [[ "$val" =~ ^[0-9]+$ ]]; then
            BUDGETS[$label]=$val
            echo "  [info] Budget '$label' overridden to ${val}ms via $env_key" >&2
        fi
    fi
done

# Fix locale for bc so it always uses '.' as decimal separator.
export LC_NUMERIC=C

# ---- Read input ------------------------------------------------------------
if [ -n "$INPUT_FILE" ]; then
    RAW=$(cat "$INPUT_FILE")
else
    RAW=$(cat)
fi

# ---- Parse Criterion output ------------------------------------------------
# Default format:
#   cli_cold_start/cli_cold_start_info
#                   time:   [123.45 us 124.56 us 125.67 us]
#
# Bencher format:
#   test bench_cli_cold_start ... bench:    123456 ns/iter (+/- 12345)
#
# We try the default-format first, then fall back to bencher format.

declare -A RESULTS  # label -> median_ns

# Default format parser
while IFS= read -r line; do
    # Look for "time: [lower median upper]" patterns
    if [[ $line =~ time:[[:space:]]*\[([0-9.]+)\ ([a-z]+)\ ([0-9.]+)\ ([a-z]+)\ ([0-9.]+)\ ([a-z]+)\] ]]; then
        median_val="${BASH_REMATCH[3]}"
        median_unit="${BASH_REMATCH[4]}"
        # Convert to nanoseconds
        case "$median_unit" in
            ns) median_ns="$median_val" ;;
            us) median_ns=$(echo "$median_val * 1000" | bc 2>/dev/null || echo "${median_val}000") ;;
            ms) median_ns=$(echo "$median_val * 1000000" | bc 2>/dev/null || echo "${median_val}000000") ;;
            s)  median_ns=$(echo "$median_val * 1000000000" | bc 2>/dev/null || echo "${median_val}000000000") ;;
            *)  continue ;;
        esac
        # Get the benchmark label from a preceding line or the same line's prefix
        RESULTS["$label_accum"]=$median_ns
    fi

    # Accumulate the benchmark name from header lines before "time:" appears
    if [[ $line =~ ^([a-zA-Z_][a-zA-Z0-9_/]+)[[:space:]]*$ ]]; then
        label_accum="${BASH_REMATCH[1]}"
        # Extract just the last segment as the budget label
        label_accum="${label_accum##*/}"
    fi
done <<< "$RAW"

# Bencher format parser (fallback)
if [ ${#RESULTS[@]} -eq 0 ]; then
    while IFS= read -r line; do
        if [[ $line =~ ^test[[:space:]]+bench_([a-zA-Z_]+)[[:space:]]+.*bench:[[:space:]]+([0-9]+)[[:space:]]+ns/iter ]]; then
            label="${BASH_REMATCH[1]}"
            ns="${BASH_REMATCH[2]}"
            RESULTS["$label"]=$ns
        fi
    done <<< "$RAW"
fi

# ---- Check budgets ---------------------------------------------------------
ANY_FAIL=false
ANY_NOISY=false
CHECKS_JSON=""

check_label() {
    local label="$1"
    local budget_ms="${2:-}"
    local median_ns="${RESULTS[$label]:-}"
    local status="SKIPPED"
    local median_ms="0"
    local cv="0"

    if [ -z "$budget_ms" ]; then
        status="SKIPPED"
    elif [ -z "$median_ns" ]; then
        status="ERROR"
    else
        median_ms=$(echo "scale=3; $median_ns / 1000000" | bc 2>/dev/null || echo "0")
        if [ "$(echo "$median_ms > $budget_ms" | bc 2>/dev/null)" = "1" ]; then
            status="FAIL"
            ANY_FAIL=true
        else
            status="PASS"
        fi
    fi

    local entry
    entry=$(cat << EOF
    {
      "budget": "$label",
      "budget_max_ms": $budget_ms,
      "actual_median_ms": $median_ms,
      "cv": $cv,
      "status": "$status"
    }
EOF
)
    if [ -n "$CHECKS_JSON" ]; then
        CHECKS_JSON="$CHECKS_JSON,"
    fi
    CHECKS_JSON="$CHECKS_JSON$entry"
}

for label in "${!BUDGETS[@]}"; do
    check_label "$label" "${BUDGETS[$label]}"
done

# ---- Generate JSON report --------------------------------------------------
ALL_PASS="true"
if [ "$ANY_FAIL" = "true" ]; then
    ALL_PASS="false"
fi

REPORT=$(cat << EOF
{
  "all_pass": $ALL_PASS,
  "any_fail": $ANY_FAIL,
  "any_noisy": $ANY_NOISY,
  "checks": [
$CHECKS_JSON
  ]
}
EOF
)

# ---- Output ----------------------------------------------------------------
if [ -n "$REPORT_PATH" ]; then
    echo "$REPORT" > "$REPORT_PATH"
    echo "  [info] Latency budget report written to $REPORT_PATH" >&2
fi

echo "$REPORT"

# ---- Exit code -------------------------------------------------------------
if [ "$ANY_FAIL" = "true" ]; then
    echo "" >&2
    echo "  ❌ Latency budget violations detected!" >&2
    exit 1
else
    echo "" >&2
    echo "  ✅ All active latency budgets met." >&2
    exit 0
fi
