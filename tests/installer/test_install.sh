#!/usr/bin/env bash
# tests/installer/test_install.sh
#
# Installer test suite for install.sh.
#
# Scenarios covered:
#   Primary flows  : clean install, upgrade (binary already present)
#   Boundary cases : success message contains version, no temp files left,
#                    checksum download fails
#   Failure paths  : bad checksum, download failure, empty tag,
#                    unsupported OS, unsupported architecture
#
# Tests run in isolated mktemp directories — no network, no root, no
# side-effects.  Each test injects stub implementations for the three curl
# calls in install.sh by embedding them directly into the patched script.
#
# Usage:
#   bash tests/installer/test_install.sh
#   VERBOSE=1 bash tests/installer/test_install.sh

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'
RESET='\033[0m'; BOLD='\033[1m'
pass() { echo -e "${GREEN}[PASS]${RESET} $1"; }
fail() { echo -e "${RED}[FAIL]${RESET} $1"; FAILURES=$((FAILURES + 1)); }

FAILURES=0
TESTS_RUN=0

TMPDIR_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_ROOT"' EXIT

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALL_SH="$REPO_ROOT/install.sh"

# ---------------------------------------------------------------------------
# Stub implementations injected into every patched installer.
# STUB_* environment variables control their behaviour at runtime.
# ---------------------------------------------------------------------------
_STUB_BODY='
# ---------- injected stubs ----------
_stub_curl_download() {
    # Called instead of: curl -sL "$DOWNLOAD_URL" -o "$WORK_DIR/$TAR_FILE"
    local dest="$WORK_DIR/$TAR_FILE"
    if [ "${STUB_DOWNLOAD_FAIL:-0}" = "1" ]; then
        echo "error: download failed." >&2; return 1
    fi
    printf "#!/bin/sh\necho starforge-stub-ok\n" > "$WORK_DIR/starforge"
    chmod +x "$WORK_DIR/starforge"
    tar -czf "$dest" -C "$WORK_DIR" starforge
    rm "$WORK_DIR/starforge"
}
_stub_curl_checksum() {
    # Called instead of: curl -sL "$CHECKSUM_URL" -o "$WORK_DIR/checksums.txt"
    if [ "${STUB_CHECKSUM_DL_FAIL:-0}" = "1" ]; then
        echo "error: checksum download failed." >&2; return 1
    fi
    local dest="$WORK_DIR/checksums.txt"
    if [ "${STUB_BAD_CHECKSUM:-0}" = "1" ]; then
        printf "0000000000000000000000000000000000000000000000000000000000000000  %s\n" "$TAR_FILE" > "$dest"
    else
        if command -v sha256sum >/dev/null 2>&1; then
            (cd "$WORK_DIR"; sha256sum "$TAR_FILE") > "$dest"
        elif command -v shasum >/dev/null 2>&1; then
            (cd "$WORK_DIR"; shasum -a 256 "$TAR_FILE") > "$dest"
        else
            printf "" > "$dest"
        fi
    fi
}
# ---------- /stubs ----------
'

# Build a patched copy of install.sh with network calls replaced by stubs.
# The stubs are prepended directly so the child bash process has them.
make_patched_installer() {
    local work_dir="$1"
    local patched="$work_dir/install_patched.sh"

    {
        printf "#!/usr/bin/env bash\nset -euo pipefail\n"
        printf '%s\n' "$_STUB_BODY"
        # Strip the original shebang + set line, then patch curl calls.
        sed \
            -e '1,2d' \
            -e 's|TAG=\$(curl -s.*|TAG="${STUB_TAG:-}"|' \
            -e 's|curl -sL "\$DOWNLOAD_URL" -o "\$WORK_DIR/\$TAR_FILE".*|_stub_curl_download|' \
            -e 's|curl -sL "\$CHECKSUM_URL" -o "\$WORK_DIR/checksums.txt".*|_stub_curl_checksum|' \
            "$INSTALL_SH"
    } > "$patched"

    chmod +x "$patched"
}

run_test() {
    local name="$1"
    TESTS_RUN=$((TESTS_RUN + 1))
    local work_dir; work_dir="$(mktemp -d "$TMPDIR_ROOT/test_XXXXXX")"
    make_patched_installer "$work_dir"
    if (
        "$name" "$work_dir"
    ) 2>&1 | ([ "${VERBOSE:-0}" = "1" ] && cat || grep -E "^(error|FAIL|SKIP)" || true); then
        pass "$name"
    else
        fail "$name"
    fi
}

# ── 1. Primary flow: clean install succeeds ───────────────────────────────────
test_clean_install_succeeds() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    STUB_TAG="v9.9.9" INSTALL_DIR="$fake_bin" bash "$work_dir/install_patched.sh"
    [ -f "$fake_bin/starforge" ] || { echo "FAIL: binary not installed"; exit 1; }
    [ -x "$fake_bin/starforge" ] || { echo "FAIL: binary not executable"; exit 1; }
}

# ── 2. Boundary: upgrade replaces existing binary ─────────────────────────────
test_upgrade_replaces_existing_binary() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    printf '#!/bin/sh\necho "starforge 1.0.0"\n' > "$fake_bin/starforge"
    chmod +x "$fake_bin/starforge"
    local before; before="$("$fake_bin/starforge")"
    STUB_TAG="v9.9.9" INSTALL_DIR="$fake_bin" bash "$work_dir/install_patched.sh"
    local after; after="$("$fake_bin/starforge")"
    [ "$before" != "$after" ] || { echo "FAIL: binary NOT replaced on upgrade"; exit 1; }
}

