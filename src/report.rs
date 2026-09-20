//! Render a review as Markdown (for a PR comment) or JSON (for tooling).

use crate::review::Review;
use crate::rules::{Finding, rule_name};
use serde_json::json;
use std::collections::BTreeSet;
use std::fmt::Write;

fn tag(finding: &Finding) -> String {
    match (finding.basis, finding.confidence) {
        ("jev", Some(p)) => format!("Jev {p:.2}"),
        ("candidate", _) => "unconfirmed".to_string(),
        _ => "mechanical".to_string(),
    }
}

pub fn markdown(review: &Review, show_suppressed: bool) -> String {
    let shown: Vec<&Finding> = review.findings.iter().filter(|f| show_suppressed || f.suppressed.is_none()).collect();
    let hidden = review.findings.len() - shown.len();
    let raised = shown.iter().filter(|f| f.raised()).count();
    let effort = if review.jev_used { format!("{} Jev request(s)", review.jev_requests) } else { "mechanical rules only".to_string() };

    let mut out = String::from("## Five Lines review\n\n");
    out += "_A structural-quality lens (Christian Clausen, *Five Lines of Code*). It is not a correctness review: \
            a method can break every rule below and be correct, and a 3-line method can still have a bug._\n\n";
    let _ = writeln!(out, "{} changed method(s) reviewed · {raised} finding(s) · {effort}", review.units);
    if !review.notes.is_empty() {
        out += "\n";
        for note in &review.notes {
            let _ = writeln!(out, "> {note}");
        }
    }
    if shown.is_empty() {
        out += "\nNo rule violations introduced by this diff.\n";
    }
    for rule in shown.iter().map(|f| f.rule).collect::<BTreeSet<_>>() {
        let _ = writeln!(out, "\n### Rule {rule} · {}", rule_name(rule));
        for f in shown.iter().filter(|f| f.rule == rule) {
            let status = f.suppressed.as_ref().map(|why| format!("suppressed: {why}")).unwrap_or_else(|| f.severity.to_string());
            let _ = writeln!(out, "- `{}:{}` — {} _({}; {status})_\n  - Fix: {}", f.path, f.line, f.message, tag(f), f.fix);
        }
    }
    if hidden > 0 {
        let _ = writeln!(out, "\n_{hidden} finding(s) suppressed as framework or language idioms; rerun with `--show-suppressed`._");
    }
    out
}

pub fn as_json(review: &Review) -> String {
    let value = json!({
        "units": review.units,
        "jev_used": review.jev_used,
        "jev_requests": review.jev_requests,
        "notes": review.notes,
        "findings": review.findings,
    });
    serde_json::to_string_pretty(&value).expect("findings serialise") + "\n"
}
