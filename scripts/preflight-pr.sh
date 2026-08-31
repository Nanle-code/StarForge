#!/usr/bin/env bash
#
# StarForge Preflight PR Verification Script
#
# This script mirrors CI merge gates locally to ensure that Pull Requests pass
# all required status checks and meet branch protection criteria before submission.
#
# Usage:
#   ./scripts/preflight-pr.sh            # Run standard merge gate checks
#   ./scripts/preflight-pr.sh --quick    # Run fast subset (fmt, clippy, unit tests)
#   ./scripts/preflight-pr.sh --all      # Run complete test suite and all checks
#   ./scripts/preflight-pr.sh --fix      # Auto-format code before running checks
#   ./scripts/preflight-pr.sh --help     # Display help and options
#
# Exit codes:
#   0 - All gates passed successfully
#   1 - One or more gates failed

set -uo pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# Configuration & flags
RUN_ALL_TESTS=false
QUICK_MODE=false
AUTO_FIX=false
SKIP_DENY=false
BASE_BRANCH="master"

# Counters & tracking
GATES_RUN=0
GATES_PASSED=0
GATES_FAILED=0
declare -a FAILED_GATE_NAMES=()

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            RUN_ALL_TESTS=true
            shift
            ;;
        --quick)
            QUICK_MODE=true
            shift
            ;;
        --fix)
            AUTO_FIX=true
            shift
            ;;
        --skip-deny)
            SKIP_DENY=true
            shift
            ;;
        --base)
            BASE_BRANCH="$2"
            shift 2
            ;;
        -h|--help)
            echo -e "${BOLD}StarForge PR Preflight Verification${NC}"
            echo ""
            echo "Usage: ./scripts/preflight-pr.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --quick       Run quick gate checks (fmt, clippy, unit tests, JSON contract)"
            echo "  --all         Run full test suite across entire workspace"
            echo "  --fix         Run 'cargo fmt --all' before verifying"
            echo "  --skip-deny   Skip 'cargo deny' dependency audit"
            echo "  --base <br>   Base branch to check for merge conflicts (default: master)"
            echo "  -h, --help    Show this help message"
            echo ""
            echo "Merge Gates Checked:"
            echo "  1. Git hygiene & conflict marker check"
            echo "  2. Code formatting (cargo fmt --all --check)"
            echo "  3. Compilation & MSRV compatibility (cargo check --locked)"
            echo "  4. Linting & correctness (cargo clippy --locked -- -D warnings)"
            echo "  5. CLI JSON contract stability (cargo test --test json_contract_stability)"
            echo "  6. Unit & core test suite (cargo test)"
            echo "  7. Smoke tests (cargo test --test cli_smoke)"
            echo "  8. Dependency security & licenses (cargo deny check, if installed)"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Run './scripts/preflight-pr.sh --help' for usage."
            exit 1
            ;;
    esac
done

echo -e "${BLUE}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║${BOLD}            StarForge Pull Request Preflight Gates               ${NC}${BLUE}║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Helper to run a merge gate
run_gate() {
    local gate_name="$1"
    local gate_command="$2"
    local allow_failure="${3:-false}"

    GATES_RUN=$((GATES_RUN + 1))
    echo -e "${CYAN}▶ Gate ${GATES_RUN}: ${BOLD}${gate_name}${NC}"
    echo -e "  ${BLUE}\$ ${gate_command}${NC}"

    local start_time
    start_time=$(date +%s)

    # Execute command
    if eval "$gate_command"; then
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo -e "  ${GREEN}✔ PASS${NC} (${duration}s)"
        echo ""
        GATES_PASSED=$((GATES_PASSED + 1))
        return 0
    else
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        if [ "$allow_failure" = "true" ]; then
            echo -e "  ${YELLOW}⚠ WARNING (non-blocking)${NC} (${duration}s)"
            echo ""
            return 0
        else
            echo -e "  ${RED}✖ FAIL${NC} (${duration}s)"
            echo ""
            GATES_FAILED=$((GATES_FAILED + 1))
            FAILED_GATE_NAMES+=("$gate_name")
            return 1
        fi
    fi
}

# Auto-fix formatting if requested
if [ "$AUTO_FIX" = "true" ]; then
    echo -e "${YELLOW}Auto-formatting code with cargo fmt --all...${NC}"
    cargo fmt --all
    echo ""
fi

