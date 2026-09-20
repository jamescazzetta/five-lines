//! Run the ten rules over a diff: mechanical checks first, Jev only where judgment is needed.

use crate::diff::{self, FileDiff};
use crate::jev;
use crate::lang::Lang;
use crate::rules::{self, Finding};
use crate::units::{self, ParsedFile, Unit};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Methods per Jev request unless --batch says otherwise. Measured with `five-lines eval --batch N`
/// (see the README): 8 cuts requests eightfold while compliant code stays well below the reporting
/// floor; one request for everything is faster still but eats most of that margin.
pub const DEFAULT_BATCH: usize = 8;
const IDIOM_THRESHOLD: f64 = 0.70;
const LOOK_THRESHOLD: f64 = 0.55;

pub struct Options {
    pub repo: Option<PathBuf>,
    pub base: Option<String>,
    pub use_jev: bool,
    pub threshold: f64,
    /// Methods per Jev request.
    pub batch: usize,
}

#[derive(Default)]
pub struct Review {
    pub findings: Vec<Finding>,
    pub units: usize,
    pub jev_requests: usize,
    pub jev_used: bool,
    pub notes: Vec<String>,
}

/// (rule, message, fix) for each judgment Jev can raise. `{name}` is the method.
fn jev_rule(question: &str) -> Option<(u8, &'static str, &'static str)> {
    Some(match question {
        "r2" => (
            2,
            "`{name}` both orchestrates collaborators and computes on raw values",
            "keep the calls in `{name}`; move the inline computation into a method that is handed the values",
        ),
        "r3" => (
            3,
            "`{name}` has an `if` that is not its first statement, or code after its if/else block",
            "extract the `if` and its block into its own method, so the condition starts that method",
        ),
        "r4" => (
            4,
            "`{name}` branches with if/else between two pieces of its own domain logic",
            "introduce one interface with a class per branch, and push each branch's body into its class",
        ),
        "r5" => (
            5,
            "`{name}` has a switch with a catch-all arm, or an arm that does not return",
            "drop the catch-all so a new variant fails loudly and make every arm return; or replace with polymorphism",
        ),
        "r7" => (
            7,
            "a condition in `{name}` has a side effect",
            "hoist the side effect into its own statement before the condition; split the query from the command",
        ),
        "r9" => (
            9,
            "`{name}` is an accessor that invites callers to do the object's work",
            "move the caller logic that uses `{name}` into the owning class (push code into data), then remove the accessor",
        ),
        _ => return None,
    })
}

