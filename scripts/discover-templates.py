#!/usr/bin/env python3
"""Emit every template scaffoldable by `starforge new` as a JSON array."""

import json
import re
from pathlib import Path

root = Path(__file__).resolve().parents[1]
new_source = (root / "src/commands/new.rs").read_text(encoding="utf-8")
match = re.search(
    r"pub const BUILTIN_TEMPLATE_NAMES: &\[&str\] = &\[(.*?)\];", new_source, re.S
)
if not match:
    raise SystemExit("Could not find BUILTIN_TEMPLATE_NAMES in src/commands/new.rs")
builtins = re.findall(r'"([a-zA-Z0-9_-]+)"', match.group(1))
examples = sorted(
    path.name for path in (root / "templates/examples").iterdir() if path.is_dir()
)
print(json.dumps(sorted(set(builtins + examples)), separators=(",", ":")))
