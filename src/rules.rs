//! The ten rules. What can be COUNTED is counted here; what needs judgment becomes a question for Jev.
//!
//! A finding's `basis` says how it was decided:
//!
//! * `mechanical` — by parsing alone. Reproducible, no model involved.
//! * `jev`        — by a typed yes/no question, with the probability attached.
//! * `candidate`  — a structural signal was found, but no judgment was available to confirm it.

use crate::lang::{self, Lang};
use crate::units::{ParsedFile, Unit, first_of};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;
use tree_sitter::Node;

pub const LIMIT: usize = 5;

pub fn rule_name(rule: u8) -> &'static str {
    match rule {
        1 => "Five lines",
        2 => "Call or pass, not both",
        3 => "If only at the start",
        4 => "Never if-else",
        5 => "Never switch",
        6 => "Inherit only from interfaces",
        7 => "Pure conditions",
        8 => "No single-implementation interfaces",
        9 => "Avoid getters/setters",
        _ => "No common affixes",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: u8,
    pub path: String,
    pub line: usize,
    pub unit: String,
    pub message: String,
    pub fix: String,
    pub basis: &'static str,
    pub confidence: Option<f64>,
    pub severity: &'static str,
    pub suppressed: Option<String>,
}

impl Finding {
    pub fn mechanical(rule: u8, unit: &Unit, line: usize, message: String, fix: String) -> Finding {
        Finding {
            rule,
            path: unit.path.clone(),
            line,
            unit: unit.name.clone(),
            message,
            fix,
            basis: "mechanical",
            confidence: None,
            severity: "introduced",
            suppressed: None,
        }
    }

    pub fn raised(&self) -> bool {
        self.suppressed.is_none() && self.severity != "worth a look"
    }
}

// ---------------------------------------------------------------- walking a function

/// Descendants of `node`, not descending into nested functions: those are their own unit.
fn within<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if lang::is_function(child.kind()) {
            continue;
        }
        out.push(child);
        within(child, out);
    }
}

fn descendants(node: Node) -> Vec<Node> {
    let mut out = Vec::new();
    within(node, &mut out);
    out
}

fn line(node: Node) -> usize {
    node.start_position().row + 1
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).filter(|n| !n.kind().contains("comment")).collect()
}

/// The top-level statements of a function body, without a leading Python docstring.
fn body_statements<'t>(function: Node<'t>, parsed: &ParsedFile) -> Vec<Node<'t>> {
    let Some(body) = function.child_by_field_name("body") else { return Vec::new() };
    let mut statements = named_children(body);
    if parsed.lang == Lang::Python && statements.first().is_some_and(|s| is_docstring(*s)) {
        statements.remove(0);
    }
    statements
}

fn is_docstring(statement: Node) -> bool {
    statement.kind() == "expression_statement" && statement.named_child(0).is_some_and(|c| c.kind() == "string")
}

// ---------------------------------------------------------------- rule 1

/// Statements, not lines: one call wrapped over six lines is one statement.
pub fn statement_count(function: Node, parsed: &ParsedFile) -> usize {
    let Some(body) = function.child_by_field_name("body") else { return 0 };
    if !body.kind().contains("block") && body.kind() != "compound_statement" {
        return 1; // an arrow function or lambda whose body is a single expression
    }
    let docstring = named_children(body).first().copied().filter(|s| parsed.lang == Lang::Python && is_docstring(*s));
    let nodes = descendants(body);
    let mut count = nodes.iter().filter(|n| counts_as_statement(**n) && Some(**n) != docstring).count();
    if parsed.lang == Lang::Rust {
        // `a + b` as the last line of a block is Rust's return: a statement in all but grammar.
        let blocks = std::iter::once(body).chain(nodes.iter().copied().filter(|n| n.kind() == "block"));
        count += blocks.filter(|b| named_children(*b).last().is_some_and(|l| !lang::is_statement(l.kind()))).count();
    }
    count
}

