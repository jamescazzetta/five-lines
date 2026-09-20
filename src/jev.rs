//! The judgment half: typed yes/no questions to Jev (typesafe.ai), one request per method.
//!
//! Jev answers a Noul with a number from 0 to 1. Questions in one request are evaluated in
//! parallel and in isolation, so every rule is its own atomic question and the answers are
//! combined in code — never one "review this method" prompt.
//!
//! Only questions whose structural precondition holds in the ADDED lines are sent: a method
//! whose diff adds no `else` is never asked about if-else.

use crate::units::Unit;
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MAX_SOURCE_CHARS: usize = 6000;

pub fn model() -> String {
    std::env::var("FIVE_LINES_MODEL").unwrap_or_else(|_| "jev-latest".to_string())
}

pub fn api_key() -> Option<String> {
    std::env::var("TYPESAFE_API_KEY").ok().filter(|k| !k.trim().is_empty())
}

fn noul(instructions: &str, yes: &str, no: &str) -> Value {
    json!({"type": "noul", "instructions": instructions, "criteria": {"true": yes, "false": no}})
}

pub const QUESTION_KEYS: [&str; 7] = ["r2", "r3", "r4", "r5", "r7", "r9", "idiom"];

pub fn question(key: &str) -> Value {
    match key {
        "r2" => noul(
            "Does `method` mix two levels of abstraction: it orchestrates calls on collaborator objects AND ALSO does inline low-level work on raw values (arithmetic, string building, index manipulation)?",
            "Both are present in the same body: calls such as this.x.y() / self.x.y() alongside inline arithmetic or string assembly",
            "The method only orchestrates calls, or only computes on the data it was given",
        ),
        "r3" => noul(
            "Does `method` contain an `if` that is not the very first statement of the method, or is there code after an if/else block at the same nesting level?",
            "An if is preceded by other statements, sits inside a loop after other work, or is followed by more statements",
            "There is no if, or the if is the first statement and nothing follows its block",
        ),
        "r4" => noul(
            "Does `method` contain an if/else (or else-if chain) in which BOTH branches are the project's own domain logic?",
            "Both branches implement business behaviour that could be two implementations of one interface",
            "There is no else; or the branch tests a foreign or library type, null/None, an error value, or a primitive the project does not control; or it is a guard clause that returns early",
        ),
        "r5" => noul(
            "Does `method` contain a switch / match / when / case that has a catch-all arm (default, else, _) or an arm that falls through, breaks, or otherwise does not return?",
            "A catch-all arm exists, or at least one arm does not return",
            "There is no switch-like construct; or it is exhaustive with no catch-all and every arm returns",
        ),
        "r7" => noul(
            "Does the boolean condition of any if or while in `method` have a side effect?",
            "The condition itself assigns a variable, performs I/O, logs, mutates state, advances an iterator or stream, or calls something that writes or throws",
            "Conditions only read values and call pure queries",
        ),
        "r9" => noul(
            "Is `method` a getter or setter that exposes an object's internal field so callers can branch on it or mutate it?",
            "It returns or assigns a private field of an object that has behaviour, inviting logic to live in the caller",
            "It is not an accessor; or it belongs to a plain data carrier (DTO, record, struct, dataclass, schema type, ORM-mapped entity) that has no behaviour to push data into",
        ),
        "affix" => noul(
            "Do the identifiers in `names` describe parts of ONE concept that should be its own type?",
            "They share a prefix or suffix because they belong together, e.g. a range, a coordinate, an address, a money amount",
            "They merely share a common word (id, name, count, list) and are otherwise unrelated",
        ),
        _ => noul(
            "Is the shape of `method` dictated by the language or a framework rather than chosen by the author?",
            "A generated or promoted constructor of a data carrier; a framework entry point (controller action, resolver, handler, lifecycle hook, main); a test fixture hook (setUp, beforeEach, @BeforeAll); an ORM or serialization mapping",
            "Ordinary application or domain code whose structure the author is free to change",
        ),
    }
}

static PRECONDITIONS: LazyLock<BTreeMap<&'static str, Regex>> = LazyLock::new(|| {
    BTreeMap::from([
        ("r3", Regex::new(r"\bif\b").unwrap()),
        ("r4", Regex::new(r"\b(else|elif|elsif)\b").unwrap()),
        ("r5", Regex::new(r"\b(switch|match|when|case)\b").unwrap()),
        ("r7", Regex::new(r"\b(if|while|elif)\b").unwrap()),
        (
            "r9",
            Regex::new(r"(?m)\b(get|set|is|has)[A-Z_]\w*\s*\(|@property|\bget\s*[;{]|\bset\s*[;{(]|=>\s*this\.|return\s+(this|self)[.>-]+\w+\s*;?\s*$")
                .unwrap(),
        ),
    ])
});

/// The subset worth asking: drop rules already decided mechanically and rules whose construct the diff did not add.
pub fn questions_for(unit: &Unit, skip: &[&str]) -> Map<String, Value> {
    let added = unit.added_text();
    let picked: Map<String, Value> = QUESTION_KEYS
        .iter()
        .filter(|key| !skip.contains(*key) && PRECONDITIONS.get(*key).is_none_or(|re| re.is_match(&added)))
        .map(|key| (key.to_string(), question(key)))
        .collect();
    // Nothing to exempt if nothing else is being asked.
    if picked.keys().all(|k| k == "idiom") { Map::new() } else { picked }
}

pub fn ask(state: Value, questions: &Map<String, Value>, key: &str) -> Result<Value> {
    let payload = json!({"model": model(), "state": state, "questions": questions});
    for attempt in 0..6u32 {
        let response = ureq::post(ENDPOINT)
            .timeout(Duration::from_secs(60))
            .set("Authorization", &format!("Bearer {key}"))
            .set("User-Agent", concat!("five-lines/", env!("CARGO_PKG_VERSION")))
            .send_json(&payload);
        match response {
            Ok(ok) => return Ok(ok.into_json()?),
            Err(ureq::Error::Status(code, _)) if code == 429 || code == 529 => {
                std::thread::sleep(Duration::from_secs(1 << attempt));
            }
            Err(ureq::Error::Status(code, body)) => {
                let text: String = body.into_string().unwrap_or_default().chars().take(300).collect();
                bail!("Jev returned HTTP {code}: {text}");
            }
            Err(other) => bail!("could not reach Jev: {other}"),
        }
    }
    bail!("Jev is still rate limiting after 6 attempts")
}

pub fn nouls(response: &Value) -> Result<BTreeMap<String, f64>> {
    let answers = response["answers"].as_object().ok_or_else(|| anyhow!("Jev response has no answers"))?;
    Ok(answers.iter().filter_map(|(k, a)| a["noul"].as_f64().map(|p| (k.clone(), p))).collect())
}

pub fn judge_unit(unit: &Unit, skip: &[&str], key: &str) -> Result<BTreeMap<String, f64>> {
    let questions = questions_for(unit, skip);
    if questions.is_empty() {
        return Ok(BTreeMap::new());
    }
    let source: String = unit.source.chars().take(MAX_SOURCE_CHARS).collect();
    nouls(&ask(json!({"language": unit.language, "name": unit.name, "method": source}), &questions, key)?)
}

pub fn judge_affixes(names: &[String], key: &str) -> Result<f64> {
    let questions = Map::from_iter([("affix".to_string(), question("affix"))]);
    let answers = nouls(&ask(json!({"names": names}), &questions, key)?)?;
    answers.get("affix").copied().ok_or_else(|| anyhow!("Jev did not answer the affix question"))
}
