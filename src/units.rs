//! Find the methods a diff touched. The rules are per-method, so the method is the unit of review.
//!
//! Every supported language is parsed with its tree-sitter grammar. A file in any other
//! language, or a diff reviewed without `--repo`, falls back to the hunk text itself.

use crate::diff::FileDiff;
use crate::lang::{self, Lang};
use tree_sitter::{Node, Parser, Tree};

pub struct ParsedFile {
    pub lang: Lang,
    pub source: String,
    pub tree: Tree,
}

impl ParsedFile {
    pub fn parse(path: &str, source: &str) -> Option<ParsedFile> {
        let lang = Lang::from_path(path)?;
        let mut parser = Parser::new();
        parser.set_language(&lang.grammar()).ok()?;
        let tree = parser.parse(source, None)?;
        Some(ParsedFile { lang, source: source.to_string(), tree })
    }

    pub fn text(&self, node: Node) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// Every function-like node in the file, outermost first.
    pub fn functions(&self) -> Vec<Node<'_>> {
        let mut found = Vec::new();
        collect(self.tree.root_node(), &mut found);
        found
    }

    /// The function node a unit was built from.
    pub fn node_of(&self, unit: &Unit) -> Option<Node<'_>> {
        let (start, end) = unit.bytes?;
        self.functions().into_iter().find(|n| n.start_byte() == start && n.end_byte() == end)
    }
}

fn collect<'t>(node: Node<'t>, found: &mut Vec<Node<'t>>) {
    if lang::is_function(node.kind()) {
        found.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, found);
    }
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub path: String,
    pub language: String,
    pub name: String,
    /// False for a hunk reviewed without its file.
    #[allow(dead_code)]
    pub is_method: bool,
    pub start: usize,
    #[allow(dead_code)]
    pub end: usize,
    pub source: String,
    pub added_lines: Vec<usize>,
    /// Byte range of the function node, when the file was parsed.
    pub bytes: Option<(usize, usize)>,
}

impl Unit {
    /// Only the lines this diff added. A rule about a construct (an if, a switch) is judged on
    /// these: an `if` that was already there is pre-existing and out of scope, even inside a
    /// method the diff touched.
    pub fn added_text(&self) -> String {
        let lines: Vec<&str> = self.source.lines().collect();
        self.added_lines.iter().filter_map(|n| n.checked_sub(self.start).and_then(|i| lines.get(i))).copied().collect::<Vec<_>>().join("\n")
    }

    pub fn added(&self, line: usize) -> bool {
        self.added_lines.contains(&line)
    }
}

pub fn units_for(file: &FileDiff, parsed: Option<&ParsedFile>) -> Vec<Unit> {
    let units = parsed.map(|p| method_units(file, p)).unwrap_or_default();
    if units.is_empty() { hunk_units(file) } else { units }
}

/// Every method of a file, as if the whole file had been added. Used on the BASE version to
/// tell "introduced" from "grown".
pub fn all_units(path: &str, parsed: &ParsedFile) -> Vec<Unit> {
    let everything = FileDiff {
        path: path.to_string(),
        added: parsed.source.lines().enumerate().map(|(i, l)| (i + 1, l.to_string())).collect(),
        ..Default::default()
    };
    method_units(&everything, parsed)
}

fn method_units(file: &FileDiff, parsed: &ParsedFile) -> Vec<Unit> {
    let functions = parsed.functions();
    let mut units: Vec<Unit> = Vec::new();
    for (&line, text) in &file.added {
        if text.trim().is_empty() {
            continue;
        }
        // innermost enclosing method = the narrowest function containing the line
        let holder = functions
            .iter()
            .filter(|n| (n.start_position().row + 1..=n.end_position().row + 1).contains(&line))
            .min_by_key(|n| n.end_byte() - n.start_byte());
        let Some(node) = holder else { continue };
        let bytes = Some((node.start_byte(), node.end_byte()));
        match units.iter_mut().find(|u| u.bytes == bytes) {
            Some(unit) => unit.added_lines.push(line),
            None => units.push(Unit {
                path: file.path.clone(),
                language: parsed.lang.name().to_string(),
                name: name_of(*node, parsed),
                is_method: true,
                start: node.start_position().row + 1,
                end: node.end_position().row + 1,
                source: parsed.text(*node).to_string(),
                added_lines: vec![line],
                bytes,
            }),
        }
    }
    units
}

