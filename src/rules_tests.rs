use super::*;
use crate::units::all_units;

fn with<T>(path: &str, source: &str, name: &str, check: impl FnOnce(&Unit, Node, &ParsedFile) -> T) -> T {
    let parsed = ParsedFile::parse(path, source).expect("supported language");
    let unit = all_units(path, &parsed).into_iter().find(|u| u.name == name).unwrap_or_else(|| panic!("no unit {name} in {path}"));
    let node = parsed.node_of(&unit).expect("unit has a node");
    check(&unit, node, &parsed)
}

fn count(path: &str, source: &str, name: &str) -> usize {
    with(path, source, name, |_, node, parsed| statement_count(node, parsed))
}

#[test]
fn statements_are_counted_not_lines_in_every_language() {
    assert_eq!(count("m.py", "def f(a):\n    \"\"\"doc\"\"\"\n    x = g(\n        a,\n        1,\n    )\n    return x\n", "f"), 2);
    assert_eq!(count("m.js", "function f(a) {\n  const x = g(\n    a,\n    1\n  );\n  return x;\n}\n", "f"), 2);
    assert_eq!(count("m.ts", "class A {\n  f(a: number): number {\n    const x = a * 2;\n    return x;\n  }\n}\n", "f"), 2);
    assert_eq!(count("M.java", "class M {\n  int f(int a) {\n    int x = a * 2;\n    log(x);\n    return x;\n  }\n}\n", "f"), 3);
    assert_eq!(count("m.go", "package m\nfunc f(a int) int {\n\tx := a * 2\n\treturn x\n}\n", "f"), 2);
    assert_eq!(count("m.php", "<?php\nfunction f($a) {\n    $x = $a * 2;\n    return $x;\n}\n", "f"), 2);
    assert_eq!(count("m.c", "int f(int a) {\n    int x = a * 2;\n    return x;\n}\n", "f"), 2);
    assert_eq!(count("M.cs", "class M {\n  int F(int a) {\n    var x = a * 2;\n    return x;\n  }\n}\n", "F"), 2);
}

#[test]
fn a_rust_tail_expression_is_a_statement_in_all_but_grammar() {
    assert_eq!(count("m.rs", "fn f(a: i32) -> i32 {\n    let x = a * 2;\n    x + 1\n}\n", "f"), 2);
}

#[test]
fn a_loop_header_does_not_spend_the_budget() {
    let source = "function f(xs) {\n  for (let i = 0; i < xs.length; i++) {\n    use(xs[i]);\n  }\n}\n";
    assert_eq!(count("m.js", source, "f"), 2); // the for, and the call inside it
}

#[test]
fn a_method_that_did_not_grow_is_not_flagged() {
    let body: String = (0..8).map(|i| format!("    s{i} = {i}\n")).collect();
    with("m.py", &format!("def f():\n{body}"), "f", |unit, node, parsed| {
        assert!(rule_1(unit, node, parsed, Some(8)).is_empty());
        assert_eq!(rule_1(unit, node, parsed, Some(6))[0].severity, "grown");
        assert_eq!(rule_1(unit, node, parsed, None)[0].severity, "introduced");
    });
}

fn rule_3_lines(path: &str, source: &str) -> Vec<usize> {
    with(path, source, "f", |unit, node, parsed| rule_3(unit, node, parsed).iter().map(|f| f.line).collect())
}

#[test]
fn a_lone_leading_if_is_compliant_everywhere() {
    assert!(rule_3_lines("m.py", "def f(x):\n    if x:\n        return 1\n").is_empty());
    assert!(rule_3_lines("m.ts", "function f(x: number) {\n  if (x) {\n    return 1;\n  }\n}\n").is_empty());
    assert!(rule_3_lines("m.go", "package m\nfunc f(x bool) {\n\tif x {\n\t\tgoOn()\n\t}\n}\n").is_empty());
    assert!(rule_3_lines("m.rs", "fn f(x: bool) {\n    if x {\n        go_on();\n    }\n}\n").is_empty());
}

#[test]
fn an_if_after_other_statements_is_flagged_and_else_if_is_not_double_counted() {
    assert_eq!(rule_3_lines("m.py", "def f(x):\n    y = 1\n    if x:\n        return 1\n    elif y:\n        return 2\n"), [3]);
    let ts = "function f(x: number) {\n  const y = 1;\n  if (x) {\n    return 1;\n  } else if (y) {\n    return 2;\n  }\n}\n";
    assert_eq!(rule_3_lines("m.ts", ts), [3]);
    let java = "class M {\n  int f(int x) {\n    int y = 1;\n    if (x > 0) {\n      return 1;\n    }\n    return y;\n  }\n}\n";
    assert_eq!(rule_3_lines("M.java", java), [4]);
}

fn rule_5_messages(path: &str, source: &str) -> Vec<String> {
    with(path, source, "f", |unit, node, parsed| rule_5(unit, node, parsed).into_iter().map(|f| f.message).collect())
}

