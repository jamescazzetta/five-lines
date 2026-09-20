"""The ten rules. What can be COUNTED is counted here; what needs judgment becomes a question for Jev.

Each rule produces findings with a ``basis``:

* ``mechanical`` — decided by parsing alone. Reproducible, no model involved.
* ``candidate``  — a structural signal was found, but whether it is a violation needs
  judgment (is this if/else your own domain logic, or a branch on a library type?).
  Jev confirms or dismisses it; without an API key it is reported as unconfirmed.
"""

from __future__ import annotations

import ast
import re
from dataclasses import dataclass, field
from itertools import combinations

from .units import Unit

LIMIT = 5

RULES = {
    1: "Five lines",
    2: "Call or pass, not both",
    3: "If only at the start",
    4: "Never if-else",
    5: "Never switch",
    6: "Inherit only from interfaces",
    7: "Pure conditions",
    8: "No single-implementation interfaces",
    9: "Avoid getters/setters",
    10: "No common affixes",
}


@dataclass
class Finding:
    rule: int
    path: str
    line: int
    unit: str
    message: str
    fix: str
    basis: str  # "mechanical" | "jev" | "candidate"
    confidence: float | None = None
    severity: str = "introduced"
    suppressed: str | None = None
    detail: dict[str, object] = field(default_factory=dict)


# ---------------------------------------------------------------- rule 1


