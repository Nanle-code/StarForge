#!/usr/bin/env bash
#
# StarForge Homebrew Formula Updater
#
# This script updates the Homebrew formula with new version, URLs, and checksums
# for all supported platforms. It downloads release artifacts, computes checksums,
# and updates the formula file in place.
#
# Usage:
#   ./scripts/update-homebrew-formula.sh <version>
#
# Example:
#   ./scripts/update-homebrew-formula.sh 0.2.0
#
# Environment variables:
#   GITHUB_REPOSITORY - Repository in format "owner/repo" (default: from git remote)
#   FORMULA_PATH      - Path to formula file (default: packaging/homebrew/starforge.rb)
#   DRY_RUN           - If set to "1", only print the updated formula without writing

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
VERSION="${1:-}"
GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-$(git config --get remote.origin.url | sed -E 's/.*github.com[:/](.+)\.git/\1/')}"
FORMULA_PATH="${FORMULA_PATH:-packaging/homebrew/starforge.rb}"
DRY_RUN="${DRY_RUN:-0}"

# Platform configurations matching the formula
PLATFORMS=(
    "darwin_x86_64:starforge-darwin-x86_64.tar.gz:OS.mac? && Hardware::CPU.intel?"
    "darwin_aarch64:starforge-darwin-aarch64.tar.gz:OS.mac? && Hardware::CPU.arm?"
    "linux_x86_64:starforge-linux-x86_64.tar.gz:OS.linux? && Hardware::CPU.intel?"
    "linux_aarch64:starforge-linux-aarch64.tar.gz:OS.linux? && Hardware::CPU.arm?"
)

# Base URL for release artifacts
BASE_URL="https://github.com/${GITHUB_REPOSITORY}/releases/download/v${VERSION}"

# Temporary directory for downloads
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

# Validate version format (semver-like: major.minor.patch with optional pre-release)
validate_version() {
    local version="$1"
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]]; then
        log_error "Invalid version format: '$version'. Expected semver (e.g., 1.2.3 or 1.2.3-rc.1)"
        return 1
    fi
    return 0
}

# Check if formula file exists
check_formula_exists() {
    if [[ ! -f "$FORMULA_PATH" ]]; then
        log_error "Formula file not found: $FORMULA_PATH"
        return 1
    fi
    return 0
}

# Download artifact and compute sha256
download_and_checksum() {
    local archive_name="$1"
    local url="${BASE_URL}/${archive_name}"
    local output_path="${TMPDIR}/${archive_name}"

    log_info "Downloading $archive_name from $url"

    if ! curl -fL --retry 3 --retry-delay 2 -o "$output_path" "$url"; then
        log_error "Failed to download $archive_name from $url"
        log_error "Check that the release exists and has the expected artifacts"
        return 1
    fi

    if [[ ! -s "$output_path" ]]; then
        log_error "Downloaded file is empty: $output_path"
        return 1
    fi

    local sha256
    sha256=$(sha256sum "$output_path" | cut -d' ' -f1)

    if [[ -z "$sha256" || "$sha256" == " " ]]; then
        log_error "Failed to compute sha256 for $archive_name"
        return 1
    fi

    log_success "Downloaded $archive_name (sha256: ${sha256:0:16}...)"
    echo "$sha256"
}

