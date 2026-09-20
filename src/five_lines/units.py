"""Find the methods a diff touched. The rules are per-method, so the method is the unit of review.

Python is parsed exactly with ``ast``. Brace languages use a header regex plus brace
matching that skips strings and comments — a heuristic, good enough to find the method
around a changed line, not a parser. Anything else falls back to the hunk text itself.
"""

from __future__ import annotations

import ast
import re
from dataclasses import dataclass, field

from .diff import FileDiff

LANGUAGES = {
    ".py": "python", ".js": "javascript", ".jsx": "javascript", ".mjs": "javascript",
    ".ts": "typescript", ".tsx": "typescript", ".java": "java", ".kt": "kotlin",
    ".cs": "csharp", ".go": "go", ".php": "php", ".rs": "rust", ".swift": "swift",
    ".c": "c", ".h": "c", ".cpp": "cpp", ".cc": "cpp", ".hpp": "cpp", ".scala": "scala", ".dart": "dart",
}
BRACE_LANGUAGES = set(LANGUAGES.values()) - {"python"}

_CONTROL = r"(?:if|else|for|foreach|while|switch|catch|try|do|return|new|throw|using|lock|synchronized|match|when|select)\b"
_HEADER = re.compile(
    rf"^\s*(?!{_CONTROL})"                      # not a control statement
    r"(?:[\w<>\[\],.:?&*@$]+\s+)*"              # modifiers / return type / 'function' / 'func' / 'fn' / 'def'
    r"(?P<name>[\w$]+)\s*(?:<[^>]*>)?\s*\([^;]*$"  # name(  ...and no ';' => a definition, not a call statement
)
_ARROW = re.compile(r"(?:const|let|var|public|private|protected|readonly|static|\s)*\s*(?P<name>[\w$]+)\s*[:=][^=]*=>\s*\{?\s*$")


@dataclass
class Unit:
    path: str
    language: str
    name: str
    kind: str  # "method" | "hunk"
    start: int
    end: int
    source: str
    added_lines: list[int] = field(default_factory=list)
    node: ast.AST | None = None  # python only

    @property
    def added_text(self) -> str:
        """Only the lines this diff added. A rule about a construct (an if, a switch) is judged on these:
        an `if` that was already there is pre-existing and out of scope, even inside a method the diff touched."""
        lines = self.source.splitlines()
        return "\n".join(lines[n - self.start] for n in self.added_lines if 0 <= n - self.start < len(lines))


def language_of(path: str) -> str:
    dot = path.rfind(".")
    return LANGUAGES.get(path[dot:].lower(), "unknown") if dot >= 0 else "unknown"


def units_for(file: FileDiff, source: str | None) -> list[Unit]:
    language = language_of(file.path)
    if source is not None:
        spans = _python_spans(source) if language == "python" else (
            _brace_spans(source) if language in BRACE_LANGUAGES else [])
        units = _units_from_spans(file, source, language, spans)
        if units:
            return units
    return _hunk_units(file, language)


def _units_from_spans(file: FileDiff, source: str, language: str,
                      spans: list[tuple[str, int, int, ast.AST | None]]) -> list[Unit]:
    lines = source.splitlines()
    by_span: dict[tuple[int, int], Unit] = {}
    for line_no in sorted(file.added):
        if not file.added[line_no].strip():
            continue
        # innermost enclosing method = the narrowest span containing the line
        holders = [s for s in spans if s[1] <= line_no <= s[2]]
        if not holders:
            continue
        name, start, end, node = min(holders, key=lambda s: s[2] - s[1])
        unit = by_span.setdefault((start, end), Unit(
            file.path, language, name, "method", start, end, "\n".join(lines[start - 1:end]), node=node))
        unit.added_lines.append(line_no)
    return list(by_span.values())


def _python_spans(source: str) -> list[tuple[str, int, int, ast.AST | None]]:
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return []
    return [(n.name, n.lineno, n.end_lineno or n.lineno, n)
            for n in ast.walk(tree) if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]


def _brace_spans(source: str) -> list[tuple[str, int, int, ast.AST | None]]:
    code = _blank_strings_and_comments(source).splitlines()
    spans: list[tuple[str, int, int, ast.AST | None]] = []
    for i, line in enumerate(code):
        m = _HEADER.match(line) or _ARROW.match(line)
        if not m or m.group("name") in {"if", "for", "while", "switch", "catch", "function"}:
            continue
        opened = _find_open_brace(code, i)
        if opened is None:
            continue
        end = _match_brace(code, opened)
        if end is not None and end > i:
            spans.append((m.group("name"), i + 1, end + 1, None))
    return spans


def _find_open_brace(code: list[str], start: int) -> int | None:
    """The header may wrap: look a few lines ahead for the '{' that opens the body."""
    for j in range(start, min(start + 8, len(code))):
        if "{" in code[j]:
            return j
        if ";" in code[j]:  # a declaration or a call after all
            return None
    return None


def _match_brace(code: list[str], open_line: int) -> int | None:
    depth = 0
    for j in range(open_line, len(code)):
        for ch in code[j]:
            depth += ch == "{"
            depth -= ch == "}"
            if depth == 0 and ch == "}":
                return j
    return None


def _blank_strings_and_comments(source: str) -> str:
    """Replace string and comment CONTENT with spaces, keeping line structure, so braces inside them don't count."""
    out: list[str] = []
    i, n = 0, len(source)
    while i < n:
        two = source[i:i + 2]
        if two == "//":
            j = source.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif two == "/*":
            j = source.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append("".join(c if c == "\n" else " " for c in source[i:j]))
            i = j
        elif source[i] in "\"'`":
            quote, j = source[i], i + 1
            # ' and " end at the line break at the latest (an apostrophe in a comment-free line must not eat the file)
            while j < n and source[j] != quote and (quote == "`" or source[j] != "\n"):
                j += 2 if source[j] == "\\" else 1
            closed = j < n and source[j] == quote
            out.append(quote + "".join(c if c == "\n" else " " for c in source[i + 1:j]) + (quote if closed else ""))
            i = j + 1 if closed else j
        else:
            out.append(source[i])
            i += 1
    return "".join(out)


def _hunk_units(file: FileDiff, language: str) -> list[Unit]:
    """No full file to read (diff piped in without --repo): review each run of added lines with its context."""
    merged = {**file.context, **file.added}
    units: list[Unit] = []
    run: list[int] = []
    for line_no in sorted(file.added) + [-1]:
        if run and line_no != run[-1] + 1:
            start, end = max(min(merged), run[0] - 3), min(max(merged), run[-1] + 3)
            text = "\n".join(merged.get(k, "") for k in range(start, end + 1))
            units.append(Unit(file.path, language, f"lines {run[0]}-{run[-1]}", "hunk", start, end, text, list(run)))
            run = []
        if line_no >= 0:
            run.append(line_no)
    return [u for u in units if u.source.strip()]