pub fn git_diff(repo: &Path, base: &str) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--unified=3", &format!("{base}...HEAD")])
        .output()
        .context("could not run git")?;
    anyhow::ensure!(out.status.success(), "git diff failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A changed method after the mechanical pass, waiting for Jev's judgment.
struct Pending {
    unit: Unit,
    findings: Vec<Finding>,
    else_lines: Vec<usize>,
    item: jev::Item,
}

pub fn review(diff_text: &str, options: &Options) -> Result<Review> {
    let mut result = Review::default();
    let key = if options.use_jev { jev::api_key() } else { None };
    result.jev_used = key.is_some();
    if options.use_jev && key.is_none() {
        result.notes.push(
            "TYPESAFE_API_KEY is not set: the judgment rules (2, 7, 9, and the confirmation of 4 and 10) were not evaluated.".to_string(),
        );
    }
    // Pass 1, per file: everything a parser can decide.
    let mut pending: Vec<Pending> = Vec::new();
    let mut repo_sources: Option<BTreeMap<String, String>> = None;
    for file in diff::parse(diff_text) {
        let source = options.repo.as_ref().and_then(|repo| std::fs::read_to_string(repo.join(&file.path)).ok());
        if source.is_none() && options.repo.is_some() {
            result.notes.push(format!("{}: not found under --repo, reviewed from the hunk text only", file.path));
        }
        let parsed = source.as_deref().and_then(|s| ParsedFile::parse(&file.path, s));
        if source.is_some() && parsed.is_none() && Lang::from_path(&file.path).is_none() {
            result.notes.push(format!("{}: no parser for this language, mechanical rules skipped", file.path));
        }
        let base = base_version(options, &file);
        let base_units = base.as_ref().map(|b| units::all_units(&file.path, b)).unwrap_or_default();

        for unit in units::units_for(&file, parsed.as_ref()) {
            let base_count = base.as_ref().and_then(|b| base_statement_count(&unit, &base_units, b));
            pending.push(mechanical_pass(unit, parsed.as_ref(), base_count));
        }
        if !rules::inheritance_in(&file.added).is_empty() || !rules::interfaces_in(&file.added).is_empty() {
            let sources = repo_sources.get_or_insert_with(|| read_repo(options.repo.as_deref()));
            result.findings.extend(structure_findings(&file, sources, options.repo.is_some()));
        }
    }
    result.units = pending.len();

    // Pass 2, across the whole diff: everything that needs judgment, in as few requests as allowed.
    let answers = match &key {
        Some(key) => {
            let items: Vec<&jev::Item> = pending.iter().map(|p| &p.item).collect();
            let (answers, requests) = jev::judge(&items, options.batch, key)?;
            result.jev_requests = requests;
            Some(answers)
        }
        None => None,
    };

    // Pass 3: turn probabilities into findings.
    for (index, waiting) in pending.into_iter().enumerate() {
        let judged = answers.as_ref().map(|all| &all[index]);
        result.findings.extend(settle(waiting, judged, options.threshold));
    }
    result.findings.sort_by(|a, b| (a.rule, &a.path, a.line).cmp(&(b.rule, &b.path, b.line)));
    Ok(result)
}

fn mechanical_pass(unit: Unit, parsed: Option<&ParsedFile>, base_count: Option<usize>) -> Pending {
    let mut findings = Vec::new();
    let mut decided: Vec<&str> = Vec::new(); // questions already answered by parsing
    let mut else_lines = Vec::new();
    let mut affixes = Vec::new();

    if let Some((parsed, function)) = parsed.and_then(|p| p.node_of(&unit).map(|f| (p, f))) {
        findings.extend(rules::rule_1(&unit, function, parsed, base_count));
        findings.extend(rules::rule_3(&unit, function, parsed));
        findings.extend(rules::rule_5(&unit, function, parsed));
        decided.extend(["r3", "r5"]);
        else_lines = rules::rule_4_candidates(&unit, function);
        if else_lines.is_empty() {
            decided.push("r4");
        }
        let (affix_findings, candidates) = rules::rule_10(&unit, &rules::declared_names(function, parsed));
        findings.extend(affix_findings);
        affixes = candidates;
    }
    let item = jev::Item {
        language: unit.language.clone(),
        name: unit.name.clone(),
        source: unit.source.clone(),
        questions: jev::questions_for(&unit, &decided),
        affixes,
    };
    Pending { unit, findings, else_lines, item }
}

fn settle(waiting: Pending, answers: Option<&BTreeMap<String, f64>>, threshold: f64) -> Vec<Finding> {
    let Pending { unit, mut findings, else_lines, item } = waiting;
    let Some(answers) = answers else {
        if let Some(&line) = else_lines.first() {
            let (_, _, fix) = jev_rule("r4").expect("r4 is a Jev rule");
            let mut finding = Finding::mechanical(
                4,
                &unit,
                line,
                format!(
                    "if/else at line {line} in `{}` (unconfirmed: needs judgment on whether both branches are your own domain logic)",
                    unit.name
                ),
                fix.replace("{name}", &unit.name),
            );
            finding.basis = "candidate";
            findings.push(finding);
        }
        return findings;
    };

    for (question, &probability) in answers {
        let Some((rule, message, fix)) = jev_rule(question).filter(|_| probability >= LOOK_THRESHOLD) else { continue };
        let line = if question == "r4" { else_lines.first().copied().unwrap_or(unit.start) } else { unit.start };
        let (message, fix) = (message.replace("{name}", &unit.name), fix.replace("{name}", &unit.name));
        findings.push(judged(rule, &unit, line, message, fix, probability, threshold));
    }
    for (n, names) in item.affixes.iter().enumerate() {
        let Some(&probability) = answers.get(&format!("affix{n}")).filter(|p| **p >= LOOK_THRESHOLD) else { continue };
        let listed = names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
        findings.push(judged(
            10,
            &unit,
            unit.start,
            format!("{listed} share an affix and look like one concept"),
            "introduce a type that holds them together, and move the logic that uses them onto it".to_string(),
            probability,
            threshold,
        ));
    }
    if let Some(idiom) = answers.get("idiom").filter(|p| **p >= IDIOM_THRESHOLD) {
        for finding in &mut findings {
            finding.suppressed = Some(format!("framework or language idiom (p={idiom:.2})"));
        }
    }
    findings
}

fn judged(rule: u8, unit: &Unit, line: usize, message: String, fix: String, probability: f64, threshold: f64) -> Finding {
    let mut finding = Finding::mechanical(rule, unit, line, message, fix);
    finding.basis = "jev";
    finding.confidence = Some(probability);
    finding.severity = if probability >= threshold { "introduced" } else { "worth a look" };
    finding
}

fn structure_findings(file: &FileDiff, repo: &BTreeMap<String, String>, have_repo: bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let finding = |rule: u8, line: usize, unit: &str, message: String, fix: String, basis: &'static str| Finding {
        rule,
        path: file.path.clone(),
        line,
        unit: unit.to_string(),
        message,
        fix,
        basis,
        confidence: None,
        severity: "introduced",
        suppressed: None,
    };
    for (line, class, base) in rules::inheritance_in(&file.added) {
        let (detail, basis) = match rules::base_is_interface(&base, repo) {
            Some(true) => continue,
            Some(false) => (", which carries method bodies: that is inheriting implementation", "mechanical"),
            None => (", which was not found in the repo (a library type? then this is fine)", "candidate"),
        };
        findings.push(finding(
            6,
            line,
            &class,
            format!("`{class}` inherits from `{base}`{detail}"),
            format!("give `{class}` a `{base}` field and delegate to it; share the contract through an interface"),
            basis,
        ));
    }
    if have_repo {
        for (line, name) in rules::interfaces_in(&file.added) {
            let users = rules::implementers(&name, repo);
            if users.len() <= 1 {
                let who = users.first().map(|u| format!(": {u}")).unwrap_or_default();
                findings.push(finding(
                    8,
                    line,
                    &name,
                    format!("interface `{name}` has {} non-test implementer(s){who}", users.len()),
                    format!("delete `{name}` and use the concrete class directly until a second implementation exists"),
                    "mechanical",
                ));
            }
        }
    }
    findings
}

