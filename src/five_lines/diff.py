"""Unified-diff parsing: which lines of which files did this PR add?"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

_HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")


@dataclass
class FileDiff:
    path: str
    is_new: bool = False
    added: dict[int, str] = field(default_factory=dict)
    """New-file line number -> added text. The rules only ever judge what a diff ADDS."""
    context: dict[int, str] = field(default_factory=dict)
    """New-file line number -> unchanged text the hunk carried; the fallback when the full file is unavailable."""


def parse(diff_text: str) -> list[FileDiff]:
    files: list[FileDiff] = []
    current: FileDiff | None = None
    old_is_null = False  # '--- /dev/null' precedes the '+++' line it describes
    new_line = 0
    for raw in diff_text.splitlines():
        if raw.startswith("diff --git"):
            current, new_line = None, 0
        elif raw.startswith("--- "):
            old_is_null = raw[4:].strip() == "/dev/null"
        elif raw.startswith("+++ "):
            target = raw[4:].strip()
            current = None if target == "/dev/null" else FileDiff(target.removeprefix("b/"), is_new=old_is_null)
            if current:
                files.append(current)
        elif current is not None and (m := _HUNK.match(raw)):
            new_line = int(m.group(1))
        elif current is not None and new_line:
            if raw.startswith("+"):
                current.added[new_line] = raw[1:]
                new_line += 1
            elif raw.startswith(" ") or raw == "":
                current.context[new_line] = raw[1:]
                new_line += 1
            # '-' lines and '\ No newline' markers do not advance the new-file counter
    return [f for f in files if f.added]