def statement_count(unit: Unit) -> int:
    if isinstance(unit.node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        return sum(1 for n in _body_nodes(unit.node) if isinstance(n, ast.stmt))
    return _brace_statement_count(unit.source)


def _body_nodes(fn: ast.FunctionDef | ast.AsyncFunctionDef) -> list[ast.AST]:
    body = fn.body[1:] if _is_docstring(fn.body[0]) else fn.body
    found: list[ast.AST] = []
    stack: list[ast.AST] = list(body)
    while stack:
        node = stack.pop()
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            found.append(node)  # a nested def counts as one statement; its body is its own unit
            continue
        found.append(node)
        stack.extend(c for c in ast.iter_child_nodes(node) if isinstance(c, (ast.stmt, ast.ExceptHandler, ast.match_case)))
    return found


def _is_docstring(node: ast.stmt) -> bool:
    return isinstance(node, ast.Expr) and isinstance(node.value, ast.Constant) and isinstance(node.value.value, str)


def _brace_statement_count(source: str) -> int:
    """Body lines that carry a statement. One call wrapped over several lines is ONE statement."""
    lines = source.splitlines()
    opened = next((i for i, text in enumerate(lines) if "{" in text), None)
    body = lines[opened + 1:-1] if opened is not None and len(lines) > opened + 1 else lines[1:]
    count, depth = 0, 0
    for raw in body:
        text = re.sub(r"//.*$", "", raw).strip()
        if not text or text.startswith(("/*", "*", "*/")):
            continue
        # A line that opens with closing punctuation or an operator continues the statement above it.
        if depth == 0 and not re.match(r"^[)\]}.,?:&|+]", text) and text != "{":
            count += 1
        depth = max(depth + sum(text.count(c) for c in "([") - sum(text.count(c) for c in ")]"), 0)
    return count


def rule_1(unit: Unit, base_count: int | None) -> list[Finding]:
    if unit.kind != "method":
        return []
    count = statement_count(unit)
    if count <= LIMIT or (base_count is not None and count <= base_count):
        return []  # within budget, or the diff did not make an already-long method longer
    grew = base_count is not None and base_count > LIMIT
    size = "slightly over" if count <= LIMIT + 2 else ("well over" if count <= 15 else "a god-method")
    block = _largest_block(unit)
    return [Finding(
        1, unit.path, unit.start, unit.name, basis="mechanical", severity="grown" if grew else "introduced",
        message=f"`{unit.name}` has {count} statements ({size}; budget {LIMIT})"
                + (f", up from {base_count}" if grew else ""),
        fix=f"extract the `{block[0]}` block at line {block[1]} into its own method" if block
            else "extract the longest cohesive run of statements into its own method",
        detail={"statements": count, "base": base_count},
    )]


def _largest_block(unit: Unit) -> tuple[str, int] | None:
    if not isinstance(unit.node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        return None
    blocks = [n for n in unit.node.body if isinstance(n, (ast.For, ast.AsyncFor, ast.While, ast.If, ast.With, ast.Try))]
    if not blocks:
        return None
    biggest = max(blocks, key=lambda n: (n.end_lineno or n.lineno) - n.lineno)
    return type(biggest).__name__.lower().replace("async", ""), biggest.lineno


# ---------------------------------------------------------------- rule 3 and 4 (python: exact)


def rule_3_python(unit: Unit) -> list[Finding]:
    fn = unit.node
    if not isinstance(fn, (ast.FunctionDef, ast.AsyncFunctionDef)):
        return []
    body = fn.body[1:] if _is_docstring(fn.body[0]) else fn.body
    findings: list[Finding] = []
    for node in _ifs_outside_elif(fn):
        first = bool(body) and node is body[0]
        if first and len(body) == 1:
            continue
        what = "is followed by more statements at the same level" if first else "is not the method's first statement"
        findings.append(Finding(
            3, unit.path, node.lineno, unit.name, basis="mechanical",
            message=f"the `if` at line {node.lineno} in `{unit.name}` {what}",
            fix=f"extract the `if` at line {node.lineno} and its block into its own method, so it starts that method",
        ))
    return findings


def _ifs_outside_elif(fn: ast.AST) -> list[ast.If]:
    elifs = {id(n.orelse[0]) for n in ast.walk(fn)
             if isinstance(n, ast.If) and len(n.orelse) == 1 and isinstance(n.orelse[0], ast.If)}
    own = [n for n in ast.walk(fn) if isinstance(n, ast.If) and id(n) not in elifs]
    nested_defs = [d for d in ast.walk(fn) if d is not fn and isinstance(d, (ast.FunctionDef, ast.AsyncFunctionDef))]
    inner = {id(n) for d in nested_defs for n in ast.walk(d)}
    return [n for n in own if id(n) not in inner]


def rule_4_candidates(unit: Unit) -> list[int]:
    """Lines of if/else constructs. Whether both branches are OWN domain logic is Jev's question."""
    if isinstance(unit.node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        return [n.lineno for n in _ifs_outside_elif(unit.node) if n.orelse]
    return [unit.start + i for i, text in enumerate(unit.source.splitlines()) if re.search(r"\belse\b", text)]


# ---------------------------------------------------------------- rule 5


def rule_5_python(unit: Unit) -> list[Finding]:
    if not isinstance(unit.node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        return []
    findings: list[Finding] = []
    for match in (n for n in ast.walk(unit.node) if isinstance(n, ast.Match)):
        catch_all = [c for c in match.cases if isinstance(c.pattern, ast.MatchAs) and c.pattern.pattern is None]
        open_arms = [c for c in match.cases if not isinstance(c.body[-1], (ast.Return, ast.Raise))]
        if catch_all:
            findings.append(Finding(5, unit.path, match.lineno, unit.name, basis="mechanical",
                                    message=f"the `match` at line {match.lineno} has a catch-all `case _`",
                                    fix="list every case explicitly so a new variant fails loudly, or replace with polymorphism"))
        elif open_arms:
            findings.append(Finding(5, unit.path, match.lineno, unit.name, basis="mechanical",
                                    message=f"{len(open_arms)} arm(s) of the `match` at line {match.lineno} do not return",
                                    fix="make every arm return; move any work after the match into the arms or a new method"))
    return findings


# ---------------------------------------------------------------- rule 6 and 8 (need the repo)

_IDIOM_BASES = {"Exception", "BaseException", "Error", "Protocol", "ABC", "Enum", "IntEnum", "StrEnum", "Flag",
                "TypedDict", "NamedTuple", "BaseModel", "object", "Generic", "TestCase", "Component", "PureComponent"}
_EXTENDS = re.compile(r"\bclass\s+(?P<cls>\w+)(?:<[^>]*>)?\s+extends\s+(?P<base>[\w.\\]+)")
_PY_CLASS = re.compile(r"^\s*class\s+(?P<cls>\w+)\s*\((?P<bases>[^)]*)\)\s*:")
_INTERFACE_DECL = re.compile(r"\b(?:interface|trait|protocol)\s+(?P<name>\w+)")


def inheritance_in(added: dict[int, str]) -> list[tuple[int, str, str]]:
    """(line, class, base) for every inheritance the diff ADDS."""
    found: list[tuple[int, str, str]] = []
    for line_no, text in added.items():
        if m := _EXTENDS.search(text):
            found.append((line_no, m.group("cls"), m.group("base").split(".")[-1].split("\\")[-1]))
        elif m := _PY_CLASS.match(text):
            for base in (b.strip().split("[")[0].split(".")[-1] for b in m.group("bases").split(",")):
                if base and "=" not in base:
                    found.append((line_no, m.group("cls"), base))
    return [f for f in found if f[2] not in _IDIOM_BASES and not f[2].endswith(("Error", "Exception"))]


def interfaces_in(added: dict[int, str]) -> list[tuple[int, str]]:
    found = [(n, m.group("name")) for n, text in added.items() if (m := _INTERFACE_DECL.search(text))]
    for n, text in added.items():
        if (m := _PY_CLASS.match(text)) and re.search(r"\b(Protocol|ABC)\b", m.group("bases")):
            found.append((n, m.group("cls")))
    return found


def base_is_interface(base: str, repo_sources: dict[str, str]) -> bool | None:
    """True = bodiless contract, False = carries implementation, None = not found in the repo (a library type?)."""
    for path, source in repo_sources.items():
        if re.search(rf"\b(?:interface|trait|protocol)\s+{re.escape(base)}\b", source):
            return True
        if path.endswith(".py") and re.search(rf"^\s*class\s+{re.escape(base)}\b", source, re.M):
            return _python_class_is_bodiless(source, base)
        if re.search(rf"\b(?:abstract\s+)?class\s+{re.escape(base)}\b", source):
            return False
    return None


def _python_class_is_bodiless(source: str, name: str) -> bool | None:
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return None
    for cls in (n for n in ast.walk(tree) if isinstance(n, ast.ClassDef) and n.name == name):
        methods = [n for n in cls.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
        return all(_is_stub(m) for m in methods)
    return None


def _is_stub(fn: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    body = fn.body[1:] if _is_docstring(fn.body[0]) and len(fn.body) > 1 else fn.body
    return all(isinstance(s, ast.Pass) or _is_docstring(s) or isinstance(s, ast.Raise)
               or (isinstance(s, ast.Expr) and isinstance(s.value, ast.Constant) and s.value.value is Ellipsis)
               for s in body)


_TEST_PATH = re.compile(r"(^|/)(tests?|spec|__tests__|mocks?|fakes?|stubs?|fixtures)(/|$)|[._-](test|spec|mock|fake|stub)s?\.", re.I)


def implementers(interface: str, repo_sources: dict[str, str]) -> list[str]:
    name = re.escape(interface)
    patterns = [rf"\bimplements\b[^{{]*\b{name}\b", rf"^\s*class\s+\w+\s*\([^)]*\b{name}\b[^)]*\)\s*:",
                rf"\bimpl(?:<[^>]*>)?\s+{name}\b", rf"\bclass\s+\w+[^{{\n]*:\s*[^{{\n]*\b{name}\b",
                rf"\buse\s+{name}\s*;"]
    return sorted(path for path, source in repo_sources.items()
                  if not _TEST_PATH.search(path) and any(re.search(p, source, re.M) for p in patterns))


# ---------------------------------------------------------------- rule 10

_PAIRS = [{"start", "end"}, {"begin", "end"}, {"min", "max"}, {"from", "to"}, {"first", "last"}, {"old", "new"},
          {"src", "dst"}, {"source", "target"}, {"source", "destination"}, {"lower", "upper"}, {"left", "right"},
          {"before", "after"}, {"prev", "next"}, {"previous", "next"}, {"in", "out"}, {"input", "output"},
          {"x", "y"}, {"lat", "lng"}, {"lat", "lon"}, {"latitude", "longitude"}, {"width", "height"}]


def words(identifier: str) -> list[str]:
    return [w.lower() for w in re.findall(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|\d+", identifier.replace("_", " ").replace("$", ""))]


def declared_names(unit: Unit) -> list[str]:
    """Parameters and fields this unit declares — the siblings rule 10 compares."""
    if isinstance(unit.node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        args = unit.node.args
        names = [a.arg for a in [*args.posonlyargs, *args.args, *args.kwonlyargs] if a.arg not in {"self", "cls"}]
        names += [t.attr for n in ast.walk(unit.node) if isinstance(n, (ast.Assign, ast.AnnAssign))
                  for t in (n.targets if isinstance(n, ast.Assign) else [n.target])
                  if isinstance(t, ast.Attribute) and isinstance(t.value, ast.Name) and t.value.id == "self"]
        return list(dict.fromkeys(names))
    header = unit.source.split("{", 1)[0]
    params = re.search(r"\(([^)]*)\)", header, re.S)
    names = [_param_name(part, unit.language) for part in (params.group(1).split(",") if params else [])]
    return list(dict.fromkeys(n for n in names if n))


def _param_name(part: str, language: str) -> str:
    """`name: Type` (TS, Kotlin, Swift, Rust), `name Type` (Go) or `Type name` (Java, C#, PHP, C)."""
    tokens = re.findall(r"[$\w]+", re.sub(r"=.*$", "", part).split(":")[0])
    if not tokens:
        return ""
    name_first = ":" in part or language == "go"
    return (tokens[0] if name_first else tokens[-1]).lstrip("$")


def rule_10(unit: Unit) -> tuple[list[Finding], list[list[str]]]:
    """Known antonym pairs are flagged mechanically; other shared affixes are returned as candidates for Jev."""
    names = declared_names(unit)
    findings: list[Finding] = []
    flagged: set[str] = set()
    for a, b in combinations(names, 2):
        wa, wb = words(a), words(b)
        if len(wa) != len(wb) or len(wa) < 2:
            continue
        diff = [(x, y) for x, y in zip(wa, wb) if x != y]
        if len(diff) == 1 and {diff[0][0], diff[0][1]} in _PAIRS and (wa[0] != wb[0] or wa[-1] != wb[-1]):
            shared = "".join(w.title() for w in wa if w not in diff[0])
            flagged |= {a, b}
            findings.append(Finding(
                10, unit.path, unit.start, unit.name, basis="mechanical",
                message=f"`{a}` and `{b}` differ only by the affix {diff[0][0]}/{diff[0][1]}",
                fix=f"introduce a `{shared or 'Range'}Range`-style type holding both, and move the logic that uses them onto it",
            ))
    groups: dict[tuple[str, ...], list[str]] = {}
    for name in names:
        w = words(name)
        if len(w) >= 2 and name not in flagged:
            groups.setdefault(("prefix", *w[:-1]), []).append(name)
            groups.setdefault(("suffix", *w[1:]), []).append(name)
    candidates = [g for key, g in groups.items() if len(g) >= 2 and len("".join(key[1:])) >= 4]
    return findings, [list(g) for g in {tuple(sorted(g)): g for g in candidates}.values()]
