#!/usr/bin/env python3
"""Generate a flaky-test report from a cargo-nextest JUnit XML report.

cargo-nextest marks a test that failed and then passed on a retry as flaky by
emitting one ``<flakyFailure>``/``<flakyError>`` element per failed attempt,
with no top-level ``<failure>`` element. Tests that kept failing after every
retry carry a ``<failure>``/``<error>`` plus ``<rerunFailure>``/
``<rerunError>`` elements and are treated as real failures, not flakes.

This script turns that XML into the two CI artifacts consumed by
``.github/workflows/flaky-tests.yml``:

* ``flaky-report.md``   - human-readable summary (also usable as a step summary)
* ``flaky-report.json`` - machine-readable report for tooling / trend tracking

It annotates GitHub Actions runs when ``--annotations`` is passed, cross-references
``.github/flaky-quarantine.json`` for owner + deadline accounting, and always
emits a valid report -- including when there are zero flaky tests or when the
JUnit file is missing, so the artifact upload step never has to guess.

Exit code is 0 even when no report can be produced (the workflow must still
publish an artifact describing that) unless ``--strict`` is passed, in which
case a missing/invalid input exits 2.

Usage:
    python3 scripts/flaky-report.py \
        --input target/nextest/ci/junit.xml \
        --markdown flaky-report.md \
        --json flaky-report.json \
        --quarantine .github/flaky-quarantine.json \
        --summary "$GITHUB_STEP_SUMMARY" \
        --annotations
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import sys
import xml.etree.ElementTree as ET
from typing import Any, Dict, List, Optional, Tuple

# JUnit child tags that mean "this test flaked" (failed, then passed).
FLAKY_TAGS = ("flakyFailure", "flakyError")
# JUnit child tags that mean the test ultimately failed.
FAILURE_TAGS = ("failure", "error", "rerunFailure", "rerunError")

SCHEMA_VERSION = 1


def _localname(tag: Any) -> str:
    """Return an XML tag name without any ``{namespace}`` prefix."""
    if not isinstance(tag, str):
        return ""
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def _element_text(elem: ET.Element) -> str:
    return "".join(elem.itertext()).strip()


def _first_line(text: str) -> str:
    for line in text.splitlines():
        line = line.strip()
        if line:
            return line
    return ""


def _load_junit(path: str) -> Tuple[List[Dict[str, Any]], int]:
    """Parse a nextest JUnit file.

    Returns ``(flaky_tests, total_testcases)``. Raises ``ET.ParseError`` or
    ``OSError`` on an unreadable/malformed report.
    """
    root = ET.parse(path).getroot()
    flaky: List[Dict[str, Any]] = []
    total = 0

    for testcase in root.iter():
        if _localname(testcase.tag) != "testcase":
            continue
        total += 1

        children = list(testcase)
        child_tags = {_localname(child.tag) for child in children}
        if "skipped" in child_tags:
            continue

        flaky_children = [c for c in children if _localname(c.tag) in FLAKY_TAGS]
        if not flaky_children:
            continue
        # A test that still failed after its retries is a real failure, not a
        # flake that happened to pass.
        if child_tags & set(FAILURE_TAGS):
            continue

        name = testcase.get("name") or "<unnamed>"
        classname = testcase.get("classname") or ""
        test_id = "{}::{}".format(classname, name) if classname else name
        failed_attempts = len(flaky_children)

        flaky.append(
            {
                "id": test_id,
                "name": name,
                "className": classname,
                "failedAttempts": failed_attempts,
                # e.g. 2 failures then a pass -> passed on attempt 3.
                "passedOnAttempt": failed_attempts + 1,
                "lastError": _first_line(_element_text(flaky_children[0])),
                "time": testcase.get("time"),
            }
        )

    flaky.sort(key=lambda item: item["id"])
    return flaky, total


def _load_quarantine(path: Optional[str]) -> Dict[str, Dict[str, Any]]:
    """Load the quarantine registry as ``{test id: entry}``.

    A missing or malformed registry is non-fatal: an empty registry simply
    means every flaky test is reported as new.
    """
    if not path or not os.path.exists(path):
        return {}
    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError) as exc:  # pragma: no cover - defensive
        print(
            "flaky-report: warning: could not read quarantine file {}: {}".format(
                path, exc
            ),
            file=sys.stderr,
        )
        return {}

    entries = data.get("quarantined") if isinstance(data, dict) else None
    if not isinstance(entries, list):
        return {}

    registry: Dict[str, Dict[str, Any]] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        test = str(entry.get("test") or "").strip()
        if test:
            registry[test] = entry
    return registry


def _match_quarantine(
    test: Dict[str, Any], registry: Dict[str, Dict[str, Any]]
) -> Optional[Dict[str, Any]]:
    """Match a test by full id, then by bare test name."""
    if test["id"] in registry:
        return registry[test["id"]]
    if test["name"] in registry:
        return registry[test["name"]]
    return None


def _deadline_status(entry: Dict[str, Any], today: _dt.date) -> Tuple[str, str]:
    deadline = str(entry.get("deadline") or "").strip()
    if not deadline:
        return "missing-deadline", "no deadline recorded"
    try:
        parsed = _dt.date.fromisoformat(deadline)
    except ValueError:
        return "invalid-deadline", "deadline is not an ISO date: {}".format(deadline)
    if parsed < today:
        return "expired", "deadline {} has passed".format(deadline)
    return "active", "due {}".format(deadline)


def _build_report(
    tests: List[Dict[str, Any]],
    total: int,
    junit_path: str,
    registry: Dict[str, Dict[str, Any]],
    today: _dt.date,
) -> Dict[str, Any]:
    quarantined = 0
    entries: List[Dict[str, Any]] = []

    for test in tests:
        item = dict(test)
        entry = _match_quarantine(test, registry)
        if entry is not None:
            quarantined += 1
            status, detail = _deadline_status(entry, today)
            item["quarantined"] = True
            item["quarantine"] = {
                "owner": entry.get("owner"),
                "issue": entry.get("issue"),
                "deadline": entry.get("deadline"),
                "reason": entry.get("reason"),
                "status": status,
                "detail": detail,
            }
        else:
            item["quarantined"] = False
            item["quarantine"] = None
        entries.append(item)

    return {
        "schemaVersion": SCHEMA_VERSION,
        "generatedAt": _dt.datetime.now(_dt.timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "junitPath": junit_path,
        "totals": {
            "testcases": total,
            "flaky": len(tests),
            "quarantined": quarantined,
            "unquarantined": len(tests) - quarantined,
        },
        "flakyTests": entries,
    }


def _render_markdown(report: Dict[str, Any], status: str) -> str:
    totals = report["totals"]
    tests = report["flakyTests"]
    lines: List[str] = ["# Flaky Test Report", ""]

    if status == "no-input":
        lines += [
            "> ⚠️ No JUnit report was found at `{}`. The test job may not have "
            "run, or it failed before producing output.".format(report["junitPath"]),
            "",
        ]
    elif status == "parse-error":
        lines += [
            "> ⚠️ The JUnit report at `{}` could not be parsed. See the workflow "
            "log for details.".format(report["junitPath"]),
            "",
        ]

    if not tests:
        lines += [
            "**No flaky tests detected.** Every test passed on its first "
            "attempt (or the suite did not run).",
            "",
        ]
    else:
        lines += [
            "Detected **{}** test(s) that passed only on a retry, out of {} "
            "test case(s).".format(totals["flaky"], totals["testcases"]),
            "",
            "| Test | Failed attempts | Passed on | Quarantine | Owner | Deadline |",
            "| --- | ---: | ---: | --- | --- | --- |",
        ]
        for test in tests:
            quarantine = test["quarantine"]
            if quarantine:
                state = quarantine["status"]
                owner = quarantine.get("owner") or "—"
                deadline = quarantine.get("deadline") or "—"
            else:
                state = "**needs quarantine**"
                owner = "—"
                deadline = "—"
            lines.append(
                "| `{}` | {} | {} | {} | {} | {} |".format(
                    test["id"],
                    test["failedAttempts"],
                    test["passedOnAttempt"],
                    state,
                    owner,
                    deadline,
                )
            )
        lines.append("")

        expired = [
            t
            for t in tests
            if t["quarantine"] and t["quarantine"]["status"] == "expired"
        ]
        if expired:
            lines += ["## Expired quarantines", ""]
            lines += [
                "- `{}` — {}".format(t["id"], t["quarantine"]["detail"])
                for t in expired
            ]
            lines += [
                "",
                "Expired entries must be fixed, re-quarantined with a new "
                "deadline, or removed. See CONTRIBUTING.md.",
                "",
            ]

    lines += [
        "---",
        "",
        "_Generated by `scripts/flaky-report.py` from `{}`._".format(
            report["junitPath"]
        ),
        "",
    ]
    return "\n".join(lines)


def _emit_annotations(tests: List[Dict[str, Any]]) -> None:
    for test in tests:
        if test["quarantine"] and test["quarantine"]["status"] == "active":
            continue
        if test["quarantine"] and test["quarantine"]["status"] == "expired":
            print(
                "::warning title=Flaky quarantine expired::{} passed on attempt "
                "{} but its quarantine deadline has passed (owner: {}).".format(
                    test["id"],
                    test["passedOnAttempt"],
                    test["quarantine"].get("owner") or "unassigned",
                )
            )
            continue
        print(
            "::warning title=Flaky test (passed on retry)::{} failed {} time(s) "
            "then passed on attempt {}.".format(
                test["id"], test["failedAttempts"], test["passedOnAttempt"]
            )
        )


def _write_text(path: str, content: str) -> None:
    directory = os.path.dirname(os.path.abspath(path))
    if directory:
        os.makedirs(directory, exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(content)


def _append_text(path: str, content: str) -> None:
    """Append to a file (step summaries already hold earlier steps' output)."""
    directory = os.path.dirname(os.path.abspath(path))
    if directory:
        os.makedirs(directory, exist_ok=True)
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(content)


def _parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a flaky-test report from cargo-nextest JUnit XML."
    )
    parser.add_argument(
        "--input",
        default="target/nextest/ci/junit.xml",
        help="Path to the nextest JUnit XML report.",
    )
    parser.add_argument(
        "--markdown",
        default="flaky-report.md",
        help="Path to write the Markdown report.",
    )
    parser.add_argument(
        "--json",
        default="flaky-report.json",
        help="Path to write the JSON report.",
    )
    parser.add_argument(
        "--quarantine",
        default=".github/flaky-quarantine.json",
        help="Path to the quarantine registry (missing file = empty registry).",
    )
    parser.add_argument(
        "--summary",
        default=None,
        help="If set, append the Markdown report to this file ($GITHUB_STEP_SUMMARY).",
    )
    parser.add_argument(
        "--annotations",
        action="store_true",
        help="Emit GitHub Actions ::warning annotations for non-active flakes.",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit 2 when the JUnit report is missing or malformed.",
    )
    return parser.parse_args(argv)


