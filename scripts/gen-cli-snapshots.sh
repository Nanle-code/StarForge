#!/usr/bin/env bash
#
# Regenerate the golden CLI output snapshots under tests/cmd/.
#
# The corpus is driven by `tests/cli_golden.rs`. Each `tests/cmd/<case>.toml`
# describes one `starforge` invocation, and the observed stdout / stderr / exit
# code are stored next to it as `<case>.stdout`, `<case>.stderr` and
# `<case>.code`. The test compares against those files; setting
# `TRYCMD=overwrite` (mirroring trycmd) rewrites them instead.
#
# Use this after an intentional change to help text, success output, or error
# output. Always review the diff before committing:
#
#   git diff -- tests/cmd
#
# A missing snapshot is generated automatically (and the case passes), so this
# script is only needed to refresh existing snapshots or to populate the full
# `--help` corpus on first checkout.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "Regenerating golden CLI snapshots under tests/cmd/ ..."
TRYCMD=overwrite cargo test --locked --test cli_golden -- --nocapture
echo
echo "Done. Review the snapshot diff before committing:"
echo "  git diff -- tests/cmd"
