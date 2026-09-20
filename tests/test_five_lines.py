"""Tests for the mechanical half. They make no network request."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from five_lines import jev, rules  # noqa: E402
from five_lines.diff import FileDiff, parse  # noqa: E402
from five_lines.review import review  # noqa: E402
from five_lines.units import units_for  # noqa: E402

EXAMPLE_DIFF = (ROOT / "examples" / "change.diff").read_text()
EXAMPLE_REPO = ROOT / "examples" / "repo"


def unit_of(source: str, path: str, name: str):
    whole = FileDiff(path, added=dict(enumerate(source.splitlines(), 1)))
    return next(u for u in units_for(whole, source) if u.name == name)


class DiffParsing(unittest.TestCase):
    def test_added_lines_carry_new_file_numbers(self):
        files = {f.path: f for f in parse(EXAMPLE_DIFF)}
        self.assertEqual(set(files), {"orders/pricing.py", "web/cart.ts"})
        self.assertEqual(files["web/cart.ts"].added[10].strip(), "getTotal(): number {")

    def test_deleted_files_and_pure_deletions_are_ignored(self):
        diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n"
        self.assertEqual(parse(diff), [])

    def test_new_file_is_marked(self):
        diff = "diff --git a/n.py b/n.py\nnew file mode 100644\n--- /dev/null\n+++ b/n.py\n@@ -0,0 +1 @@\n+x = 1\n"
        self.assertTrue(parse(diff)[0].is_new)


class UnitExtraction(unittest.TestCase):
    def test_only_methods_containing_added_lines_are_units(self):
        file = next(f for f in parse(EXAMPLE_DIFF) if f.path == "orders/pricing.py")
        names = {u.name for u in units_for(file, (EXAMPLE_REPO / file.path).read_text())}
        self.assertIn("quote", names)
        self.assertNotIn("legacy_report", names)  # long, but untouched by the diff

    def test_brace_method_spans_skip_braces_in_strings_and_comments(self):
        source = 'class A {\n  render(): string {\n    // a } in a comment\n    return "}{";\n  }\n  other(): void {\n    this.x();\n  }\n}\n'
        self.assertEqual((unit_of(source, "a.ts", "render").start, unit_of(source, "a.ts", "render").end), (2, 5))

    def test_without_the_file_the_hunk_is_the_unit(self):
        file = next(f for f in parse(EXAMPLE_DIFF) if f.path == "web/cart.ts")
        self.assertTrue(all(u.kind == "hunk" for u in units_for(file, None)))


class RuleOne(unittest.TestCase):
    def test_python_counts_statements_not_lines(self):
        source = "def f(a):\n    '''doc'''\n    x = g(\n        a,\n        1,\n    )\n    return x\n"
        self.assertEqual(rules.statement_count(unit_of(source, "m.py", "f")), 2)

    def test_brace_wrapped_call_is_one_statement(self):
        source = "function f(a) {\n  const x = g(\n    a,\n    1\n  );\n  return x;\n}\n"
        self.assertEqual(rules.statement_count(unit_of(source, "m.js", "f")), 2)

    def test_a_method_that_did_not_grow_is_not_flagged(self):
        body = "".join(f"    s{i} = {i}\n" for i in range(8))
        unit = unit_of("def f():\n" + body, "m.py", "f")
        self.assertEqual(rules.rule_1(unit, base_count=8), [])
        self.assertEqual(rules.rule_1(unit, base_count=6)[0].severity, "grown")
        self.assertEqual(rules.rule_1(unit, base_count=None)[0].severity, "introduced")


class PythonStructure(unittest.TestCase):
    def test_lone_leading_if_is_compliant(self):
        unit = unit_of("def f(x):\n    if x:\n        return 1\n", "m.py", "f")
        self.assertEqual(rules.rule_3_python(unit), [])

    def test_if_after_other_statements_is_flagged_and_elif_is_not_double_counted(self):
        unit = unit_of("def f(x):\n    y = 1\n    if x:\n        return 1\n    elif y:\n        return 2\n", "m.py", "f")
        self.assertEqual([f.line for f in rules.rule_3_python(unit)], [3])

    def test_exhaustive_match_where_every_arm_returns_is_compliant(self):
        ok = "def f(s):\n    match s:\n        case 'a':\n            return 1\n        case 'b':\n            return 2\n"
        bad = ok + "        case _:\n            return 0\n"
        self.assertEqual(rules.rule_5_python(unit_of(ok, "m.py", "f")), [])
        self.assertEqual(len(rules.rule_5_python(unit_of(bad, "m.py", "f"))), 1)


class InheritanceAndInterfaces(unittest.TestCase):
    REPO = {
        "base.py": "class Concrete:\n    def go(self):\n        return 1\n",
        "port.py": "from typing import Protocol\nclass Port(Protocol):\n    def go(self) -> int: ...\n",
        "impl.py": "class RealPort(Port):\n    def go(self):\n        return 1\n",
        "tests/test_port.py": "class FakePort(Port):\n    def go(self):\n        return 0\n",
        "Shape.java": "public interface Shape { double area(); }",
    }

    def test_bodiless_python_base_is_an_interface(self):
        self.assertIs(rules.base_is_interface("Port", self.REPO), True)
        self.assertIs(rules.base_is_interface("Concrete", self.REPO), False)
        self.assertIs(rules.base_is_interface("Shape", self.REPO), True)
        self.assertIsNone(rules.base_is_interface("SomeLibraryType", self.REPO))

    def test_test_doubles_are_not_implementers(self):
        self.assertEqual(rules.implementers("Port", self.REPO), ["impl.py"])

    def test_exceptions_and_framework_bases_are_not_inheritance_findings(self):
        added = {1: "class Boom(ValueError):", 2: "class Thing(Concrete):", 3: "class Foo extends Bar {"}
        self.assertEqual([(c, b) for _, c, b in rules.inheritance_in(added)], [("Thing", "Concrete"), ("Foo", "Bar")])


class RuleTen(unittest.TestCase):
    def test_antonym_pairs_are_flagged_in_any_casing_and_language(self):
        py = unit_of("def f(self, start_date, end_date, user_id):\n    pass\n", "m.py", "f")
        ts = unit_of("function f(minPrice: number, maxPrice: number) {\n  return 1;\n}\n", "m.ts", "f")
        go = unit_of("func f(fromCity string, toCity string) int {\n\treturn 1\n}\n", "m.go", "f")
        for unit in (py, ts, go):
            self.assertEqual(len(rules.rule_10(unit)[0]), 1, unit.language)

    def test_a_shared_word_alone_is_only_a_candidate(self):
        unit = unit_of("def f(self, billing_address, shipping_address):\n    pass\n", "m.py", "f")
        findings, candidates = rules.rule_10(unit)
        self.assertEqual(findings, [])
        self.assertEqual(candidates, [["billing_address", "shipping_address"]])


class JevQuestions(unittest.TestCase):
    def test_questions_without_their_structural_precondition_are_not_asked(self):
        plain = unit_of("function add(a: number, b: number) {\n  return a + b;\n}\n", "m.ts", "add")
        self.assertEqual(set(jev.questions_for(plain, skip=set())), {"r2", "idiom"})
        branching = unit_of("function f(a) {\n  if (a) {\n    return 1;\n  } else {\n    return 2;\n  }\n}\n", "m.js", "f")
        self.assertLessEqual({"r3", "r4", "r7"}, set(jev.questions_for(branching, skip=set())))


class OnlyWhatTheDiffAdds(unittest.TestCase):
    SOURCE = "def f(x):\n    y = 1\n    if x:\n        return 1\n    z = 2\n    return y + z\n"

    def _review(self, added_line: int):
        diff = ("diff --git a/m.py b/m.py\n--- a/m.py\n+++ b/m.py\n"
                f"@@ -{added_line},0 +{added_line},1 @@\n+{self.SOURCE.splitlines()[added_line - 1]}\n")
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "m.py").write_text(self.SOURCE)
            return review(diff, Path(tmp), base=None, use_jev=False, threshold=0.8).findings

    def test_a_pre_existing_if_in_a_touched_method_is_not_flagged(self):
        self.assertEqual([f.rule for f in self._review(added_line=5)], [])

    def test_an_if_the_diff_added_is_flagged(self):
        self.assertEqual([f.rule for f in self._review(added_line=3)], [3])


class EndToEnd(unittest.TestCase):
    def test_example_diff_without_jev(self):
        result = review(EXAMPLE_DIFF, EXAMPLE_REPO, base=None, use_jev=False, threshold=0.8)
        self.assertFalse(result.jev_used)
        self.assertEqual(sorted({f.rule for f in result.findings}), [1, 3, 4, 5, 6, 8, 10])
        self.assertTrue(all(f.unit != "legacy_report" for f in result.findings))


if __name__ == "__main__":
    unittest.main()