pub fn name_of(node: Node, parsed: &ParsedFile) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return parsed.text(name).to_string();
    }
    // C and C++ bury the name inside nested declarators.
    if let Some(declarator) = node.child_by_field_name("declarator")
        && let Some(id) = first_of(declarator, &["identifier", "field_identifier", "qualified_identifier"])
    {
        return parsed.text(id).to_string();
    }
    // `const total = (a, b) => ...` — an anonymous function takes the name it is assigned to.
    let owner = node.parent().and_then(|p| p.child_by_field_name("name").or_else(|| p.child_by_field_name("left")));
    owner.map(|n| parsed.text(n).to_string()).unwrap_or_else(|| "<anonymous>".to_string())
}

pub fn first_of<'t>(node: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().find_map(|child| first_of(child, kinds))
}

/// No parse available: review each run of added lines together with the context the hunk carried.
fn hunk_units(file: &FileDiff) -> Vec<Unit> {
    let mut merged = file.context.clone();
    merged.extend(file.added.clone());
    let (Some(&first), Some(&last)) = (merged.keys().next(), merged.keys().next_back()) else {
        return Vec::new();
    };
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for &line in file.added.keys() {
        match runs.last_mut() {
            Some(run) if run.last() == Some(&(line - 1)) => run.push(line),
            _ => runs.push(vec![line]),
        }
    }
    runs.into_iter()
        .map(|run| {
            let start = run[0].saturating_sub(3).max(first);
            let end = (run[run.len() - 1] + 3).min(last);
            let source: Vec<&str> = (start..=end).map(|n| merged.get(&n).map(String::as_str).unwrap_or("")).collect();
            Unit {
                path: file.path.clone(),
                language: Lang::name_for_path(&file.path),
                name: format!("lines {}-{}", run[0], run[run.len() - 1]),
                is_method: false,
                start,
                end,
                source: source.join("\n"),
                added_lines: run,
                bytes: None,
            }
        })
        .filter(|u| !u.source.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff;

    fn example(path: &str) -> (FileDiff, ParsedFile) {
        let file = diff::parse(include_str!("../examples/change.diff")).into_iter().find(|f| f.path == path).unwrap();
        let source = std::fs::read_to_string(format!("{}/examples/repo/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let parsed = ParsedFile::parse(path, &source).unwrap();
        (file, parsed)
    }

    #[test]
    fn only_methods_containing_added_lines_are_units() {
        let (file, parsed) = example("orders/pricing.py");
        let names: Vec<String> = units_for(&file, Some(&parsed)).into_iter().map(|u| u.name).collect();
        assert!(names.contains(&"quote".to_string()));
        assert!(!names.contains(&"legacy_report".to_string()), "long, but untouched by the diff");
    }

    #[test]
    fn typescript_methods_are_found_with_exact_spans() {
        let (file, parsed) = example("web/cart.ts");
        let units = units_for(&file, Some(&parsed));
        let drain = units.iter().find(|u| u.name == "drain").unwrap();
        assert_eq!((drain.start, drain.end), (22, 28));
    }

    #[test]
    fn braces_in_strings_and_comments_do_not_confuse_a_real_parser() {
        let source = "class A {\n  render(): string {\n    // a } in a comment\n    return \"}{\";\n  }\n}\n";
        let parsed = ParsedFile::parse("a.ts", source).unwrap();
        let unit = &all_units("a.ts", &parsed)[0];
        assert_eq!((unit.name.as_str(), unit.start, unit.end), ("render", 2, 5));
    }

    #[test]
    fn without_the_file_the_hunk_is_the_unit() {
        let (file, _) = example("web/cart.ts");
        assert!(units_for(&file, None).iter().all(|u| !u.is_method));
    }

    #[test]
    fn an_arrow_function_takes_the_name_it_is_assigned_to() {
        let parsed = ParsedFile::parse("a.js", "const total = (a, b) => {\n  return a + b;\n};\n").unwrap();
        assert_eq!(all_units("a.js", &parsed)[0].name, "total");
    }
}
