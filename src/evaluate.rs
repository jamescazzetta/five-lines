//! Score Jev's judgments against labelled snippets.
//!
//! In a code review the costly error is the FALSE ALARM: a reviewer who is told three times
//! that a plain DTO "smuggles behaviour through accessors" stops reading the tool. A missed
//! violation costs little. So the number to watch is how many raised findings (probability
//! >= 0.80) are wrong, not overall accuracy.

use crate::jev;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const RAISE: f64 = 0.80;
const BUNDLED_CASES: &str = include_str!("../eval/cases.json");

#[derive(Deserialize)]
struct CaseFile {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    language: String,
    name: String,
    source: String,
    expect: BTreeMap<String, bool>,
}

#[derive(Serialize)]
struct Row {
    case: String,
    language: String,
    question: String,
    expected: bool,
    p: f64,
    correct: bool,
}

pub fn run(cases: Option<&Path>, out: Option<&Path>, batch: usize) -> Result<()> {
    let key = jev::api_key().ok_or_else(|| anyhow!("TYPESAFE_API_KEY is not set"))?;
    let text = match cases {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        None => BUNDLED_CASES.to_string(),
    };
    let file: CaseFile = serde_json::from_str(&text).context("parsing the cases file")?;

    // The same path a review takes, so a batch size measured here is the batch size you get there.
    let items: Vec<jev::Item> = file
        .cases
        .iter()
        .map(|case| jev::Item {
            language: case.language.clone(),
            name: case.name.clone(),
            source: case.source.clone(),
            questions: case.expect.keys().cloned().collect(),
            affixes: Vec::new(),
        })
        .collect();
    let started = std::time::Instant::now();
    let (answers, requests) = jev::judge(&items.iter().collect::<Vec<_>>(), batch, &key)?;
    let elapsed = started.elapsed();

    let mut rows = Vec::new();
    for (case, answered) in file.cases.iter().zip(&answers) {
        for (question, &expected) in &case.expect {
            let p = *answered.get(question).ok_or_else(|| anyhow!("no answer for {question} in {}", case.id))?;
            let correct = (p >= 0.5) == expected;
            println!("{} {question:<6} expected={expected:<5} p={p:.2}  {}", if correct { ' ' } else { 'x' }, case.id);
            rows.push(Row { case: case.id.clone(), language: case.language.clone(), question: question.clone(), expected, p, correct });
        }
    }
    println!("\nbatch size {batch}: {requests} request(s) for {} snippets in {:.1}s", file.cases.len(), elapsed.as_secs_f64());

    println!("\n{:<7}{:>4}{:>10}{:>8}{:>14}{:>8}", "rule", "n", "accuracy", "raised", "false alarms", "missed");
    for question in rows.iter().map(|r| r.question.as_str()).collect::<BTreeSet<_>>() {
        let group: Vec<&Row> = rows.iter().filter(|r| r.question == question).collect();
        let accuracy = 100.0 * group.iter().filter(|r| r.correct).count() as f64 / group.len() as f64;
        println!(
            "{question:<7}{:>4}{:>9.0}%{:>8}{:>14}{:>8}",
            group.len(),
            accuracy,
            group.iter().filter(|r| r.p >= RAISE).count(),
            group.iter().filter(|r| r.p >= RAISE && !r.expected).count(),
            group.iter().filter(|r| r.expected && r.p < RAISE).count(),
        );
    }
    let raised: Vec<&Row> = rows.iter().filter(|r| r.p >= RAISE).collect();
    println!(
        "\n{} judgments, {:.0}% correct at 0.5; {} raised at >= {RAISE}, of which {} are false alarms",
        rows.len(),
        100.0 * rows.iter().filter(|r| r.correct).count() as f64 / rows.len() as f64,
        raised.len(),
        raised.iter().filter(|r| !r.expected).count(),
    );
    if let Some(path) = out {
        let body = serde_json::to_string_pretty(&json!({"model": jev::model(), "batch": batch, "requests": requests, "rows": rows}))?;
        std::fs::write(path, body + "\n")?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