# ── 3. Failure: checksum mismatch aborts install ──────────────────────────────
test_checksum_mismatch_fails() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        echo "SKIP: no sha256 tool available"; return 0
    fi
    local rc=0
    STUB_TAG="v9.9.9" STUB_BAD_CHECKSUM=1 INSTALL_DIR="$fake_bin" \
        bash "$work_dir/install_patched.sh" 2>&1 || rc=$?
    [ "$rc" -ne 0 ] || { echo "FAIL: expected non-zero exit on bad checksum"; exit 1; }
    [ ! -f "$fake_bin/starforge" ] || { echo "FAIL: binary installed despite bad checksum"; exit 1; }
}

# ── 4. Failure: download failure → non-zero exit ──────────────────────────────
test_download_failure_exits_nonzero() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    local rc=0
    STUB_TAG="v9.9.9" STUB_DOWNLOAD_FAIL=1 INSTALL_DIR="$fake_bin" \
        bash "$work_dir/install_patched.sh" 2>&1 || rc=$?
    [ "$rc" -ne 0 ] || { echo "FAIL: expected non-zero exit on download failure"; exit 1; }
}

# ── 5. Failure: no release tag → non-zero exit ────────────────────────────────
test_empty_tag_exits_nonzero() {
    local work_dir="$1"
    local rc=0
    STUB_TAG="" INSTALL_DIR="$work_dir/fake_bin" \
        bash "$work_dir/install_patched.sh" 2>&1 || rc=$?
    [ "$rc" -ne 0 ] || { echo "FAIL: expected non-zero exit when TAG is empty"; exit 1; }
}

# ── 6. Failure: unsupported OS → non-zero exit ────────────────────────────────
test_unsupported_os_exits_nonzero() {
    local work_dir="$1"
    cat > "$work_dir/uname" <<'EOF'
#!/bin/sh
if [ "$1" = "-s" ]; then echo "FreeBSD"; else /usr/bin/uname "$@"; fi
EOF
    chmod +x "$work_dir/uname"
    local rc=0
    STUB_TAG="v9.9.9" PATH="$work_dir:$PATH" INSTALL_DIR="$work_dir/fake_bin" \
        bash "$work_dir/install_patched.sh" 2>&1 || rc=$?
    [ "$rc" -ne 0 ] || { echo "FAIL: expected non-zero exit for unsupported OS"; exit 1; }
}

# ── 7. Failure: unsupported architecture → non-zero exit ─────────────────────
test_unsupported_arch_exits_nonzero() {
    local work_dir="$1"
    cat > "$work_dir/uname" <<'EOF'
#!/bin/sh
if [ "$1" = "-m" ]; then echo "mips"; else /usr/bin/uname "$@"; fi
EOF
    chmod +x "$work_dir/uname"
    local rc=0
    STUB_TAG="v9.9.9" PATH="$work_dir:$PATH" INSTALL_DIR="$work_dir/fake_bin" \
        bash "$work_dir/install_patched.sh" 2>&1 || rc=$?
    [ "$rc" -ne 0 ] || { echo "FAIL: expected non-zero exit for unsupported arch"; exit 1; }
}

# ── 8. Boundary: no temp files left behind after success ──────────────────────
test_no_temp_files_left_after_success() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    STUB_TAG="v9.9.9" INSTALL_DIR="$fake_bin" bash "$work_dir/install_patched.sh"
    # install.sh uses an EXIT trap on its own WORK_DIR; verify nothing stray
    # landed in the current directory or the test work_dir root.
    local stray
    stray="$(find "$work_dir" -maxdepth 1 \( -name "*.tar.gz" -o -name "checksums.txt" \) 2>/dev/null | head -1)"
    [ -z "$stray" ] || { echo "FAIL: temp file left behind: $stray"; exit 1; }
}

# ── 9. Boundary: success message contains installed version ───────────────────
test_success_message_contains_version() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    local output
    output="$(STUB_TAG="v9.9.9" INSTALL_DIR="$fake_bin" bash "$work_dir/install_patched.sh" 2>&1)"
    echo "$output" | grep -q "v9.9.9" || { echo "FAIL: version not in output"; echo "$output"; exit 1; }
}

# ── 10. Boundary: checksum download failure → non-zero exit ───────────────────
test_checksum_download_failure_exits_nonzero() {
    local work_dir="$1"
    local fake_bin="$work_dir/fake_bin"; mkdir -p "$fake_bin"
    local rc=0
    STUB_TAG="v9.9.9" STUB_CHECKSUM_DL_FAIL=1 INSTALL_DIR="$fake_bin" \
        bash "$work_dir/install_patched.sh" 2>&1 || rc=$?
    [ "$rc" -ne 0 ] || { echo "FAIL: expected non-zero exit when checksum download fails"; exit 1; }
}

# ── runner ────────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}StarForge installer tests${RESET}"
echo "────────────────────────────────────────────────"

run_test test_clean_install_succeeds
run_test test_upgrade_replaces_existing_binary
run_test test_checksum_mismatch_fails
run_test test_download_failure_exits_nonzero
run_test test_empty_tag_exits_nonzero
run_test test_unsupported_os_exits_nonzero
run_test test_unsupported_arch_exits_nonzero
run_test test_no_temp_files_left_after_success
run_test test_success_message_contains_version
run_test test_checksum_download_failure_exits_nonzero

echo "────────────────────────────────────────────────"
echo -e "${BOLD}Results: $TESTS_RUN tests, $FAILURES failed${RESET}"
echo ""
[ "$FAILURES" -eq 0 ]