fn counts_as_statement(node: Node) -> bool {
    if !lang::is_statement(node.kind()) {
        return false;
    }
    // C#: `var x = 1;` is a local_declaration_statement WRAPPING a variable_declaration. One statement.
    if node.parent().is_some_and(|p| p.kind() == "local_declaration_statement") {
        return false;
    }
    // `for (let i = 0; ...)`: the initialiser belongs to the loop header, not to the budget.
    let in_loop_header =
        node.parent().is_some_and(|p| p.kind().starts_with("for_") && p.child_by_field_name("body").is_some_and(|b| b.id() != node.id()));
    !in_loop_header
}

pub fn rule_1(unit: &Unit, function: Node, parsed: &ParsedFile, base_count: Option<usize>) -> Vec<Finding> {
    let count = statement_count(function, parsed);
    if count <= LIMIT || base_count.is_some_and(|base| count <= base) {
        return Vec::new(); // within budget, or the diff did not make an already-long method longer
    }
    let grown = base_count.filter(|base| *base > LIMIT);
    let size = match count {
        c if c <= LIMIT + 2 => "slightly over",
        c if c <= 15 => "well over",
        _ => "a god-method",
    };
    let message = format!(
        "`{}` has {count} statements ({size}; budget {LIMIT}){}",
        unit.name,
        grown.map(|b| format!(", up from {b}")).unwrap_or_default()
    );
    let fix = match largest_block(function, parsed) {
        Some((kind, at)) => format!("extract the `{kind}` block at line {at} into its own method"),
        None => "extract the longest cohesive run of statements into its own method".to_string(),
    };
    let mut finding = Finding::mechanical(1, unit, unit.start, message, fix);
    finding.severity = if grown.is_some() { "grown" } else { "introduced" };
    vec![finding]
}

fn largest_block(function: Node, parsed: &ParsedFile) -> Option<(String, usize)> {
    body_statements(function, parsed)
        .into_iter()
        .filter(|s| s.end_position().row > s.start_position().row)
        .max_by_key(|s| s.end_position().row - s.start_position().row)
        .map(|s| (s.kind().split('_').next().unwrap_or("block").to_string(), line(s)))
}

// ---------------------------------------------------------------- rule 3 and 4

/// `if` nodes that start their own conditional: an `else if` belongs to the `if` above it.
fn own_ifs(function: Node) -> Vec<Node> {
    descendants(function)
        .into_iter()
        .filter(|n| lang::is_if(n.kind()))
        .filter(|n| {
            let parent = n.parent();
            let is_else_if = parent.is_some_and(|p| {
                p.kind() == "else_clause" || (lang::is_if(p.kind()) && p.child_by_field_name("alternative").map(|a| a.id()) == Some(n.id()))
            });
            !is_else_if
        })
        .collect()
}

pub fn rule_3(unit: &Unit, function: Node, parsed: &ParsedFile) -> Vec<Finding> {
    let body = body_statements(function, parsed);
    own_ifs(function)
        .into_iter()
        .filter(|node| unit.added(line(*node)))
        .filter_map(|node| {
            // The `if` may be wrapped: `expression_statement` in Rust, or be the statement itself.
            let first =
                body.first().is_some_and(|s| s.id() == node.id() || (s.start_byte() == node.start_byte() && s.named_child_count() == 1));
            if first && body.len() == 1 {
                return None;
            }
            let what = if first { "is followed by more statements at the same level" } else { "is not the method's first statement" };
            Some(Finding::mechanical(
                3,
                unit,
                line(node),
                format!("the `if` at line {} in `{}` {what}", line(node), unit.name),
                format!("extract the `if` at line {} and its block into its own method, so it starts that method", line(node)),
            ))
        })
        .collect()
}

/// Lines of if/else constructs the diff added. Whether both branches are OWN domain logic is Jev's question.
pub fn rule_4_candidates(unit: &Unit, function: Node) -> Vec<usize> {
    own_ifs(function)
        .into_iter()
        .filter(|n| {
            let mut cursor = n.walk();
            n.child_by_field_name("alternative").is_some()
                || n.children(&mut cursor).any(|c| matches!(c.kind(), "else_clause" | "elif_clause" | "else"))
        })
        .map(line)
        .filter(|l| unit.added(*l))
        .collect()
}