# ==============================================================================
# Gate 1: Git Hygiene & Conflict Markers Check
# ==============================================================================
run_git_check() {
    if ! command -v git >/dev/null 2>&1; then
        echo -e "  ${YELLOW}⊘ Git not found in PATH, skipping git hygiene check${NC}"
        return 0
    fi

    if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo -e "  ${YELLOW}⊘ Not inside a git repository, skipping git hygiene check${NC}"
        return 0
    fi

    # 1. Check for active unresolved merge/rebase in progress
    local git_dir
    git_dir=$(git rev-parse --git-dir 2>/dev/null || true)
    if [ -f "$git_dir/MERGE_HEAD" ] || [ -d "$git_dir/rebase-apply" ] || [ -d "$git_dir/rebase-merge" ]; then
        echo -e "  ${RED}Error: Merge or rebase currently in progress. Please finish or abort before preflight.${NC}"
        return 1
    fi

    # 2. Check for unmerged paths in git index
    local unmerged
    unmerged=$(git ls-files -u 2>/dev/null || true)
    if [ -n "$unmerged" ]; then
        echo -e "  ${RED}Error: Unmerged paths detected in git index:${NC}"
        echo "$unmerged" | awk '{print $4}' | sort -u | sed 's/^/    /'
        return 1
    fi

    # 3. Check modified and staged files for actual conflict markers
    local changed_files
    changed_files=$(git status --porcelain 2>/dev/null | awk '{print $NF}' || true)
    if [ -n "$changed_files" ]; then
        local conflict_found=false
        while IFS= read -r file; do
            if [ -f "$file" ] && [ "$file" != "scripts/preflight-pr.sh" ]; then
                if grep -q -E '^<{7}( |$)|^={7}$|^>{7}( |$)' "$file" 2>/dev/null; then
                    echo -e "  ${RED}Error: Unresolved conflict marker found in modified file: $file${NC}"
                    conflict_found=true
                fi
            fi
        done <<< "$changed_files"

        if [ "$conflict_found" = "true" ]; then
            return 1
        fi
    fi

    # 4. Check base branch status
    if git rev-parse --verify "$BASE_BRANCH" >/dev/null 2>&1 || git rev-parse --verify "origin/$BASE_BRANCH" >/dev/null 2>&1; then
        local target_ref
        if git rev-parse --verify "origin/$BASE_BRANCH" >/dev/null 2>&1; then
            target_ref="origin/$BASE_BRANCH"
        else
            target_ref="$BASE_BRANCH"
        fi

        local behind_count
        behind_count=$(git rev-list --count "HEAD..$target_ref" 2>/dev/null || echo "0")
        if [ "$behind_count" -gt 0 ]; then
            echo -e "  ${YELLOW}Note: Branch is $behind_count commit(s) behind $target_ref.${NC}"
            echo -e "  ${YELLOW}Consider rebasing against $target_ref before opening PR: git rebase $target_ref${NC}"
        fi
    fi

    return 0
}

GATES_RUN=$((GATES_RUN + 1))
echo -e "${CYAN}▶ Gate ${GATES_RUN}: ${BOLD}Git Hygiene & Conflict Check${NC}"
if run_git_check; then
    echo -e "  ${GREEN}✔ PASS${NC}"
    echo ""
    GATES_PASSED=$((GATES_PASSED + 1))
else
    echo -e "  ${RED}✖ FAIL${NC}"
    echo ""
    GATES_FAILED=$((GATES_FAILED + 1))
    FAILED_GATE_NAMES+=("Git Hygiene & Conflict Check")
fi

# ==============================================================================
# Gate 2: Code Formatting
# ==============================================================================
run_gate "Rustfmt Formatting Check" "cargo fmt --all --check"

# ==============================================================================
# Gate 3: Compilation & Workspace Build Check
# ==============================================================================
run_gate "Workspace Compilation Check" "cargo check --locked --workspace"

# ==============================================================================
# Gate 4: Clippy Linting
# ==============================================================================
run_gate "Clippy Lints (-D warnings)" "cargo clippy --locked -- -D warnings"

# ==============================================================================
# Gate 5: CLI JSON Contract Stability
# ==============================================================================
run_gate "JSON Contract Stability" "cargo test --test json_contract_stability --locked"

# ==============================================================================
# Gate 6: Test Suite Execution
# ==============================================================================
if [ "$RUN_ALL_TESTS" = "true" ]; then
    run_gate "Full Test Suite" "cargo test --locked"
elif [ "$QUICK_MODE" = "true" ]; then
    run_gate "Core Unit Tests" "cargo test --lib --locked"
else
    run_gate "Unit & Integration Tests" "cargo test --lib --locked"
    run_gate "CLI Smoke Tests" "cargo test --test cli_smoke --locked"
fi

# ==============================================================================
# Gate 7: Cargo Deny Check (if installed)
# ==============================================================================
if [ "$SKIP_DENY" = "false" ]; then
    if command -v cargo-deny >/dev/null 2>&1; then
        run_gate "Cargo Deny (Security & License Audit)" "cargo deny check"
    else
        echo -e "${CYAN}▶ Gate (Optional): ${BOLD}Cargo Deny Check${NC}"
        echo -e "  ${YELLOW}⊘ Skipped (cargo-deny not installed. Install with: cargo install cargo-deny)${NC}"
        echo ""
    fi
fi

# ==============================================================================
# Summary and Exit
# ==============================================================================
echo -e "${BLUE}══════════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Preflight Summary${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════════${NC}"
echo "  Total Gates Run: $GATES_RUN"
echo -e "  ${GREEN}Gates Passed:    $GATES_PASSED${NC}"

if [ $GATES_FAILED -gt 0 ]; then
    echo -e "  ${RED}Gates Failed:    $GATES_FAILED${NC}"
    echo ""
    echo -e "${RED}${BOLD}The following gates failed:${NC}"
    for failed in "${FAILED_GATE_NAMES[@]}"; do
        echo -e "  ${RED}✖ ${failed}${NC}"
    done
    echo ""
    echo -e "${RED}Please resolve the failures above before submitting your pull request.${NC}"
    exit 1
else
    echo -e "  ${GREEN}Gates Failed:    0${NC}"
    echo ""
    echo -e "${GREEN}${BOLD}✔ All preflight merge gates passed! Your PR is ready for submission.${NC}"
    echo ""
    exit 0
fi
