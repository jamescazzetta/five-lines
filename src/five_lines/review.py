"""Run the ten rules over a diff: mechanical checks first, Jev only where judgment is needed."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from . import jev, rules
from .diff import FileDiff, parse
from .rules import Finding
from .units import LANGUAGES, Unit, units_for

IDIOM_THRESHOLD = 0.70
SKIP_DIRS = {".git", "node_modules", "vendor", "dist", "build", ".venv", "venv", "target", "__pycache__", ".next"}

JEV_RULES: dict[str, tuple[int, str, str]] = {
    "r2": (2, "`{name}` both orchestrates collaborators and computes on raw values",
           "keep the calls in `{name}`; move the inline computation into a method that is handed the values"),
    "r3": (3, "`{name}` has an `if` that is not its first statement, or code after its if/else block",
           "extract the `if` and its block into its own method, so the condition starts that method"),
    "r4": (4, "`{name}` branches with if/else between two pieces of its own domain logic",
           "introduce one interface with a class per branch, and push each branch's body into its class"),
    "r5": (5, "`{name}` has a switch with a catch-all arm, or an arm that does not return",
           "drop the catch-all so a new variant fails loudly and make every arm return; or replace with polymorphism"),
    "r7": (7, "a condition in `{name}` has a side effect",
           "hoist the side effect into its own statement before the condition; split the query from the command"),
    "r9": (9, "`{name}` is an accessor that invites callers to do the object's work",
           "move the caller logic that uses `{name}` into the owning class (push code into data), then remove the accessor"),
}


@dataclass
class Review:
    findings: list[Finding] = field(default_factory=list)
    units: int = 0
    jev_requests: int = 0
    jev_used: bool = False
    notes: list[str] = field(default_factory=list)


def git_diff(repo: Path, base: str) -> str:
    return subprocess.run(["git", "-C", str(repo), "diff", "--unified=3", f"{base}...HEAD"],
                          check=True, capture_output=True, text=True).stdout


def review(diff_text: str, repo: Path | None, base: str | None, use_jev: bool, threshold: float) -> Review:
    result = Review()
    key = jev.api_key() if use_jev else None
    result.jev_used = key is not None
    if use_jev and key is None:
        result.notes.append("TYPESAFE_API_KEY is not set: judgment rules 2, 4, 7 and 9 were not evaluated, "
                            "and rules 3 and 5 only for Python.")
    files = parse(diff_text)
    repo_sources: dict[str, str] | None = None
    for file in files:
        source = _read(repo, file.path)
        if source is None and repo is not None:
            result.notes.append(f"{file.path}: not found under --repo, reviewed from the hunk text only")
        base_source = _git_show(repo, base, file.path) if repo and base and not file.is_new else None
        base_units = _all_units(file.path, base_source)
        for unit in units_for(file, source):
            result.units += 1
            result.findings += _review_unit(unit, base_units, key, threshold, result)
        if rules.inheritance_in(file.added) or rules.interfaces_in(file.added):
            repo_sources = repo_sources if repo_sources is not None else _repo_sources(repo)
            result.findings += _structure_findings(file, repo_sources, repo is not None)
    result.findings.sort(key=lambda f: (f.rule, f.path, f.line))
    return result


def _review_unit(unit: Unit, base_units: list[Unit], key: str | None, threshold: float, result: Review) -> list[Finding]:
    base = min((b for b in base_units if b.name == unit.name), key=lambda b: abs(b.start - unit.start), default=None)
    findings = rules.rule_1(unit, rules.statement_count(base) if base else None)
    decided: set[str] = set()
    if unit.language == "python" and unit.kind == "method":
        # Pre-existing constructs in a touched method are out of scope: keep only the ones this diff added.
        findings += [f for f in rules.rule_3_python(unit) + rules.rule_5_python(unit) if f.line in unit.added_lines]
        decided |= {"r3", "r5"}
    else_lines = [line for line in rules.rule_4_candidates(unit) if line in unit.added_lines]
    if not else_lines:
        decided.add("r4")
    affix_findings, affix_candidates = rules.rule_10(unit) if unit.kind == "method" else ([], [])
    findings += affix_findings

    if key is None:
        findings += [Finding(4, unit.path, line, unit.name, basis="candidate",
                             message=f"if/else at line {line} in `{unit.name}` (unconfirmed: needs judgment on whether "
                                     "both branches are your own domain logic)",
                             fix=JEV_RULES["r4"][2].format(name=unit.name)) for line in else_lines[:1]]
        return findings

    answers = jev.judge_unit(unit, decided, key)
    result.jev_requests += bool(answers)
    for question, probability in answers.items():
        if question == "idiom" or probability < 0.55:
            continue
        number, message, fix = JEV_RULES[question]
        line = else_lines[0] if question == "r4" and else_lines else unit.start
        findings.append(Finding(number, unit.path, line, unit.name, message.format(name=unit.name),
                                fix.format(name=unit.name), basis="jev", confidence=probability,
                                severity="introduced" if probability >= threshold else "worth a look"))
    for names in affix_candidates:
        probability = jev.judge_affixes(names, key)
        result.jev_requests += 1
        if probability >= 0.55:
            findings.append(Finding(10, unit.path, unit.start, unit.name, basis="jev", confidence=probability,
                                    severity="introduced" if probability >= threshold else "worth a look",
                                    message=f"{', '.join(f'`{n}`' for n in names)} share an affix and look like one concept",
                                    fix="introduce a type that holds them together, and move the logic that uses them onto it"))
    idiom = answers.get("idiom", 0.0)
    if idiom >= IDIOM_THRESHOLD:
        for finding in findings:
            finding.suppressed = f"framework or language idiom (p={idiom:.2f})"
    return findings


def _structure_findings(file: FileDiff, repo_sources: dict[str, str], have_repo: bool) -> list[Finding]:
    findings: list[Finding] = []
    for line, cls, base in rules.inheritance_in(file.added):
        kind = rules.base_is_interface(base, repo_sources)
        if kind is True:
            continue
        unknown = kind is None
        findings.append(Finding(
            6, file.path, line, cls, basis="candidate" if unknown else "mechanical",
            message=f"`{cls}` inherits from `{base}`" + (
                ", which was not found in the repo (a library type? then this is fine)" if unknown
                else ", which carries method bodies: that is inheriting implementation"),
            fix=f"give `{cls}` a `{base}` field and delegate to it; share the contract through an interface"))
    for line, name in rules.interfaces_in(file.added) if have_repo else []:
        users = rules.implementers(name, repo_sources)
        if len(users) <= 1:
            findings.append(Finding(
                8, file.path, line, name, basis="mechanical",
                message=f"interface `{name}` has {len(users)} non-test implementer(s)" + (f": {users[0]}" if users else ""),
                fix=f"delete `{name}` and use the concrete class directly until a second implementation exists"))
    return findings


def _read(repo: Path | None, path: str) -> str | None:
    if repo is None:
        return None
    try:
        return (repo / path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def _git_show(repo: Path, ref: str, path: str) -> str | None:
    shown = subprocess.run(["git", "-C", str(repo), "show", f"{ref}:{path}"], capture_output=True, text=True)
    return shown.stdout if shown.returncode == 0 else None


def _all_units(path: str, source: str | None) -> list[Unit]:
    """Every method in the BASE version, to tell 'introduced' from 'grown' and to skip methods that only shrank."""
    if source is None:
        return []
    everything = FileDiff(path, added={n: text for n, text in enumerate(source.splitlines(), 1)})
    return [u for u in units_for(everything, source) if u.kind == "method"]


def _repo_sources(repo: Path | None) -> dict[str, str]:
    if repo is None:
        return {}
    sources: dict[str, str] = {}
    for path in repo.rglob("*"):
        if len(sources) >= 5000:
            break
        if path.suffix.lower() in LANGUAGES and not (set(path.parts) & SKIP_DIRS) and path.is_file():
            if path.stat().st_size <= 1_000_000:
                sources[str(path.relative_to(repo))] = path.read_text(encoding="utf-8", errors="replace")
    return sources
