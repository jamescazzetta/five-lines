# five-lines-jev

Review a pull-request diff against the ten refactoring rules of Christian Clausen's
*Five Lines of Code* (Manning).

The rules are meant to be mechanical: apply a rule, don't argue about a smell. In
practice about half of them can be **counted** and the other half need a small
**judgment**. This tool treats the two halves differently:

| | How it is decided | Reproducible | Needs an API key |
|---|---|---|---|
| Rules 1, 6, 8, 10 · rules 3 and 5 in Python | Parsing and counting | Yes | No |
| Rules 2, 4, 7, 9 · rules 3 and 5 in other languages · "is this a framework idiom?" | A typed yes/no question to [Jev](https://typesafe.ai) | No | Yes |

Jev is a model that returns a probability for a structured question instead of text.
That fits this problem: each judgment becomes one atomic question ("does the condition
of any `if` in this method have a side effect?"), the answers are combined in code,
and the probability decides what gets raised.

**It is a structural-quality lens, not a correctness review.** A method can break
every rule here and be correct, and a 3-line method can still have a bug. Run this next
to a real review, never instead of one.

## Quick start

No dependencies beyond Python 3.10+.

```sh
git clone <this repo> && cd five-lines-jev
python3 -m pip install -e .          # or: PYTHONPATH=src python3 -m five_lines.cli ...

# the bundled example, mechanical rules only (no network)
five-lines review examples/change.diff --repo examples/repo --no-jev

# with Jev judging the rest
export TYPESAFE_API_KEY=...          # never commit this
five-lines review examples/change.diff --repo examples/repo
```

On a real pull request, run it from a checkout of the PR branch:

```sh
gh pr checkout 123
gh pr diff 123 | five-lines review - --repo . --base origin/main
# or let it compute the diff itself
five-lines review --repo . --base origin/main
```

- `--repo` lets the tool read whole methods, not only the hunk, and look up base classes
  and interface implementers. Without it, the hunk text is reviewed on its own.
- `--base` tells "introduced" from "grown" and skips a long method the diff did not make longer.
- `--format json` for tooling, `--fail-on-findings` for CI, `--threshold` to change when
  a Jev judgment is raised (default 0.80; 0.55 up to the threshold is "worth a look").

Expected output for the bundled example is in [`examples/expected-review.md`](examples/expected-review.md).

## What it flags, and what it leaves alone

The rule text and its exemptions live in [`docs/five-lines-review.SKILL.md`](docs/five-lines-review.SKILL.md).
The tool follows them:

- **Only what the diff adds.** An `if` that was already in a method the diff touched is
  pre-existing and out of scope. A long method is flagged only if the diff introduced
  it or made it longer.
- **Framework and language idioms are suppressed**, not deleted: test fixture hooks,
  controller actions, promoted constructors, ORM mappings. Jev is asked whether the
  method's shape was dictated to the author; `--show-suppressed` reveals what was hidden.
- **Plain data carriers are not accessor violations** (rule 9), exhaustive matches where
  every arm returns are rule 5's approved shape, and test doubles do not count as a
  second implementation (rule 8).
- Exceptions, `Protocol`, `ABC`, `Enum`, `BaseModel` and similar bases are not
  inheritance findings (rule 6). A base class that is not found in the repo is reported
  as unconfirmed, because it is probably a library type.

## How the ten rules are checked

| # | Rule | Check |
|---|---|---|
| 1 | Five lines | Statement count, not line count. Python via `ast`; brace languages by counting body lines, with a wrapped call counted once. Compared against the base version when `--base` is given. |
| 2 | Call or pass, not both | Jev |
| 3 | If only at the start | Python: exact via `ast`. Other languages: Jev |
| 4 | Never if-else | Structure finds the `else` the diff added; Jev decides whether both branches are your own domain logic or a branch on a foreign type |
| 5 | Never switch | Python `match`: exact (catch-all arm, or an arm that does not return). Other languages: Jev |
| 6 | Inherit only from interfaces | Finds added `extends` / Python bases, then looks the base up in the repo: bodiless contract or implementation? |
| 7 | Pure conditions | Jev |
| 8 | No single-implementation interfaces | Finds added interfaces, protocols and traits, then counts implementers in the repo, excluding test paths |
| 9 | Avoid getters/setters | Jev |
| 10 | No common affixes | Known antonym pairs (`start`/`end`, `min`/`max`, `from`/`to`, ...) are flagged mechanically; any other shared affix is a candidate that Jev confirms |

One Jev request is made per changed method, carrying only the questions whose structural
precondition is present in the added lines. A method with no `else` is never asked about
if-else.

## How much to trust the Jev half

Measured, not assumed. `five-lines eval` scores Jev against the labelled snippets in
[`eval/cases.json`](eval/cases.json). In a code review the costly error is the **false
alarm**: a reviewer who is told twice that a plain DTO "smuggles behaviour through
accessors" stops reading the tool. So the number to watch is how many *raised* findings
are wrong.

First run, 2026-09-21, `jev-1.13.0` ([rows](eval/results-2026-09-21.json)):

| | |
|---|---|
| Judgments | 34 across 23 snippets in Python, TypeScript, JavaScript, Java, Go, PHP, Rust and C |
| Correct at 0.5 | 33 (97%) |
| Raised at ≥ 0.80 | 10 of the 12 true cases, with **0 false alarms**; no compliant snippet scored above 0.24 |
| True case scored as "worth a look" | an if/else between two pieces of domain logic, at 0.73 |
| The one miss | a PHP promoted constructor was not recognised as a language idiom (0.46) |

Read that with its limits:

- **The labels were written by the author of this tool, while building it.** It is a
  seed set, not an independent benchmark, and 34 judgments is a small number.
- **Jev is not deterministic.** In a separate test, identical requests moved confidence
  by up to 0.13 and flipped answers that were near 0.5. Findings close to the threshold
  can appear in one run and not the next. The mechanical findings never do.
- No comparison against a general LLM was run on these cases.
- Method source from your diff is sent to typesafe.ai when Jev is enabled. Use
  `--no-jev` for code you may not send to a third party.

## Limits of the mechanical half

- Python is parsed exactly. Brace languages (TypeScript, JavaScript, Java, Kotlin, C#,
  Go, PHP, Rust, Swift, C, C++, Scala, Dart) use a header pattern and brace matching.
  That finds the method around a changed line well; it is not a parser, and unusual
  formatting can defeat it.
- Rule 6 and rule 8 search the repo by name. Two types with the same name in different
  packages will confuse them.
- Rule 3 is strict because the book is strict. On real code it is usually the most
  frequent finding.

## Development

```sh
python3 -m unittest discover -s tests     # 21 tests, no network
five-lines eval --out eval/results-$(date +%F).json
```

Layout: `src/five_lines/` has `diff.py` (parse the diff), `units.py` (find changed
methods), `rules.py` (mechanical rules), `jev.py` (questions and client), `review.py`
(orchestration), `report.py` and `evaluate.py`.

## Credits

The ten rules are from Christian Clausen, *Five Lines of Code: How and when to
refactor* (Manning, 2021). The review workflow and exemptions follow the
`five-lines-review` skill in `docs/`. Jev is a product of TypeSafe AI; this project is
not affiliated with them or with the book.