#[test]
fn a_catch_all_arm_is_flagged_in_every_language() {
    let cases = [
        ("m.py", "def f(s):\n    match s:\n        case 'a':\n            return 1\n        case _:\n            return 0\n"),
        ("m.ts", "function f(s: string) {\n  switch (s) {\n    case 'a':\n      return 1;\n    default:\n      return 0;\n  }\n}\n"),
        (
            "M.java",
            "class M {\n  int f(int s) {\n    switch (s) {\n      case 1:\n        return 1;\n      default:\n        return 0;\n    }\n  }\n}\n",
        ),
        ("m.go", "package m\nfunc f(s int) int {\n\tswitch s {\n\tcase 1:\n\t\treturn 1\n\tdefault:\n\t\treturn 0\n\t}\n}\n"),
        ("m.rs", "fn f(s: u8) -> u8 {\n    match s {\n        1 => 1,\n        _ => 0,\n    }\n}\n"),
        (
            "m.c",
            "int f(int s) {\n    switch (s) {\n        case 1:\n            return 1;\n        default:\n            return 0;\n    }\n}\n",
        ),
        (
            "m.php",
            "<?php\nfunction f($s) {\n    switch ($s) {\n        case 1:\n            return 1;\n        default:\n            return 0;\n    }\n}\n",
        ),
        (
            "M.cs",
            "class M {\n  int f(int s) {\n    switch (s) {\n      case 1:\n        return 1;\n      default:\n        return 0;\n    }\n  }\n}\n",
        ),
    ];
    for (path, source) in cases {
        let messages = rule_5_messages(path, source);
        assert!(messages.len() == 1 && messages[0].contains("catch-all"), "{path}: {messages:?}");
    }
}

#[test]
fn an_exhaustive_switch_where_every_arm_returns_is_the_approved_shape() {
    let ts = "function f(s: 'a' | 'b') {\n  switch (s) {\n    case 'a':\n      return 1;\n    case 'b':\n      return 2;\n  }\n}\n";
    assert!(rule_5_messages("m.ts", ts).is_empty());
    let rust = "fn f(t: Tier) -> u32 {\n    match t {\n        Tier::Free => 0,\n        Tier::Pro => 900,\n    }\n}\n";
    assert!(rule_5_messages("m.rs", rust).is_empty());
    let grouped =
        "function f(s: string) {\n  switch (s) {\n    case 'a':\n    case 'b':\n      return 1;\n    case 'c':\n      return 2;\n  }\n}\n";
    assert!(rule_5_messages("m.ts", grouped).is_empty(), "grouped labels are not fall-through bugs");
}

#[test]
fn an_arm_that_breaks_instead_of_returning_is_flagged() {
    let java = "class M {\n  void f(int k) {\n    switch (k) {\n      case 1:\n        add();\n        break;\n      case 2:\n        remove();\n        break;\n    }\n  }\n}\n";
    let messages = rule_5_messages("M.java", java);
    assert!(messages.len() == 1 && messages[0].starts_with("2 arm(s)"), "{messages:?}");
}

fn repo() -> BTreeMap<String, String> {
    BTreeMap::from(
        [
            ("base.py", "class Concrete:\n    def go(self):\n        return 1\n"),
            ("port.py", "from typing import Protocol\nclass Port(Protocol):\n    def go(self) -> int: ...\n"),
            ("impl.py", "class RealPort(Port):\n    def go(self):\n        return 1\n"),
            ("tests/test_port.py", "class FakePort(Port):\n    def go(self):\n        return 0\n"),
            ("Shape.java", "public interface Shape { double area(); }"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
}

#[test]
fn a_bodiless_base_is_an_interface_and_a_library_type_is_unknown() {
    assert_eq!(base_is_interface("Port", &repo()), Some(true));
    assert_eq!(base_is_interface("Concrete", &repo()), Some(false));
    assert_eq!(base_is_interface("Shape", &repo()), Some(true));
    assert_eq!(base_is_interface("SomeLibraryType", &repo()), None);
}

#[test]
fn test_doubles_are_not_implementers() {
    assert_eq!(implementers("Port", &repo()), ["impl.py"]);
}

#[test]
fn exceptions_and_framework_bases_are_not_inheritance_findings() {
    let added = BTreeMap::from([
        (1, "class Boom(ValueError):".to_string()),
        (2, "class Thing(Concrete):".to_string()),
        (3, "class Foo extends Bar {".to_string()),
    ]);
    let found: Vec<(String, String)> = inheritance_in(&added).into_iter().map(|(_, c, b)| (c, b)).collect();
    assert_eq!(found, [("Thing".to_string(), "Concrete".to_string()), ("Foo".to_string(), "Bar".to_string())]);
}

fn affixes(path: &str, source: &str) -> (usize, Vec<Vec<String>>) {
    with(path, source, "f", |unit, node, parsed| {
        let (findings, candidates) = rule_10(unit, &declared_names(node, parsed));
        (findings.len(), candidates)
    })
}

#[test]
fn antonym_pairs_are_flagged_in_any_casing_and_language() {
    let cases = [
        ("m.py", "def f(self, start_date, end_date, user_id):\n    pass\n"),
        ("m.ts", "function f(minPrice: number, maxPrice: number) {\n  return 1;\n}\n"),
        ("m.go", "package m\nfunc f(fromCity string, toCity string) int {\n\treturn 1\n}\n"),
        ("M.java", "class M {\n  int f(int firstRow, int lastRow) {\n    return 1;\n  }\n}\n"),
        ("m.php", "<?php\nfunction f($oldName, $newName) {\n    return 1;\n}\n"),
        ("m.c", "int f(int src_len, int dst_len) {\n    return 1;\n}\n"),
        ("m.rs", "fn f(lower_bound: u8, upper_bound: u8) -> u8 {\n    1\n}\n"),
        ("M.cs", "class M {\n  int f(int leftPad, int rightPad) {\n    return 1;\n  }\n}\n"),
    ];
    for (path, source) in cases {
        assert_eq!(affixes(path, source).0, 1, "{path}");
    }
}

#[test]
fn a_shared_word_alone_is_only_a_candidate_for_judgment() {
    let (findings, candidates) = affixes("m.py", "def f(self, billing_address, shipping_address):\n    pass\n");
    assert_eq!(findings, 0);
    assert_eq!(candidates, [["billing_address".to_string(), "shipping_address".to_string()]]);
}