# Update the formula file using Ruby for robust parsing
update_formula() {
    local version="$1"
    shift
    local -A checksums=("$@")

    log_info "Updating formula: $FORMULA_PATH"

    # Build Ruby script as a temp file to avoid quoting issues
    local ruby_script="${TMPDIR}/update_formula.rb"
    
    cat > "$ruby_script" << 'RUBY_EOF'
require 'pathname'

formula_path = Pathname.new(ENV['FORMULA_PATH'])
content = formula_path.read

# Update version
content.gsub!(/^  version \".*\"/, "  version \"#{ENV['VERSION']}\"")

# Platform data
platforms = [
  {key: 'darwin_x86_64', cond: 'OS.mac? && Hardware::CPU.intel?', archive: 'starforge-darwin-x86_64.tar.gz'},
  {key: 'darwin_aarch64', cond: 'OS.mac? && Hardware::CPU.arm?', archive: 'starforge-darwin-aarch64.tar.gz'},
  {key: 'linux_x86_64', cond: 'OS.linux? && Hardware::CPU.intel?', archive: 'starforge-linux-x86_64.tar.gz'},
  {key: 'linux_aarch64', cond: 'OS.linux? && Hardware::CPU.arm?', archive: 'starforge-linux-aarch64.tar.gz'},
]

base_url = ENV['BASE_URL']

platforms.each do |p|
  sha256_var = "SHA256_" + p[:key].upcase
  sha256 = ENV[sha256_var]
  url = "#{base_url}/#{p[:archive]}"
  cond_escaped = Regexp.escape(p[:cond])

  # Update url in this platform's block
  content.gsub!(/(^[[:space:]]*(if|elsif)[[:space:]]#{cond_escaped}.*?^[[:space:]]*)url \".*?\"/m) do
    "#{$1}url \"#{url}\""
  end

  # Update sha256 in this platform's block
  content.gsub!(/(^[[:space:]]*(if|elsif)[[:space:]]#{cond_escaped}.*?^[[:space:]]*)sha256 \".*?\"/m) do
    "#{$1}sha256 \"#{sha256}\""
  end
end

# Validate Ruby syntax using ruby -c (syntax check only)
syntax_check = `echo #{content.dump} | ruby -c - 2>&1`
if $?.exitstatus != 0
  puts "SYNTAX ERROR: #{syntax_check}"
  exit 1
end

if ENV['DRY_RUN'] == '1'
  puts content
else
  formula_path.write(content)
  puts "SUCCESS: Formula updated"
end
RUBY_EOF

    # Export variables for Ruby script
    export VERSION
    export FORMULA_PATH
    export BASE_URL
    export DRY_RUN
    for platform_entry in "${PLATFORMS[@]}"; do
        IFS=':' read -r platform archive_name ruby_cond <<< "$platform_entry"
        sha256_var="SHA256_${platform^^}"
        export "$sha256_var=${checksums[$sha256_var]}"
    done

    if ! ruby "$ruby_script"; then
        log_error "Failed to update formula"
        return 1
    fi

    log_success "Formula updated: $FORMULA_PATH"
}

# Smoke test: verify formula can be installed with brew
smoke_test_formula() {
    local formula_path="$1"
    log_info "Running Homebrew smoke test..."

    # Check if brew is available
    if ! command -v brew &> /dev/null; then
        log_warn "Homebrew not available in this environment, skipping smoke test"
        log_warn "Set up a macOS or Linux runner with Homebrew to enable this test"
        return 0
    fi

    # Try to install from the local formula file
    # Note: This is a binary formula (downloads pre-built binaries), so we don't use --build-from-source
    log_info "Testing: brew install --formula $formula_path"
    if brew install --formula "$formula_path" 2>&1; then
        log_success "Homebrew install succeeded"
        
        # Test the binary works
        if brew test starforge 2>&1; then
            log_success "Homebrew test passed"
        else
            log_error "Homebrew test failed"
            return 1
        fi
    else
        log_error "Homebrew install failed"
        return 1
    fi
}

# Main execution
main() {
    log_info "StarForge Homebrew Formula Updater"
    log_info "Repository: $GITHUB_REPOSITORY"
    log_info "Formula: $FORMULA_PATH"
    log_info "Version: $VERSION"

    # Validate inputs
    if [[ -z "$VERSION" ]]; then
        log_error "Usage: $0 <version>"
        log_error "Example: $0 0.2.0"
        exit 1
    fi

    if ! validate_version "$VERSION"; then
        exit 1
    fi

    if ! check_formula_exists; then
        exit 1
    fi

    # Download all artifacts and compute checksums
    declare -A checksums
    for platform_entry in "${PLATFORMS[@]}"; do
        IFS=':' read -r platform archive_name ruby_cond <<< "$platform_entry"
        
        sha256=$(download_and_checksum "$archive_name") || exit 1
        sha256_var="SHA256_${platform^^}"
        checksums["$sha256_var"]="$sha256"
    done

    # Update the formula
    update_formula "$VERSION" "${checksums[@]}" || exit 1

    # Run smoke test if not dry run
    if [[ "$DRY_RUN" != "1" ]]; then
        smoke_test_formula "$FORMULA_PATH" || exit 1
    fi

    log_success "All done! Formula updated for version $VERSION"
}

main "$@"