// ---------------------------------------------------------------- rule 5

pub fn rule_5(unit: &Unit, function: Node, parsed: &ParsedFile) -> Vec<Finding> {
    let mut findings = Vec::new();
    for switch in descendants(function).into_iter().filter(|n| lang::is_switch(n.kind()) && unit.added(line(*n))) {
        let arms: Vec<Node> = descendants(switch)
            .into_iter()
            .filter(|n| lang::is_arm(n.kind()) && nearest_switch(*n).map(|s| s.id()) == Some(switch.id()))
            .collect();
        let word = parsed.text(switch).split(|c: char| !c.is_alphanumeric()).next().unwrap_or("switch").to_string();
        if arms.iter().any(|arm| is_catch_all(*arm, parsed)) {
            findings.push(Finding::mechanical(
                5,
                unit,
                line(switch),
                format!("the `{word}` at line {} has a catch-all arm", line(switch)),
                "list every case explicitly so a new variant fails loudly, or replace with polymorphism".to_string(),
            ));
        } else {
            let open = arms.iter().filter(|arm| arm_does_not_return(**arm)).count();
            if open > 0 {
                findings.push(Finding::mechanical(
                    5,
                    unit,
                    line(switch),
                    format!("{open} arm(s) of the `{word}` at line {} do not return", line(switch)),
                    "make every arm return; move any work after the switch into the arms or a new method".to_string(),
                ));
            }
        }
    }
    findings
}

fn nearest_switch(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(n) = current {
        if lang::is_switch(n.kind()) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

fn is_catch_all(arm: Node, parsed: &ParsedFile) -> bool {
    let text = parsed.text(arm).trim_start();
    let pattern = arm.child_by_field_name("pattern").map(|p| parsed.text(p).trim().to_string());
    matches!(arm.kind(), "switch_default" | "default_case" | "default_statement")
        || text.starts_with("default")
        || text.starts_with("case _:")
        || text.starts_with("case _ ")
        || pattern.as_deref() == Some("_")
}

fn arm_does_not_return(arm: Node) -> bool {
    // `case A -> value`, `Tier::Pro => 900`: the arm IS a value, so it returns by construction.
    // (tree-sitter-java calls the classic statement switch `switch_expression` too, so this is
    // decided per arm, not per switch.)
    if matches!(arm.kind(), "switch_rule" | "switch_expression_arm" | "match_arm") {
        return false;
    }
    // The arm's own statements: its children, or the children of its block.
    let mut statements: Vec<Node> = Vec::new();
    for child in named_children(arm) {
        if child.kind().contains("block") || child.kind() == "compound_statement" {
            statements.extend(named_children(child));
        } else {
            statements.push(child);
        }
    }
    statements.retain(|n| lang::is_statement(n.kind()) || lang::ends_the_arm(n.kind()));
    // No statements at all is a bare label grouped with the arm below it: `case "a": case "b": return 1`
    statements.last().is_some_and(|last| !lang::ends_the_arm(last.kind()))
}

// ---------------------------------------------------------------- rule 6 and 8 (need the repo)

const IDIOM_BASES: &[&str] = &[
    "Exception",
    "BaseException",
    "Error",
    "Protocol",
    "ABC",
    "Enum",
    "IntEnum",
    "StrEnum",
    "Flag",
    "TypedDict",
    "NamedTuple",
    "BaseModel",
    "object",
    "Generic",
    "TestCase",
    "Component",
    "PureComponent",
];
static EXTENDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bclass\s+(?P<cls>\w+)(?:<[^>]*>)?\s+extends\s+(?P<base>[\w.\\]+)").unwrap());
static PY_CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*class\s+(?P<cls>\w+)\s*\((?P<bases>[^)]*)\)\s*:").unwrap());
static INTERFACE_DECL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:interface|trait|protocol)\s+(?P<name>\w+)").unwrap());
static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^|/)(tests?|spec|__tests__|mocks?|fakes?|stubs?|fixtures)(/|$)|[._-](test|spec|mock|fake|stub)s?\.").unwrap()
});