fn base_version(options: &Options, file: &FileDiff) -> Option<ParsedFile> {
    let (repo, base) = (options.repo.as_ref()?, options.base.as_ref()?);
    if file.is_new {
        return None;
    }
    let shown = Command::new("git").arg("-C").arg(repo).args(["show", &format!("{base}:{}", file.path)]).output().ok()?;
    shown.status.success().then(|| ParsedFile::parse(&file.path, &String::from_utf8_lossy(&shown.stdout))).flatten()
}

/// The same method in the base version: by name, and nearest in position if the name repeats.
fn base_statement_count(unit: &Unit, base_units: &[Unit], base: &ParsedFile) -> Option<usize> {
    let twin = base_units.iter().filter(|b| b.name == unit.name).min_by_key(|b| b.start.abs_diff(unit.start))?;
    base.node_of(twin).map(|node| rules::statement_count(node, base))
}

fn read_repo(repo: Option<&Path>) -> BTreeMap<String, String> {
    let Some(repo) = repo else { return BTreeMap::new() };
    ignore::WalkBuilder::new(repo)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .filter(|entry| Lang::from_path(&entry.path().to_string_lossy()).is_some())
        .filter(|entry| entry.metadata().is_ok_and(|m| m.len() <= 1_000_000))
        .take(5000)
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(repo).ok()?.to_string_lossy().replace('\\', "/");
            Some((relative, std::fs::read_to_string(entry.path()).ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline(repo: &Path) -> Options {
        Options { repo: Some(repo.to_path_buf()), base: None, use_jev: false, threshold: 0.8, batch: 1 }
    }

    #[test]
    fn the_bundled_example_without_jev() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/repo");
        let result = review(include_str!("../examples/change.diff"), &offline(&repo)).unwrap();
        assert!(!result.jev_used);
        let rules: std::collections::BTreeSet<u8> = result.findings.iter().map(|f| f.rule).collect();
        assert_eq!(rules.into_iter().collect::<Vec<_>>(), [1, 3, 4, 5, 6, 8, 10]);
        assert!(result.findings.iter().all(|f| f.unit != "legacy_report"), "long, but untouched by the diff");
    }

    const SOURCE: &str = "def f(x):\n    y = 1\n    if x:\n        return 1\n    z = 2\n    return y + z\n";

    fn rules_when_adding(line: usize) -> Vec<u8> {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("m.py"), SOURCE).unwrap();
        let text = SOURCE.lines().nth(line - 1).unwrap();
        let diff = format!("diff --git a/m.py b/m.py\n--- a/m.py\n+++ b/m.py\n@@ -{line},0 +{line},1 @@\n+{text}\n");
        review(&diff, &offline(tmp.path())).unwrap().findings.iter().map(|f| f.rule).collect()
    }

    #[test]
    fn a_pre_existing_if_in_a_touched_method_is_not_flagged() {
        assert!(rules_when_adding(5).is_empty());
    }

    #[test]
    fn an_if_the_diff_added_is_flagged() {
        assert_eq!(rules_when_adding(3), [3]);
    }

    #[test]
    fn an_unparsed_language_falls_back_to_the_hunk_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.kt"), "fun f() {\n  println(1)\n}\n").unwrap();
        let diff = "diff --git a/a.kt b/a.kt\n--- a/a.kt\n+++ b/a.kt\n@@ -2,0 +2,1 @@\n+  println(1)\n";
        let result = review(diff, &offline(tmp.path())).unwrap();
        assert_eq!(result.units, 1);
        assert!(result.notes.iter().any(|n| n.contains("no parser")));
    }
}
