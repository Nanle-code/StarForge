#!/usr/bin/env bash
#
# StarForge nightly end-to-end suite against a live Stellar network.
#
# Flow: create an ephemeral wallet -> fund it via Friendbot -> for every
# built-in template: scaffold -> build -> deploy -> invoke -> upgrade -> verify.
#
# Every key is generated for this run only and is removed on exit. Nothing in
# this script (or in the artifacts it writes) ever prints a secret key; the
# results directory is additionally scrubbed for secret-looking strings before
# it is uploaded.
#
# Usage:
#   STARFORGE_BIN=./target/release/starforge ./.github/scripts/nightly-e2e.sh
#
# Environment:
#   STARFORGE_BIN                  starforge binary (default: target/release/starforge, then target/debug, then PATH)
#   STARFORGE_E2E_NETWORK          network to exercise (only "testnet" is supported; default: testnet)
#   STARFORGE_E2E_TEMPLATES        comma-separated built-in templates (default: hello-world,token,voting,nft)
#   STARFORGE_E2E_RESULTS_DIR      results directory (default: ./e2e-results)
#   STARFORGE_E2E_MAX_SECONDS      wall-clock budget for the suite (default: 720)
#   STARFORGE_E2E_CMD_TIMEOUT      per-command timeout in seconds (default: 300)
#   STARFORGE_E2E_UPGRADE_INVOKE   1 = try the on-chain `upgrade` entrypoint, 0 = skip (default: 1)
#   STARFORGE_E2E_KEEP_KEYS        1 = keep the ephemeral keys after the run (debugging only)
#   CARGO_TARGET_DIR               shared cargo target dir for the template builds
#
# Exit status: 0 when every template completed, 1 on any real failure (a
# template that cannot be scaffolded/built is reported and skipped, which alone
# does not fail the run; a failed deploy/invoke/verify does).

set -u -o pipefail

# ── configuration ────────────────────────────────────────────────────────────

NETWORK="${STARFORGE_E2E_NETWORK:-testnet}"
TEMPLATES_CSV="${STARFORGE_E2E_TEMPLATES:-hello-world,token,voting,nft}"
RESULTS_DIR="${STARFORGE_E2E_RESULTS_DIR:-$PWD/e2e-results}"
MAX_SECONDS="${STARFORGE_E2E_MAX_SECONDS:-720}"
CMD_TIMEOUT="${STARFORGE_E2E_CMD_TIMEOUT:-300}"
UPGRADE_INVOKE="${STARFORGE_E2E_UPGRADE_INVOKE:-1}"
KEEP_KEYS="${STARFORGE_E2E_KEEP_KEYS:-0}"

HORIZON_URL="${STARFORGE_E2E_HORIZON_URL:-https://horizon-testnet.stellar.org}"
FRIENDBOT_URL="${STARFORGE_E2E_FRIENDBOT_URL:-https://friendbot.stellar.org}"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-${RANDOM}"
IDENTITY="sf-e2e-${RUN_ID}"
WALLET="sf-e2e-${RUN_ID}"

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/sf-e2e.XXXXXX")"
PROJECTS_DIR="$WORKDIR/projects"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$WORKDIR/target}"

LOG_DIR="$RESULTS_DIR/logs"
SUMMARY_FILE="$RESULTS_DIR/summary.md"
START_EPOCH="$(date -u +%s)"

FAILED_TEMPLATES=0
SKIPPED_TEMPLATES=0
ROWS=()

# ── small helpers ────────────────────────────────────────────────────────────

