"""five-lines: review a PR diff against the ten rules of *Five Lines of Code*.

    gh pr diff 123 | five-lines review - --repo .
    five-lines review --repo . --base origin/main
    five-lines review change.diff --no-jev
    five-lines eval
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import evaluate, report
from .review import git_diff, review


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="five-lines", description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)

    r = sub.add_parser("review", help="review a diff")
    r.add_argument("diff", nargs="?", help="diff file, or '-' for stdin; omit to diff --base...HEAD in --repo")
    r.add_argument("--repo", type=Path, help="checkout of the PR head: lets the tool read whole methods and resolve base classes")
    r.add_argument("--base", help="base ref, e.g. origin/main: tells 'introduced' from 'grown' and supplies the diff if none is given")
    r.add_argument("--no-jev", action="store_true", help="mechanical rules only; makes no network request")
    r.add_argument("--threshold", type=float, default=0.80, help="Jev probability at which a judgment is raised (default 0.80); 0.55 to the threshold is 'worth a look'")
    r.add_argument("--format", choices=("md", "json"), default="md")
    r.add_argument("--show-suppressed", action="store_true", help="include findings suppressed as framework idioms")
    r.add_argument("--fail-on-findings", action="store_true", help="exit 1 if anything is raised (for CI)")

    e = sub.add_parser("eval", help="score Jev's judgments against the labelled snippets in eval/cases.json")
    e.add_argument("--cases", type=Path, default=Path(__file__).resolve().parents[2] / "eval" / "cases.json")
    e.add_argument("--out", type=Path, help="write full rows as JSON")

    args = parser.parse_args(argv)
    if args.command == "eval":
        return evaluate.run(args.cases, args.out)

    if args.diff == "-":
        diff_text = sys.stdin.read()
    elif args.diff:
        diff_text = Path(args.diff).read_text(encoding="utf-8", errors="replace")
    elif args.repo and args.base:
        diff_text = git_diff(args.repo, args.base)
    else:
        parser.error("give a diff file, '-' for stdin, or both --repo and --base")

    result = review(diff_text, args.repo, args.base, use_jev=not args.no_jev, threshold=args.threshold)
    print(report.as_json(result) if args.format == "json" else report.markdown(result, args.show_suppressed), end="")
    raised = [f for f in result.findings if not f.suppressed and f.severity != "worth a look"]
    return 1 if args.fail_on_findings and raised else 0


if __name__ == "__main__":
    sys.exit(main())