fn short(name: &str) -> &str {
    name.trim().split('[').next().unwrap_or("").rsplit(['.', '\\']).next().unwrap_or("")
}

/// (line, class, base) for every inheritance the diff ADDS.
pub fn inheritance_in(added: &BTreeMap<usize, String>) -> Vec<(usize, String, String)> {
    let mut found = Vec::new();
    for (&line, text) in added {
        if let Some(m) = EXTENDS.captures(text) {
            found.push((line, m["cls"].to_string(), short(&m["base"]).to_string()));
        } else if let Some(m) = PY_CLASS.captures(text) {
            for base in m["bases"].split(',').filter(|b| !b.contains('=')) {
                found.push((line, m["cls"].to_string(), short(base).to_string()));
            }
        }
    }
    found.retain(|(_, _, base)| {
        !base.is_empty() && !IDIOM_BASES.contains(&base.as_str()) && !base.ends_with("Error") && !base.ends_with("Exception")
    });
    found
}

pub fn interfaces_in(added: &BTreeMap<usize, String>) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (&line, text) in added {
        if let Some(m) = INTERFACE_DECL.captures(text) {
            found.push((line, m["name"].to_string()));
        } else if let Some(m) = PY_CLASS.captures(text)
            && m["bases"].split(',').any(|b| matches!(short(b), "Protocol" | "ABC"))
        {
            found.push((line, m["cls"].to_string()));
        }
    }
    found
}

/// Some(true) = bodiless contract, Some(false) = carries implementation, None = not in the repo (a library type?).
pub fn base_is_interface(base: &str, repo: &BTreeMap<String, String>) -> Option<bool> {
    let name = regex::escape(base);
    let contract = Regex::new(&format!(r"\b(?:interface|trait|protocol)\s+{name}\b")).unwrap();
    let py_class = Regex::new(&format!(r"(?m)^\s*class\s+{name}\b")).unwrap();
    let class = Regex::new(&format!(r"\b(?:abstract\s+)?class\s+{name}\b")).unwrap();
    for (path, source) in repo {
        if contract.is_match(source) {
            return Some(true);
        }
        if path.ends_with(".py") && py_class.is_match(source) {
            return python_class_is_bodiless(path, source, base);
        }
        if class.is_match(source) {
            return Some(false);
        }
    }
    None
}

fn python_class_is_bodiless(path: &str, source: &str, name: &str) -> Option<bool> {
    let parsed = ParsedFile::parse(path, source)?;
    let root = parsed.tree.root_node();
    let class = descendants(root)
        .into_iter()
        .chain(std::iter::once(root))
        .find(|n| n.kind() == "class_definition" && n.child_by_field_name("name").is_some_and(|id| parsed.text(id) == name))?;
    let mut methods = Vec::new();
    collect_functions(class, &mut methods);
    Some(methods.into_iter().all(|m| {
        body_statements(m, &parsed)
            .iter()
            .all(|s| matches!(s.kind(), "pass_statement" | "raise_statement") || is_docstring(*s) || parsed.text(*s).trim() == "...")
    }))
}

fn collect_functions<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if lang::is_function(child.kind()) {
            out.push(child);
        } else {
            collect_functions(child, out);
        }
    }
}

/// Files that implement `interface`, not counting test doubles: a mock is not a second implementation.
pub fn implementers(interface: &str, repo: &BTreeMap<String, String>) -> Vec<String> {
    let n = regex::escape(interface);
    let patterns: Vec<Regex> = [
        format!(r"\bimplements\b[^{{]*\b{n}\b"),
        format!(r"(?m)^\s*class\s+\w+\s*\([^)]*\b{n}\b[^)]*\)\s*:"),
        format!(r"\bimpl(?:<[^>]*>)?\s+{n}\b"),
        format!(r"\bclass\s+\w+[^{{\n]*:\s*[^{{\n]*\b{n}\b"),
        format!(r"\buse\s+{n}\s*;"),
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect();
    repo.iter()
        .filter(|(path, source)| !TEST_PATH.is_match(path) && patterns.iter().any(|p| p.is_match(source)))
        .map(|(path, _)| path.clone())
        .collect()
}

// ---------------------------------------------------------------- rule 10

const PAIRS: &[(&str, &str)] = &[
    ("start", "end"),
    ("begin", "end"),
    ("min", "max"),
    ("from", "to"),
    ("first", "last"),
    ("old", "new"),
    ("src", "dst"),
    ("source", "target"),
    ("source", "destination"),
    ("lower", "upper"),
    ("left", "right"),
    ("before", "after"),
    ("prev", "next"),
    ("previous", "next"),
    ("in", "out"),
    ("input", "output"),
    ("x", "y"),
    ("lat", "lng"),
    ("lat", "lon"),
    ("latitude", "longitude"),
    ("width", "height"),
];
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]+(?:[a-z]+)?|[a-z]+|\d+").unwrap());
static SELF_FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*self\.(\w+)\s*(?::[^=\n]+)?=").unwrap());

