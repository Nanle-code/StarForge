#!/usr/bin/env bash
# install.sh — starforge installer
#
# Downloads, verifies, and installs the latest starforge release binary.
#
# Supported platforms:
#   OS:   linux, darwin (macOS)
#   Arch: x86_64, aarch64 / arm64
#
# Environment variables (all optional):
#   INSTALL_DIR   Override the installation directory  (default: /usr/local/bin)
#
# Uninstall:
#   rm -f /usr/local/bin/starforge   # or wherever you installed it
#
# Security note:
#   The SHA-256 checksum of the downloaded archive is verified against the
#   release's SHA256SUMS.txt before the binary is extracted. The installer
#   aborts if the checksum does not match.
set -euo pipefail

REPO="Josetic224/StarForge"

# ── platform detection ────────────────────────────────────────────────────────
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64)         ARCH="x86_64" ;;
    aarch64|arm64)  ARCH="aarch64" ;;
    *)
        echo "error: unsupported architecture: $ARCH" >&2
        echo "       starforge supports x86_64 and aarch64 / arm64." >&2
        exit 1
        ;;
esac

case "$OS" in
    linux|darwin) ;;
    *)
        echo "error: unsupported operating system: $OS" >&2
        echo "       starforge supports linux and darwin (macOS)." >&2
        echo "       On Windows, download the .zip release from GitHub directly." >&2
        exit 1
        ;;
esac

# ── resolve install directory ─────────────────────────────────────────────────
# Tests and callers can override via INSTALL_DIR.
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# ── working directory (auto-cleaned on exit) ──────────────────────────────────
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

TAR_FILE="starforge-${OS}-${ARCH}.tar.gz"

# ── fetch latest release tag ──────────────────────────────────────────────────
echo "Fetching latest release for starforge..."
API_URL="https://api.github.com/repos/$REPO/releases/latest"
if ! command -v curl >/dev/null 2>&1; then
    echo "error: curl is required to download starforge." >&2; exit 1
fi

TAG=$(curl -s "$API_URL" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$TAG" ]; then
    echo "error: failed to fetch the latest release version." >&2
    echo "       Check your internet connection or visit: https://github.com/$REPO/releases" >&2
    exit 1
fi

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/$TAR_FILE"
CHECKSUM_URL="https://github.com/$REPO/releases/download/$TAG/SHA256SUMS.txt"

# ── download archive ──────────────────────────────────────────────────────────
echo "Downloading $DOWNLOAD_URL..."
curl -sL "$DOWNLOAD_URL" -o "$WORK_DIR/$TAR_FILE" || { echo "error: download failed." >&2; exit 1; }

# ── download checksums ────────────────────────────────────────────────────────
echo "Downloading checksum file..."
curl -sL "$CHECKSUM_URL" -o "$WORK_DIR/checksums.txt" || { echo "error: checksum download failed." >&2; exit 1; }

# ── verify checksum ───────────────────────────────────────────────────────────
echo "Verifying checksum..."
if command -v sha256sum >/dev/null 2>&1; then
    grep "$TAR_FILE" "$WORK_DIR/checksums.txt" | (cd "$WORK_DIR" && sha256sum -c -)
elif command -v shasum >/dev/null 2>&1; then
    grep "$TAR_FILE" "$WORK_DIR/checksums.txt" | (cd "$WORK_DIR" && shasum -a 256 -c -)
else
    echo "warning: no sha256 checksum tool found — skipping verification." >&2
fi

# ── extract and install ───────────────────────────────────────────────────────
echo "Extracting..."
tar -xzf "$WORK_DIR/$TAR_FILE" -C "$WORK_DIR"

echo "Installing to $INSTALL_DIR..."
if [ -w "$INSTALL_DIR" ]; then
    mv -f "$WORK_DIR/starforge" "$INSTALL_DIR/"
else
    sudo mv -f "$WORK_DIR/starforge" "$INSTALL_DIR/"
fi
chmod +x "$INSTALL_DIR/starforge"

# WORK_DIR is removed by the EXIT trap.
echo "starforge $TAG installed successfully!"
echo "Run 'starforge --version' to verify."
echo ""
echo "To uninstall: rm -f $INSTALL_DIR/starforge"
