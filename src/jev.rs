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
const MAX_BATCH_CHARS: usize = 40_000;

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

/// One agent for the whole run, so requests reuse a single TLS connection.
static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build());

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
pub fn questions_for(unit: &Unit, skip: &[&str]) -> Vec<String> {
    let added = unit.added_text();
    let picked: Vec<String> = QUESTION_KEYS
        .iter()
        .filter(|key| !skip.contains(*key) && PRECONDITIONS.get(*key).is_none_or(|re| re.is_match(&added)))
        .map(|key| key.to_string())
        .collect();
    // Nothing to exempt if nothing else is being asked.
    if picked.iter().all(|k| k == "idiom") { Vec::new() } else { picked }
}

pub fn ask(state: Value, questions: &Map<String, Value>, key: &str) -> Result<Value> {
    let payload = json!({"model": model(), "state": state, "questions": questions});
    for attempt in 0..6u32 {
        let response = AGENT
            .post(ENDPOINT)
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

/// One method (or hunk) to be judged: its source, the rule questions to ask about it, and any
/// groups of names that might be one concept (rule 10).
pub struct Item {
    pub language: String,
    pub name: String,
    pub source: String,
    pub questions: Vec<String>,
    pub affixes: Vec<Vec<String>>,
}

impl Item {
    fn is_empty(&self) -> bool {
        self.questions.is_empty() && self.affixes.is_empty()
    }
}

/// Answers per item, in the order given, plus the number of requests made.
///
/// Jev has no batch endpoint; a batch is ONE request whose state holds several methods under
/// the keys `m1`, `m2`, ... and whose questions each point at their own method. That saves a
/// round trip per method. Every question can still see the other methods in the state, so the
/// batch size is a trade between latency and isolation: `five-lines eval --batch N` measures it.
pub fn judge(items: &[&Item], batch: usize, key: &str) -> Result<(Vec<BTreeMap<String, f64>>, usize)> {
    let mut answers: Vec<BTreeMap<String, f64>> = items.iter().map(|_| BTreeMap::new()).collect();
    let mut requests = 0;
    for chunk in chunks(items, batch.max(1)) {
        let mut state = Map::new();
        let mut questions = Map::new();
        for (slot, &index) in chunk.iter().enumerate() {
            let (item, m) = (items[index], format!("m{}", slot + 1));
            let source: String = item.source.chars().take(MAX_SOURCE_CHARS).collect();
            state.insert(m.clone(), json!({"language": item.language, "name": item.name, "method": source}));
            for q in &item.questions {
                questions.insert(format!("{m}_{q}"), pointed_at(question(q), "`method`", &format!("`{m}.method`")));
            }
            for (n, names) in item.affixes.iter().enumerate() {
                state.insert(format!("{m}_names{n}"), json!(names));
                questions.insert(format!("{m}_affix{n}"), pointed_at(question("affix"), "`names`", &format!("`{m}_names{n}`")));
            }
        }
        requests += 1;
        for (name, p) in nouls(&ask(Value::Object(state), &questions, key)?)? {
            // "m3_r7" -> slot 3, question "r7"
            let Some((m, q)) = name.split_once('_') else { continue };
            let Some(&index) = m[1..].parse::<usize>().ok().and_then(|slot| chunk.get(slot.wrapping_sub(1))) else { continue };
            answers[index].insert(q.to_string(), p);
        }
    }
    Ok((answers, requests))
}

/// Indices of the items that have something to ask, grouped by count and by a source-size budget.
fn chunks(items: &[&Item], batch: usize) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut size = 0;
    for (index, item) in items.iter().enumerate().filter(|(_, item)| !item.is_empty()) {
        let chars = item.source.len().min(MAX_SOURCE_CHARS);
        let fits = groups.last().is_some_and(|g| g.len() < batch && size + chars <= MAX_BATCH_CHARS);
        if !fits {
            groups.push(Vec::new());
            size = 0;
        }
        groups.last_mut().expect("just pushed").push(index);
        size += chars;
    }
    groups
}

fn pointed_at(mut question: Value, from: &str, to: &str) -> Value {
    if let Some(text) = question["instructions"].as_str().map(|t| t.replace(from, to)) {
        question["instructions"] = Value::String(text);
    }
    question
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source_chars: usize, questions: &[&str]) -> Item {
        Item {
            language: "python".into(),
            name: "f".into(),
            source: "x".repeat(source_chars),
            questions: questions.iter().map(|q| q.to_string()).collect(),
            affixes: Vec::new(),
        }
    }

    #[test]
    fn items_with_nothing_to_ask_cost_no_request() {
        let items = [item(10, &["r2"]), item(10, &[]), item(10, &["r7"])];
        assert_eq!(chunks(&items.iter().collect::<Vec<_>>(), 8), [vec![0, 2]]);
    }

    #[test]
    fn a_batch_is_bounded_by_count_and_by_source_size() {
        let small: Vec<Item> = (0..5).map(|_| item(10, &["r2"])).collect();
        assert_eq!(chunks(&small.iter().collect::<Vec<_>>(), 2), [vec![0, 1], vec![2, 3], vec![4]]);
        let large: Vec<Item> = (0..3).map(|_| item(MAX_SOURCE_CHARS, &["r2"])).collect();
        let grouped = chunks(&large.iter().collect::<Vec<_>>(), 100);
        assert!(grouped.iter().all(|g| g.len() * MAX_SOURCE_CHARS <= MAX_BATCH_CHARS), "{grouped:?}");
    }

    #[test]
    fn a_batched_question_points_at_its_own_method() {
        let q = pointed_at(question("r7"), "`method`", "`m3.method`");
        let text = q["instructions"].as_str().unwrap();
        assert!(text.contains("`m3.method`") && !text.contains("`method`"));
    }
}