pub fn words(identifier: &str) -> Vec<String> {
    let spaced = identifier.replace(['_', '$'], " ");
    WORD.find_iter(&spaced).map(|m| m.as_str().to_lowercase()).collect()
}

/// Parameters and fields this method declares — the siblings rule 10 compares.
pub fn declared_names(function: Node, parsed: &ParsedFile) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let parameters = function
        .child_by_field_name("parameters")
        .or_else(|| function.child_by_field_name("declarator").and_then(|d| first_of(d, &["parameter_list"])));
    for parameter in parameters.map(named_children).unwrap_or_default() {
        let holder = ["name", "pattern", "declarator"].iter().find_map(|f| parameter.child_by_field_name(f)).unwrap_or(parameter);
        if let Some(id) = first_of(holder, &["identifier", "variable_name", "name"]) {
            names.push(parsed.text(id).trim_start_matches('$').to_string());
        }
    }
    if parsed.lang == Lang::Python {
        names.extend(SELF_FIELD.captures_iter(parsed.text(function)).map(|c| c[1].to_string()));
    }
    let mut seen = BTreeSet::new();
    names.retain(|n| !matches!(n.as_str(), "self" | "cls" | "this") && seen.insert(n.clone()));
    names
}

/// Known antonym pairs are flagged mechanically; other shared affixes are returned as candidates for Jev.
pub fn rule_10(unit: &Unit, names: &[String]) -> (Vec<Finding>, Vec<Vec<String>>) {
    let mut findings = Vec::new();
    let mut flagged: BTreeSet<&String> = BTreeSet::new();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            let (wa, wb) = (words(a), words(b));
            if wa.len() != wb.len() || wa.len() < 2 {
                continue;
            }
            let differing: Vec<usize> = (0..wa.len()).filter(|k| wa[*k] != wb[*k]).collect();
            let [k] = differing[..] else { continue };
            let is_pair = PAIRS.iter().any(|(p, q)| (wa[k] == *p && wb[k] == *q) || (wa[k] == *q && wb[k] == *p));
            if !is_pair || (k != 0 && k != wa.len() - 1) {
                continue;
            }
            let shared: String = wa.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, w)| capitalise(w)).collect();
            flagged.extend([a, b]);
            findings.push(Finding::mechanical(
                10,
                unit,
                unit.start,
                format!("`{a}` and `{b}` differ only by the affix {}/{}", wa[k], wb[k]),
                format!("introduce a `{shared}Range`-style type holding both, and move the logic that uses them onto it"),
            ));
        }
    }
    let mut groups: BTreeMap<(bool, Vec<String>), Vec<String>> = BTreeMap::new();
    for name in names.iter().filter(|n| !flagged.contains(n)) {
        let w = words(name);
        if w.len() >= 2 {
            groups.entry((true, w[..w.len() - 1].to_vec())).or_default().push(name.clone());
            groups.entry((false, w[1..].to_vec())).or_default().push(name.clone());
        }
    }
    let candidates: BTreeSet<Vec<String>> = groups
        .into_iter()
        .filter(|((_, shared), members)| members.len() >= 2 && shared.concat().len() >= 4)
        .map(|(_, mut members)| {
            members.sort();
            members
        })
        .collect();
    (findings, candidates.into_iter().collect())
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
