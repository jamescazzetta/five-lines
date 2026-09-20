"""Render a review as Markdown (for a PR comment) or JSON (for tooling)."""

from __future__ import annotations

import json
from dataclasses import asdict

from .review import Review
from .rules import RULES, Finding


def _tag(finding: Finding) -> str:
    if finding.basis == "mechanical":
        return "mechanical"
    if finding.basis == "candidate":
        return "unconfirmed"
    return f"Jev {finding.confidence:.2f}"


def markdown(result: Review, show_suppressed: bool) -> str:
    shown = [f for f in result.findings if show_suppressed or not f.suppressed]
    hidden = len(result.findings) - len(shown)
    raised = [f for f in shown if f.severity != "worth a look" and not f.suppressed]
    lines = [
        "## Five Lines review",
        "",
        "_A structural-quality lens (Christian Clausen, *Five Lines of Code*). It is not a correctness review: "
        "a method can break every rule below and be correct, and a 3-line method can still have a bug._",
        "",
        f"{result.units} changed method(s) reviewed · {len(raised)} finding(s) · "
        + (f"{result.jev_requests} Jev request(s)" if result.jev_used else "mechanical rules only"),
    ]
    lines += ["", *[f"> {note}" for note in result.notes]] if result.notes else []
    if not shown:
        lines += ["", "No rule violations introduced by this diff."]
    for number in sorted({f.rule for f in shown}):
        lines += ["", f"### Rule {number} · {RULES[number]}"]
        for f in (f for f in shown if f.rule == number):
            status = f"suppressed: {f.suppressed}" if f.suppressed else f.severity
            lines += [f"- `{f.path}:{f.line}` — {f.message} _({_tag(f)}; {status})_",
                      f"  - Fix: {f.fix}"]
    if hidden:
        lines += ["", f"_{hidden} finding(s) suppressed as framework or language idioms; rerun with `--show-suppressed`._"]
    return "\n".join(lines) + "\n"


def as_json(result: Review) -> str:
    return json.dumps({"units": result.units, "jev_used": result.jev_used, "jev_requests": result.jev_requests,
                       "notes": result.notes, "findings": [asdict(f) for f in result.findings]}, indent=2)
