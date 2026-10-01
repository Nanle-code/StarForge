#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
STARFORGE_BIN="${STARFORGE_BIN:-}"

if [[ -z "$STARFORGE_BIN" ]]; then
    if [[ -x "$REPO_ROOT/target/debug/starforge" ]]; then
        STARFORGE_BIN="$REPO_ROOT/target/debug/starforge"
    elif [[ -x "$REPO_ROOT/target/release/starforge" ]]; then
        STARFORGE_BIN="$REPO_ROOT/target/release/starforge"
    elif command -v starforge >/dev/null 2>&1; then
        STARFORGE_BIN="$(command -v starforge)"
    else
        echo "StarForge binary not found; build it with cargo build --locked or set STARFORGE_BIN." >&2
        exit 1
    fi
elif [[ "$STARFORGE_BIN" != /* && "$STARFORGE_BIN" == */* ]]; then
    STARFORGE_BIN="$REPO_ROOT/$STARFORGE_BIN"
fi

if [[ "$STARFORGE_BIN" == */* && ! -x "$STARFORGE_BIN" ]]; then
    echo "StarForge binary is not executable: $STARFORGE_BIN" >&2
    exit 1
fi

TEMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEMP_ROOT"' EXIT
export STARFORGE_HOME="$TEMP_ROOT/home"
export HOME="$STARFORGE_HOME"
export USERPROFILE="$STARFORGE_HOME"
mkdir -p "$STARFORGE_HOME" "$TEMP_ROOT/work"
cd "$TEMP_ROOT/work"

run_starforge() {
    "$STARFORGE_BIN" "$@"
}

run_starforge --version
tutorial_list="$(run_starforge tool tutorial list)"
if [[ "$tutorial_list" != *"onboarding-15-minute"* ]]; then
    echo "Onboarding tutorial was not listed from the clean working directory." >&2
    exit 1
fi
run_starforge tool tutorial start onboarding-15-minute --demo

run_starforge tool tutorial next
run_starforge wallet create onboarding
run_starforge tool tutorial next
run_starforge new contract onboarding-contract --template hello-world
run_starforge tool tutorial next
run_starforge network simulate run --scenario simple-counter
completion_output="$(run_starforge tool tutorial next 2>&1)"
printf '%s\n' "$completion_output"
if [[ "$completion_output" != *"Tutorial complete!"* ]]; then
    echo "Onboarding did not report completion." >&2
    exit 1
fi

status_file="$STARFORGE_HOME/.starforge/tutorial_status.json"
if [[ ! -f "$status_file" ]]; then
    echo "Tutorial progress was not saved locally: $status_file" >&2
    exit 1
fi
completed_steps="$(sed -n '/"completed_steps"/,/]/p' "$status_file" | tr -cd '0-9')"
if [[ "$completed_steps" != "0123" ]]; then
    echo "Expected all four checkpoints in local progress; found: $completed_steps" >&2
    exit 1
fi

echo "Offline onboarding completed and all checkpoints were saved locally."
