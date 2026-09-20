# five-lines

Use [Jev](https://typesafe.ai) to review a pull-request diff against the ten refactoring
rules of Christian Clausen's *Five Lines of Code* (Manning). One binary, no runtime to
install, for Windows, macOS and Linux. Ten languages are parsed exactly; any other is
reviewed through Jev.

Jev is a model that returns a probability for a structured question instead of text.
That fits code review well: each judgment becomes one atomic yes/no question ("does the
condition of any `if` in this method have a side effect?"), the answers are combined in
code, and the probability decides what gets raised and what is only "worth a look".

The rules are meant to be mechanical: apply a rule, don't argue about a smell. So the
tool only asks Jev what cannot be counted:

| | How it is decided | Reproducible | Needs an API key |
|---|---|---|---|
| Rules 2, 7, 9 · confirming 4 and 10 · "is this a framework idiom?" | A typed yes/no question to Jev | No | Yes |
| Rules 1, 3, 5, 6, 8, 10 | A real parser ([tree-sitter](https://tree-sitter.github.io)) and counting | Yes | No |

Without an API key, or with `--no-jev`, the mechanical half still runs on its own.

**It is a structural-quality lens, not a correctness review.** A method can break
every rule here and be correct, and a 3-line method can still have a bug. Run this next
to a real review, never instead of one.

## Install

Prebuilt binaries are attached to each [GitHub Release](../../releases):
Windows (x64, ARM64), macOS (Apple silicon, Intel), Linux (x64 static, ARM64).

```sh
# macOS and Linux
curl -fsSL https://raw.githubusercontent.com/jamescazzetta/five-lines/main/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/jamescazzetta/five-lines/main/install.ps1 | iex
```

Or download the archive for your platform from the Releases page, unpack it, and put
`five-lines` (`five-lines.exe` on Windows) somewhere on your `PATH`.

With a Rust toolchain: `cargo install --git https://github.com/jamescazzetta/five-lines`.

## Use

```sh
# the bundled example, mechanical rules only (no network)
five-lines review examples/change.diff --repo examples/repo --no-jev

# with Jev judging the rest
export TYPESAFE_API_KEY=...          # PowerShell: $env:TYPESAFE_API_KEY = "..."
five-lines review examples/change.diff --repo examples/repo
```

On a real pull request, run it from a checkout of the PR branch:

```sh
gh pr checkout 123
gh pr diff 123 | five-lines review - --repo . --base origin/main
# or let it compute the diff itself
five-lines review --repo . --base origin/main
```

- `--repo` lets the tool parse whole files, find the method around each changed line,
  and look up base classes and interface implementers. Without it, only the hunk text
  is available and only Jev can review it.
- `--base` tells "introduced" from "grown" and skips a long method the diff did not make
  longer. It needs `git` on the `PATH`.
- `--format json` for tooling, `--fail-on-findings` for CI, `--threshold` to change when
  a Jev judgment is raised (default 0.80; 0.55 up to the threshold is "worth a look").
- `five-lines languages` lists the parsers compiled into the binary.

Expected output for the bundled example is in [`examples/expected-review.md`](examples/expected-review.md).

## Languages

Parsed exactly: **Python, JavaScript, TypeScript (and TSX), Java, Go, PHP, Rust, C, C++, C#**.

A file in any other language (Kotlin, Swift, Ruby, ...) is reviewed from its hunk text
by Jev alone, and the report says so. Adding a language means adding its tree-sitter
grammar and, where its node names differ, a line in [`src/lang.rs`](src/lang.rs).

## What it flags, and what it leaves alone

The rule text and its exemptions live in the [`five-lines-review` skill](https://github.com/jamescazzetta/skills/blob/main/five-lines-review/SKILL.md).
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
| 1 | Five lines | Statement nodes in the method body, not lines: a call wrapped over six lines is one statement, a Rust tail expression counts, a `for` header does not. Compared against the base version when `--base` is given. |
| 2 | Call or pass, not both | Jev |
| 3 | If only at the start | Parsed: an `if` the diff added that is not the method's first and only statement. `else if` is not counted twice. |
| 4 | Never if-else | Parsed to find the if/else the diff added; Jev decides whether both branches are your own domain logic or a branch on a foreign type |
| 5 | Never switch | Parsed: a catch-all arm (`default`, `case _`, `_ =>`), or an arm that breaks or falls through instead of returning. Value arms (`=>`, `->`) return by construction. Grouped labels are not flagged. |
| 6 | Inherit only from interfaces | Finds added `extends` / Python bases, then looks the base up in the repo: bodiless contract or implementation? |
| 7 | Pure conditions | Jev |
| 8 | No single-implementation interfaces | Finds added interfaces, protocols and traits, then counts implementers in the repo, excluding test paths |
| 9 | Avoid getters/setters | Jev |
| 10 | No common affixes | Parsed parameter names. Known antonym pairs (`start`/`end`, `min`/`max`, `from`/`to`, ...) are flagged mechanically; any other shared affix is a candidate that Jev confirms |

Only questions whose structural precondition is present in the added lines are asked. Up
to eight changed methods travel in one Jev request (`--batch N` changes that): Jev has no
batch endpoint, so a batch is one request whose state holds the methods as `m1`, `m2`, ...
and whose questions each point at their own method.

## How much to trust the Jev half

Measured, not assumed. `five-lines eval` scores Jev against the labelled snippets bundled
in the binary ([`eval/cases.json`](eval/cases.json)). In a code review the costly error
is the **false alarm**: a reviewer who is told twice that a plain DTO "smuggles behaviour
through accessors" stops reading the tool. So the number to watch is how many *raised*
findings are wrong.

Two runs on 2026-09-21 against `jev-1.13.0`, 34 judgments across 23 snippets in Python,
TypeScript, JavaScript, Java, Go, PHP, Rust and C ([rows of the second run](eval/results-2026-09-21.json)):

| | Run 1 | Run 2 |
|---|---|---|
| Correct at 0.5 | 33 of 34 | 34 of 34 |
| Raised at ≥ 0.80 | 10 of the 12 true cases | 10 of the 12 true cases |
| False alarms among raised | **0** | **0** |
| Not raised | a domain if/else scored as "worth a look"; a PHP promoted constructor not recognised as an idiom (0.46) | the same two cases, both between 0.5 and 0.8 |

### What batching costs

Every question in a batch can see the other methods in the request, so batching trades
isolation for speed. `five-lines eval --batch N` measures it on the same 34 judgments:

| Methods per request | Requests | Time | Correct | False alarms | Highest score on compliant code |
|---|---|---|---|---|---|
| 1 | 23 | 7.6 s | 34 of 34 | 0 | 0.22 |
| 4 | 6 | 4.6 s* | 34 of 34 | 0 | 0.26 |
| **8 (default)** | 3 | 1.3 s | 34 of 34 | 0 | 0.38 |
| 23 (everything) | 1 | 1.0 s | 34 of 34 | 0 | 0.45 |

\* measured before requests shared one connection; the others after.

Accuracy held at every size. What moved is the margin: a finding is reported from 0.55, and
compliant code drifted from 0.22 toward it as the batch grew. Most probabilities shifted by
about 0.03; the fuzziest question ("is this a framework idiom?") shifted by up to 0.24.
Eight per request keeps most of the speed and most of the margin. One run per size on a
small seed set, so treat it as a direction, not a guarantee.

The two earlier runs differ because Jev is not deterministic, which is the first of the limits:

- **Jev is not deterministic.** In a separate test, identical requests moved confidence
  by up to 0.13 and flipped answers that were near 0.5. A finding close to the threshold
  can appear in one run and not the next. The mechanical findings never do.
- **The labels were written by the author of this tool, while building it.** It is a
  seed set, not an independent benchmark, and 34 judgments is a small number.
- No comparison against a general LLM was run on these cases.
- Method source from your diff is sent to typesafe.ai when Jev is enabled. Use
  `--no-jev` for code you may not send to a third party.

## Limits of the mechanical half

- Rule 6 and rule 8 search the repo by name. Two types with the same name in different
  packages will confuse them. The inheritance line itself is found by pattern, not by parsing.
- Statement counting is generic across grammars. It is tested in all ten languages, but
  an unusual construct can be over- or under-counted by one.
- Rule 3 is strict because the book is strict. On real code it is usually the most
  frequent finding.

## Development

```sh
cargo test                      # 29 tests, no network
cargo clippy --all-targets -- -D warnings && cargo fmt --check
cargo run -- eval --out eval/results-$(date +%F).json   # add --batch N to compare sizes
```

Layout: `src/diff.rs` (parse the diff), `lang.rs` (grammars and node kinds), `units.rs`
(find changed methods), `rules.rs` (mechanical rules), `jev.rs` (questions and client),
`review.rs` (orchestration), `report.rs`, `evaluate.rs`.

Releases: push a tag such as `v0.1.0`. [`release.yml`](.github/workflows/release.yml)
builds all six targets on GitHub's runners and publishes them with checksums.
[`ci.yml`](.github/workflows/ci.yml) runs the tests on Linux, macOS and Windows.

The first version of this tool was a Python script; it is in the git history at the
first commit.

## Credits

The ten rules are from Christian Clausen, *Five Lines of Code: How and when to
refactor* (Manning, 2021). The review workflow and exemptions follow the
[`five-lines-review` skill](https://github.com/jamescazzetta/skills/blob/main/five-lines-review/SKILL.md). Jev is a product of TypeSafe AI; this project is
not affiliated with them or with the book.

## License

MIT. See [LICENSE](LICENSE).
