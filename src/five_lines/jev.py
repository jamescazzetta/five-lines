"""The judgment half: typed yes/no questions to Jev (typesafe.ai), one request per method.

Jev answers a Noul with a number from 0 to 1. Questions in one request are evaluated in
parallel and in isolation, so every rule is asked as its own atomic question and the
answers are combined in code — never one "review this method" prompt.

Only questions whose structural precondition holds are sent: no ``else`` in the method,
no rule-4 question. Standard library only, so the tool installs with no dependencies.
"""

from __future__ import annotations

import json
import os
import re
import time
import urllib.error
import urllib.request
from typing import Any

from .units import Unit

ENDPOINT = "https://api.typesafe.ai/v1/systemone"
MODEL = os.environ.get("FIVE_LINES_MODEL", "jev-latest")
MAX_SOURCE_CHARS = 6000


def _noul(instructions: str, true: str, false: str) -> dict[str, Any]:
    return {"type": "noul", "instructions": instructions, "criteria": {"true": true, "false": false}}


QUESTIONS: dict[str, dict[str, Any]] = {
    "r2": _noul(
        "Does `method` mix two levels of abstraction: it orchestrates calls on collaborator objects AND ALSO does "
        "inline low-level work on raw values (arithmetic, string building, index manipulation)?",
        "Both are present in the same body: calls such as this.x.y() / self.x.y() alongside inline arithmetic or string assembly",
        "The method only orchestrates calls, or only computes on the data it was given"),
    "r3": _noul(
        "Does `method` contain an `if` that is not the very first statement of the method, or is there code after "
        "an if/else block at the same nesting level?",
        "An if is preceded by other statements, sits inside a loop after other work, or is followed by more statements",
        "There is no if, or the if is the first statement and nothing follows its block"),
    "r4": _noul(
        "Does `method` contain an if/else (or else-if chain) in which BOTH branches are the project's own domain logic?",
        "Both branches implement business behaviour that could be two implementations of one interface",
        "There is no else; or the branch tests a foreign or library type, null/None, an error value, or a primitive "
        "the project does not control; or it is a guard clause that returns early"),
    "r5": _noul(
        "Does `method` contain a switch / match / when / case that has a catch-all arm (default, else, _) or an arm "
        "that falls through, breaks, or otherwise does not return?",
        "A catch-all arm exists, or at least one arm does not return",
        "There is no switch-like construct; or it is exhaustive with no catch-all and every arm returns"),
    "r7": _noul(
        "Does the boolean condition of any if or while in `method` have a side effect?",
        "The condition itself assigns a variable, performs I/O, logs, mutates state, advances an iterator or stream, "
        "or calls something that writes or throws",
        "Conditions only read values and call pure queries"),
    "r9": _noul(
        "Is `method` a getter or setter that exposes an object's internal field so callers can branch on it or mutate it?",
        "It returns or assigns a private field of an object that has behaviour, inviting logic to live in the caller",
        "It is not an accessor; or it belongs to a plain data carrier (DTO, record, struct, dataclass, schema type, "
        "ORM-mapped entity) that has no behaviour to push data into"),
    "idiom": _noul(
        "Is the shape of `method` dictated by the language or a framework rather than chosen by the author?",
        "A generated or promoted constructor of a data carrier; a framework entry point (controller action, resolver, "
        "handler, lifecycle hook, main); a test fixture hook (setUp, beforeEach, @BeforeAll); an ORM or serialization mapping",
        "Ordinary application or domain code whose structure the author is free to change"),
}
AFFIX_QUESTION = _noul(
    "Do the identifiers in `names` describe parts of ONE concept that should be its own type?",
    "They share a prefix or suffix because they belong together, e.g. a range, a coordinate, an address, a money amount",
    "They merely share a common word (id, name, count, list) and are otherwise unrelated")

_PRECONDITIONS = {
    "r3": re.compile(r"\bif\b"),
    "r4": re.compile(r"\belse\b|\belif\b|\belsif\b"),
    "r5": re.compile(r"\b(switch|match|when|case)\b"),
    "r7": re.compile(r"\b(if|while|elif)\b"),
    "r9": re.compile(r"\b(get|set|is|has)[A-Z_]\w*\s*\(|@property|\bget\s*[;{]|\bset\s*[;{(]|=>\s*this\.|return\s+(this|self)[.>-]+\w+\s*;?\s*$", re.M),
}


def questions_for(unit: Unit, skip: set[str]) -> dict[str, Any]:
    """The subset worth asking: drop rules already decided mechanically and rules whose precondition is absent."""
    added = unit.added_text  # the construct must be among the ADDED lines, or it is pre-existing and out of scope
    picked = {k: q for k, q in QUESTIONS.items()
              if k not in skip and (k not in _PRECONDITIONS or _PRECONDITIONS[k].search(added))}
    if set(picked) <= {"idiom"}:
        return {}  # nothing to exempt if nothing is being asked
    return picked


def api_key() -> str | None:
    return os.environ.get("TYPESAFE_API_KEY") or None


def ask(state: Any, questions: dict[str, Any], key: str) -> dict[str, Any]:
    payload = json.dumps({"model": MODEL, "state": state, "questions": questions}).encode()
    for attempt in range(6):
        request = urllib.request.Request(ENDPOINT, data=payload, method="POST", headers={
            "Authorization": f"Bearer {key}", "Content-Type": "application/json", "User-Agent": "five-lines-jev/0.1"})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body: dict[str, Any] = json.loads(response.read())
                return body
        except urllib.error.HTTPError as error:
            if error.code in (429, 529):
                time.sleep(2 ** attempt)
                continue
            raise RuntimeError(f"Jev returned HTTP {error.code}: {error.read()[:300]!r}") from error
    raise RuntimeError("Jev is still rate limiting after 6 attempts")


def judge_unit(unit: Unit, skip: set[str], key: str) -> dict[str, float]:
    questions = questions_for(unit, skip)
    if not questions:
        return {}
    state = {"language": unit.language, "name": unit.name, "method": unit.source[:MAX_SOURCE_CHARS]}
    answers = ask(state, questions, key)["answers"]
    return {k: float(a["noul"]) for k, a in answers.items()}


def judge_affixes(names: list[str], key: str) -> float:
    return float(ask({"names": names}, {"affix": AFFIX_QUESTION}, key)["answers"]["affix"]["noul"])