def main(argv: Optional[List[str]] = None) -> int:
    args = _parse_args(argv)
    today = _dt.date.today()

    tests: List[Dict[str, Any]] = []
    total = 0
    status = "ok"

    if not os.path.exists(args.input):
        status = "no-input"
        print(
            "flaky-report: no JUnit report at {} (reporting zero flakes)".format(
                args.input
            ),
            file=sys.stderr,
        )
    else:
        try:
            tests, total = _load_junit(args.input)
        except (ET.ParseError, OSError) as exc:
            status = "parse-error"
            print(
                "flaky-report: could not parse {}: {}".format(args.input, exc),
                file=sys.stderr,
            )

    registry = _load_quarantine(args.quarantine)
    report = _build_report(tests, total, args.input, registry, today)
    report["status"] = status

    markdown = _render_markdown(report, status)
    _write_text(args.markdown, markdown)
    _write_text(args.json, json.dumps(report, indent=2, sort_keys=True) + "\n")

    if args.summary:
        _append_text(args.summary, markdown + "\n")

    if args.annotations:
        _emit_annotations(report["flakyTests"])

    print(
        "flaky-report: {flaky} flaky test(s) out of {total} "
        "({quarantined} quarantined) -> {md}, {js}".format(
            flaky=report["totals"]["flaky"],
            total=report["totals"]["testcases"],
            quarantined=report["totals"]["quarantined"],
            md=args.markdown,
            js=args.json,
        )
    )

    if args.strict and status != "ok":
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
