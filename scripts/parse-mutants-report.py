#!/usr/bin/env python3
"""
Parse cargo-mutants outcomes.json and write results to GITHUB_OUTPUT.

cargo-mutants writes outcomes.json under mutants.out/ (the default output
directory).  This script reads that file, computes the mutation kill rate, and
fails the step when the rate is below MIN_KILL_RATE (default 0.85).
"""
import json
import os
import sys

report_paths = [
    "mutation-reports/outcomes.json",
    "mutants.out/outcomes.json",
]

outcomes = None
for path in report_paths:
    if os.path.exists(path):
        with open(path) as f:
            outcomes = json.load(f)
        break

if outcomes is None:
    print("Warning: Mutation outcomes not found. Check test baseline.")
    sys.exit(1)

killed   = sum(1 for o in outcomes if o.get("summary") == "killed")
survived = sum(1 for o in outcomes if o.get("summary") == "survived")
total    = killed + survived
kill_rate = (killed / total) if total > 0 else 0.0

if total == 0:
    print("Warning: No mutants generated. Check examine_globs in .cargo-mutants.toml")

print(f"Kill Rate: {kill_rate:.1%} ({killed}/{total} mutants killed)")

github_output = os.environ.get("GITHUB_OUTPUT", "")
if github_output:
    with open(github_output, "a") as gh:
        gh.write(f"kill_rate={kill_rate}\n")
        gh.write(f"killed={killed}\n")
        gh.write(f"survived={survived}\n")
        gh.write(f"total={total}\n")

threshold = float(os.environ.get("MIN_KILL_RATE", "0.85"))
if kill_rate < threshold:
    print(f"FAIL: Kill rate {kill_rate:.1%} is below threshold {threshold:.1%}")
    sys.exit(1)

print(f"PASS: Kill rate {kill_rate:.1%} meets threshold {threshold:.1%}")