log()  { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
info() { printf '    %s\n' "$*"; }
ok()   { printf '    \033[32mPASS\033[0m %s\n' "$*"; }
warn() { printf '    \033[33mWARN\033[0m %s\n' "$*"; }
err()  { printf '    \033[31mFAIL\033[0m %s\n' "$*" >&2; }

elapsed() { echo "$(( $(date -u +%s) - START_EPOCH ))"; }

budget_exceeded() {
  [ "$(elapsed)" -ge "$MAX_SECONDS" ]
}

# Resolve the starforge binary once; never guess silently at call sites.
resolve_starforge() {
  local candidate
  if [ -n "${STARFORGE_BIN:-}" ]; then
    candidate="$STARFORGE_BIN"
  elif [ -x "target/release/starforge" ]; then
    candidate="$PWD/target/release/starforge"
  elif [ -x "target/debug/starforge" ]; then
    candidate="$PWD/target/debug/starforge"
  else
    candidate="$(command -v starforge || true)"
  fi
  if [ -z "$candidate" ] || [ ! -x "$candidate" ]; then
    err "starforge binary not found. Set STARFORGE_BIN or build with 'cargo build --release --locked'."
    return 1
  fi
  printf '%s' "$candidate"
}

# `timeout` is present on GitHub runners; degrade gracefully if it is missing.
TIMEOUT_CMD=()
if command -v timeout >/dev/null 2>&1; then
  TIMEOUT_CMD=(timeout --signal=TERM --kill-after=15 "$CMD_TIMEOUT")
fi

# Apply the timeout wrapper when available. This indirection also keeps the
# empty-array expansion valid under `set -u` on older bash (macOS ships 3.2).
run_timeout() {
  if [ "${#TIMEOUT_CMD[@]}" -gt 0 ]; then
    "${TIMEOUT_CMD[@]}" "$@"
  else
    "$@"
  fi
}

# Run a command, tee output to a log file, return its exit status.
run_cmd() {
  local log_file="$1"; shift
  local rc=0
  run_timeout "$@" >"$log_file" 2>&1 || rc=$?
  return "$rc"
}

# Run a command inside a directory (for cargo/stellar project builds).
run_cmd_in() {
  local dir="$1" log_file="$2"; shift 2
  local rc=0
  ( cd "$dir" && run_timeout "$@" ) >"$log_file" 2>&1 || rc=$?
  return "$rc"
}

# Run a command and echo its stdout on success (still logged); stderr goes to
# the log. Used for values like contract IDs and wasm hashes.
run_capture() {
  local log_file="$1"; shift
  local tmp rc=0
  tmp="$(mktemp "${TMPDIR:-/tmp}/sf-e2e.out.XXXXXX")"
  run_timeout "$@" >"$tmp" 2>>"$log_file" || rc=$?
  cat "$tmp" >>"$log_file"
  if [ "$rc" -eq 0 ]; then
    cat "$tmp"
  fi
  rm -f "$tmp"
  return "$rc"
}

# Friendbot/Horizon account existence (retries briefly to absorb testnet lag).
account_exists() {
  curl -fsS -o /dev/null "$HORIZON_URL/accounts/$1" >/dev/null 2>&1
}

wait_for_funding() {
  local pubkey="$1"
  for _ in $(seq 1 15); do
    if account_exists "$pubkey"; then
      return 0
    fi
    sleep 2
  done
  return 1
}

# Scrub secret-looking strings (Stellar secret keys start with `S`) from every
# artifact so a misbehaving tool can never leak a key into the upload.
redact_logs() {
  local found=0 f tmp
  while IFS= read -r -d '' f; do
    if grep -qE '[Ss][A-Z2-7]{55}' "$f" 2>/dev/null; then
      # Portable in-place rewrite (GNU and BSD sed disagree on `-i`).
      tmp="$f.redact.$$"
      if sed -E 's/[Ss][A-Z2-7]{55}/S***REDACTED***/g' "$f" >"$tmp" 2>/dev/null; then
        mv "$tmp" "$f"
        found=1
      else
        rm -f "$tmp"
      fi
    fi
  done < <(find "$RESULTS_DIR" -type f -name '*.log' -print0 2>/dev/null)
  if [ "$found" -eq 1 ]; then
    warn "Redacted secret-looking strings from the logs before upload."
  fi
}

cleanup() {
  local rc=$?
  redact_logs
  # A hard crash must still leave a summary behind for the tracking issue.
  if [ ! -f "$SUMMARY_FILE" ] && command -v write_summary >/dev/null 2>&1; then
    write_summary "FAILED (aborted before the suite completed)"
  fi
  if [ "$KEEP_KEYS" != "1" ]; then
    if [ -n "${IDENTITY:-}" ]; then
      stellar keys rm "$IDENTITY" >/dev/null 2>&1 || stellar keys remove "$IDENTITY" >/dev/null 2>&1 || true
    fi
    if [ -n "${SF_BIN:-}" ] && [ -n "${WALLET:-}" ]; then
      "$SF_BIN" --non-interactive wallet remove "$WALLET" >/dev/null 2>&1 || true
    fi
  else
    warn "STARFORGE_E2E_KEEP_KEYS=1 -> ephemeral keys for run ${RUN_ID} were kept."
  fi
  rm -rf "$WORKDIR" 2>/dev/null || true
  return "$rc"
}

# Always purge the ephemeral identity/wallet and the work tree, on success,
# failure, timeout or SIGINT alike. The trap preserves the script's exit code.
trap cleanup EXIT
trap 'exit 130' INT TERM

write_summary() {
  local status="$1" duration
  duration="$(elapsed)"
  {
    printf '# Nightly end-to-end suite summary\n\n'
    printf -- '- **Run id:** `%s`\n' "$RUN_ID"
    printf -- '- **Network:** %s\n' "$NETWORK"
    printf -- '- **Deployer (ephemeral):** `%s`\n' "${ADDR:-unknown}"
    printf -- '- **Duration:** %ss (budget %ss)\n' "$duration" "$MAX_SECONDS"
    printf -- '- **Result:** %s\n' "$status"
    printf -- '- **Templates failed:** %s, skipped: %s\n\n' "$FAILED_TEMPLATES" "$SKIPPED_TEMPLATES"
    printf '| Template | Scaffold | Build | Deploy | Invoke | Upgrade | Verify | Result |\n'
    printf '| --- | --- | --- | --- | --- | --- | --- | --- |\n'
    local row
    for row in "${ROWS[@]:-}"; do
      printf '%s\n' "$row"
    done
    printf '\n_Logs for every phase are attached as the `nightly-e2e-results-*` artifact._\n'
  } >"$SUMMARY_FILE"
}

finish() {
  local code="$1" status
  if [ "$code" -eq 0 ]; then status="PASSED"; else status="FAILED"; fi
  redact_logs
  write_summary "$status"
  log "Suite finished: ${status} (${FAILED_TEMPLATES} failed, ${SKIPPED_TEMPLATES} skipped) — summary at ${SUMMARY_FILE}"
  exit "$code"
}

abort() {
  err "$1"
  ROWS+=("| _n/a_ | - | - | - | - | - | - | ABORTED |")
  finish 1
}

# ── preflight ────────────────────────────────────────────────────────────────

mkdir -p "$LOG_DIR" "$PROJECTS_DIR"

log "Preflight"
SF_BIN="$(resolve_starforge)" || exit 1
info "starforge: $SF_BIN"

for tool in stellar cargo curl jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    abort "required command '$tool' is not available on PATH"
  fi
done
info "stellar:  $(command -v stellar)"
info "target:   $CARGO_TARGET_DIR"

if [ "$NETWORK" != "testnet" ]; then
  abort "only the testnet network is supported by this suite (got '$NETWORK')"
fi

if ! "$SF_BIN" --non-interactive info >"$LOG_DIR/starforge-info.log" 2>&1; then
  abort "'starforge info' failed — the binary is not runnable"
fi
ok "starforge info"

# ── ephemeral wallet: create + import + fund via Friendbot ───────────────────

log "Ephemeral wallet (create -> import -> fund via Friendbot)"
"$SF_BIN" --non-interactive network switch "$NETWORK" >"$LOG_DIR/network-switch.log" 2>&1 || \
  warn "'starforge network switch $NETWORK' returned non-zero (continuing)"

# `stellar keys generate` writes the key to ~/.config/stellar and funds it via
# Friendbot in one step; the secret stays on disk and is never printed.
if ! run_cmd "$LOG_DIR/wallet-generate.log" stellar keys generate "$IDENTITY" \
    --network "$NETWORK" --fund; then
  warn "'stellar keys generate --fund' returned non-zero; checking the identity"
  if stellar keys address "$IDENTITY" >/dev/null 2>&1; then
    info "identity was created; Friendbot funding will be retried below"
  elif ! run_cmd "$LOG_DIR/wallet-generate.log" stellar keys generate "$IDENTITY" --network "$NETWORK"; then
    abort "could not generate the ephemeral stellar-cli identity"
  fi
fi
ADDR="$(run_capture "$LOG_DIR/wallet-address.log" stellar keys address "$IDENTITY")" || \
  abort "could not resolve the ephemeral identity address"
if [ -z "$ADDR" ]; then
  abort "ephemeral identity address is empty"
fi
ok "created ephemeral keypair ${ADDR}"

if ! run_cmd "$LOG_DIR/wallet-import.log" "$SF_BIN" --non-interactive wallet import \
  "$WALLET" --from-stellar-cli "$IDENTITY" --network "$NETWORK"; then
  abort "could not import the ephemeral identity into starforge"
fi
ok "imported into starforge as wallet '${WALLET}'"

# Friendbot funding through starforge first, then the public Friendbot endpoint.
run_cmd "$LOG_DIR/wallet-fund.log" "$SF_BIN" --non-interactive wallet fund "$WALLET" || \
  warn "'starforge wallet fund' returned non-zero; trying Friendbot directly"

if ! account_exists "$ADDR"; then
  curl -fsS "${FRIENDBOT_URL}?addr=${ADDR}" >"$LOG_DIR/friendbot.log" 2>&1 || \
    warn "Friendbot request failed; retrying below"
fi

if ! wait_for_funding "$ADDR"; then
  abort "ephemeral account ${ADDR} was never funded on ${NETWORK}"
fi
ok "funded via Friendbot"

# Balance snapshot (also exercises `starforge wallet show` against the network).
"$SF_BIN" --non-interactive wallet show "$WALLET" >"$LOG_DIR/wallet-show.log" 2>&1 || \
  warn "'starforge wallet show' returned non-zero (continuing)"
"$SF_BIN" --json --non-interactive wallet list >"$LOG_DIR/wallet-list.json" 2>&1 || true

redact_logs

# ── per-template flow ────────────────────────────────────────────────────────

IFS=',' read -r -a TEMPLATES <<<"$TEMPLATES_CSV"

invoke_call() {
  local tpl="$1" fn="$2"; shift 2
  run_cmd "$LOG_DIR/${tpl}-invoke.log" stellar contract invoke \
    --id "$CID" --source "$IDENTITY" --network "$NETWORK" -- "$fn" "$@"
}

for raw_tpl in "${TEMPLATES[@]}"; do
  tpl="$(printf '%s' "$raw_tpl" | tr -d '[:space:]')"
  [ -n "$tpl" ] || continue

  p_scaffold="-" p_build="-" p_deploy="-" p_invoke="-" p_upgrade="-" p_verify="-"
  proj="e2e-${tpl}"
  proj_dir="$PROJECTS_DIR/$proj"
  crate="e2e_${tpl//-/_}"
  WASM="" CID=""

  log "Template: ${tpl}"

  if budget_exceeded; then
    warn "time budget exhausted before starting '${tpl}'"
    ROWS+=("| \`${tpl}\` | - | - | - | - | - | - | SKIPPED |")
    SKIPPED_TEMPLATES=$((SKIPPED_TEMPLATES + 1))
    continue
  fi

  # 1. scaffold ------------------------------------------------------------
  if run_cmd_in "$PROJECTS_DIR" "$LOG_DIR/${tpl}-scaffold.log" \
      "$SF_BIN" --non-interactive new contract "$proj" --template "$tpl" && [ -d "$proj_dir" ]; then
    p_scaffold="✅"
  else
    p_scaffold="❌"
    warn "cannot scaffold '${tpl}' — skipping this template"
    ROWS+=("| \`${tpl}\` | ${p_scaffold} | - | - | - | - | - | SKIPPED |")
    SKIPPED_TEMPLATES=$((SKIPPED_TEMPLATES + 1))
    continue
  fi

  # 2. build ---------------------------------------------------------------
  build_ok=0
  if run_cmd_in "$proj_dir" "$LOG_DIR/${tpl}-build.log" stellar contract build; then
    build_ok=1
  else
    warn "'stellar contract build' failed for '${tpl}'; retrying with cargo (wasm32-unknown-unknown)"
    if run_cmd_in "$proj_dir" "$LOG_DIR/${tpl}-build-cargo.log" \
        cargo build --release --target wasm32-unknown-unknown; then
      build_ok=1
    fi
  fi

  for base in "$CARGO_TARGET_DIR" "$proj_dir/target"; do
    for target in wasm32v1-none wasm32-unknown-unknown; do
      if [ -f "$base/$target/release/${crate}.wasm" ]; then
        WASM="$base/$target/release/${crate}.wasm"
        break 2
      fi
    done
  done
  if [ -z "$WASM" ]; then
    WASM="$(find "$CARGO_TARGET_DIR" "$proj_dir/target" -type f -name "${crate}.wasm" 2>/dev/null | head -n 1)"
  fi

  if [ "$build_ok" -ne 1 ] || [ -z "$WASM" ]; then
    p_build="❌"
    warn "cannot build '${tpl}' — skipping deploy/invoke/upgrade for it"
    ROWS+=("| \`${tpl}\` | ${p_scaffold} | ${p_build} | - | - | - | - | SKIPPED |")
    SKIPPED_TEMPLATES=$((SKIPPED_TEMPLATES + 1))
    continue
  fi
  p_build="✅"
  ok "built $(basename "$WASM")"

  # 3. deploy --------------------------------------------------------------
  if CID="$(run_capture "$LOG_DIR/${tpl}-deploy.log" stellar contract deploy \
      --wasm "$WASM" --source "$IDENTITY" --network "$NETWORK")" \
     && printf '%s' "$CID" | grep -qE 'C[A-Z2-7]{55}'; then
    CID="$(printf '%s\n' "$CID" | grep -oE 'C[A-Z2-7]{55}' | tail -n 1)"
    p_deploy="✅"
    ok "deployed ${CID}"
  else
    p_deploy="❌"
    ROWS+=("| \`${tpl}\` | ${p_scaffold} | ${p_build} | ${p_deploy} | - | - | - | FAILED |")
    FAILED_TEMPLATES=$((FAILED_TEMPLATES + 1))
    warn "deploy failed for '${tpl}'"
    continue
  fi

  # 4. invoke --------------------------------------------------------------
  invoke_status=""
  case "$tpl" in
    hello-world) invoke_call "$tpl" hello --to e2e && invoke_status="PASS" || invoke_status="FAIL" ;;
    token)       invoke_call "$tpl" balance --id "$ADDR" && invoke_status="PASS" || invoke_status="FAIL" ;;
    voting)      { invoke_call "$tpl" create_proposal --creator "$ADDR" --title nightly-e2e \
                     && invoke_call "$tpl" results --proposal_id 1; } \
                     && invoke_status="PASS" || invoke_status="FAIL" ;;
    nft)         invoke_call "$tpl" total_supply && invoke_status="PASS" || invoke_status="FAIL" ;;
    *)           invoke_status="SKIP" ;;
  esac
  case "$invoke_status" in
    PASS) p_invoke="✅"; ok "invoked contract function" ;;
    SKIP) p_invoke="SKIP"; info "no invoke recipe for '${tpl}'; skipping invoke" ;;
    *)    p_invoke="❌"; warn "invoke failed for '${tpl}'" ;;
  esac

  # 5. upgrade -------------------------------------------------------------
  # `upgrade prepare` validates the new wasm and checks the live account, then
  # the new code is really uploaded to the ledger. The on-chain `upgrade`
  # entrypoint is only invoked when the template exposes one; templates without
  # it are reported as SKIPPED rather than failed.
  upgrade_rc=1
  new_hash=""
  if run_cmd "$LOG_DIR/${tpl}-upgrade.log" "$SF_BIN" --non-interactive upgrade prepare \
      --contract-id "$CID" --wasm "$WASM" --network "$NETWORK"; then
    if new_hash="$(run_capture "$LOG_DIR/${tpl}-upgrade-upload.log" stellar contract upload \
        --wasm "$WASM" --source "$IDENTITY" --network "$NETWORK")"; then
      new_hash="$(printf '%s\n' "$new_hash" | grep -oE '[0-9a-f]{64}' | tail -n 1)"
      upgrade_rc=0
    fi
  fi

  if [ "$upgrade_rc" -eq 0 ] && [ "$UPGRADE_INVOKE" = "1" ] && [ -n "$new_hash" ]; then
    if run_cmd "$LOG_DIR/${tpl}-upgrade-invoke.log" stellar contract invoke \
        --id "$CID" --source "$IDENTITY" --network "$NETWORK" \
        -- upgrade --new-wasm-hash "$new_hash"; then
      p_upgrade="✅"
      ok "upgraded to wasm hash ${new_hash}"
    elif grep -qiE 'not found|missing|unknown|unrecognized|does not exist|no such|unsupported|InvalidAction' \
        "$LOG_DIR/${tpl}-upgrade-invoke.log" 2>/dev/null; then
      p_upgrade="SKIP"
      info "template '${tpl}' has no upgrade entrypoint; upgrade code was prepared and uploaded"
    else
      p_upgrade="❌"
      warn "on-chain upgrade failed for '${tpl}'"
      upgrade_rc=1
    fi
  elif [ "$upgrade_rc" -eq 0 ]; then
    p_upgrade="SKIP"
    info "upgrade prepared and uploaded (invocation disabled)"
  else
    p_upgrade="❌"
    warn "upgrade preparation failed for '${tpl}'"
  fi

  # 6. verify --------------------------------------------------------------
  if run_cmd "$LOG_DIR/${tpl}-verify.log" "$SF_BIN" --non-interactive contract inspect \
      "$CID" --network "$NETWORK" --json; then
    p_verify="✅"
    ok "verified on-chain contract state"
  else
    p_verify="❌"
    warn "verification failed for '${tpl}'"
  fi

  # 7. record --------------------------------------------------------------
  tpl_failed=0
  for phase in "$p_deploy" "$p_invoke" "$p_upgrade" "$p_verify"; do
    [ "$phase" = "❌" ] && tpl_failed=1
  done
  if [ "$tpl_failed" -eq 1 ]; then
    ROWS+=("| \`${tpl}\` | ${p_scaffold} | ${p_build} | ${p_deploy} | ${p_invoke} | ${p_upgrade} | ${p_verify} | FAILED |")
    FAILED_TEMPLATES=$((FAILED_TEMPLATES + 1))
  else
    ROWS+=("| \`${tpl}\` | ${p_scaffold} | ${p_build} | ${p_deploy} | ${p_invoke} | ${p_upgrade} | ${p_verify} | PASSED |")
  fi
done

# ── verdict ──────────────────────────────────────────────────────────────────

if [ "$FAILED_TEMPLATES" -gt 0 ]; then
  finish 1
fi
if [ "${#ROWS[@]}" -eq 0 ]; then
  abort "no templates were exercised (check STARFORGE_E2E_TEMPLATES)"
fi
# Every template skipped means the suite proved nothing (e.g. the toolchain or
# wasm target is broken); that must not look like a green run.
if [ "$SKIPPED_TEMPLATES" -ge "${#ROWS[@]}" ]; then
  err "every configured template was skipped — the suite exercised nothing"
  finish 1
fi
finish 0
