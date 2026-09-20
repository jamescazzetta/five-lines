"""Score Jev's judgments against labelled snippets.

In a code review the costly error is the FALSE ALARM: a reviewer who is told three
times that a plain DTO "smuggles behaviour through accessors" stops reading the tool.
A missed violation costs little. So the number to watch is how many raised findings
(probability >= 0.80) are wrong, not overall accuracy.
"""

from __future__ import annotations

import json
from pathlib import Path

from . import jev

RAISE = 0.80


def run(cases_path: Path, out: Path | None) -> int:
    key = jev.api_key()
    if key is None:
        print("TYPESAFE_API_KEY is not set")
        return 2
    cases = json.loads(cases_path.read_text(encoding="utf-8"))["cases"]
    rows = []
    for case in cases:
        questions = {q: jev.QUESTIONS[q] for q in case["expect"]}
        state = {"language": case["language"], "name": case["name"], "method": case["source"]}
        answers = jev.ask(state, questions, key)["answers"]
        for question, expected in case["expect"].items():
            p = float(answers[question]["noul"])
            rows.append({"case": case["id"], "language": case["language"], "question": question,
                         "expected": expected, "p": p, "correct": (p >= 0.5) == expected})
            mark = " " if rows[-1]["correct"] else "x"
            print(f"{mark} {question:<6} expected={expected!s:<5} p={p:.2f}  {case['id']}")

    print(f"\n{'rule':<7}{'n':>4}{'accuracy':>10}{'raised':>8}{'false alarms':>14}{'missed':>8}")
    for question in sorted({r["question"] for r in rows}):
        group = [r for r in rows if r["question"] == question]
        raised = [r for r in group if r["p"] >= RAISE]
        print(f"{question:<7}{len(group):>4}{sum(r['correct'] for r in group) / len(group):>10.0%}{len(raised):>8}"
              f"{sum(not r['expected'] for r in raised):>14}{sum(r['expected'] and r['p'] < RAISE for r in group):>8}")
    raised = [r for r in rows if r["p"] >= RAISE]
    print(f"\n{len(rows)} judgments, {sum(r['correct'] for r in rows) / len(rows):.0%} correct at 0.5; "
          f"{len(raised)} raised at >= {RAISE}, of which {sum(not r['expected'] for r in raised)} are false alarms")
    if out:
        out.write_text(json.dumps({"model": jev.MODEL, "rows": rows}, indent=1) + "\n", encoding="utf-8")
        print(f"wrote {out}")
    return 0
