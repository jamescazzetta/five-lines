---
name: five-lines-review
description: Use when reviewing a diff/PR for internal code quality and refactorability — method length, mixed abstraction levels, nested conditionals, if-else chains, switch statements, inheritance, getters/setters, or repeated variable-name affixes. A structural-quality lens to run alongside a correctness/adversarial review, not a replacement for one.
---

# Five Lines Review

## Overview

Ten mechanical refactoring rules from Christian Clausen's *Five Lines of Code* (Manning). Each rule is a **grep-able structural signal**, not a judgment call — that's the book's whole point: apply rules, don't rely on "code smell" intuition.

The rules are about structure, so they are language-agnostic. The *signals* below are written generically with concrete forms in parentheses; read them in whatever language the diff is in.

**This is a quality lens, not a bug-finding lens.** It never replaces a correctness/adversarial review (logic errors, edge cases, security, missing tests) — run both, report them as separate sections. A method can violate every rule below and still be correct; a 3-line method can still have a bug.

## When NOT to flag

Most real production code violates several of these — that's normal, not a PR-blocking crisis. Only flag a violation when **the PR's own diff introduces or grows it**. Skip:
- Violations in unchanged code the PR merely calls or touches in passing.
- Idioms the host language or framework requires: a generated or promoted constructor on a data carrier, a framework entry point whose shape is dictated (resolver, controller action, handler, lifecycle hook), a test fixture hook (`setUp`, `beforeEach`, `@BeforeAll`), an ORM entity's mapped accessors, serialization annotations.
- A getter/setter pair that's a plain data-transfer property, not behavior smuggled through accessors.

State severity per finding: **introduced by this diff** (worth raising) vs. **pre-existing, out of scope** (mention once, don't belabor).

## The ten rules

| # | Rule | Grep-able signal |
|---|------|-------------------|
| 1 | **Five lines** — no method over ~5 non-blank lines; extract the rest | Count statements in the method body, not `wc -l` (one multi-line call is one statement) |
| 2 | **Call or pass, not both** — a method either orchestrates (calls other methods) or computes on data it's given — never mixes both abstraction levels | A method containing both a chain of calls on collaborators (`this.x.y()`, `self.x.y()`, `$this->x->y()`) *and* inline arithmetic or string-building on raw values |
| 3 | **If only at the start** — a conditional is the method's first statement, guard-clause style; nothing at the same nesting level follows its block | An `if` preceded by other statements, or code after the `if`/`else` block at the same indent |
| 4 | **Never if-else** — except branching on a foreign/library type you don't control; otherwise prefer polymorphism | An `if`/`else` where both branches are your own domain logic |
| 5 | **Never switch** — unless exhaustive (no catch-all arm) and every branch returns | A `switch` / `match` / `when` / `case` carrying a catch-all (`default`, `else`, `_`), or a branch that falls through or breaks instead of returning |
| 6 | **Inherit only from interfaces** — never inherit *implementation* | Subclassing something that has method bodies (`extends` a concrete or abstract class, inheriting a concrete base, embedding a concrete type). Implementing an interface / trait / protocol / ABC with no bodies is fine |
| 7 | **Pure conditions** — the boolean expression in an `if`/`while` has no side effects | Assignment, a call that logs/writes/throws, or I/O inside the condition itself |
| 8 | **No single-implementation interfaces** — an interface with exactly one real implementer is boilerplate, not abstraction | Grep the interface/trait/protocol name; count implementers, excluding test doubles |
| 9 | **Avoid getters/setters** — push behavior to the data instead of exposing internals for callers to read/mutate | A public accessor (explicit getter, exported field, or language-level property) immediately followed by external logic branching on the returned value |
| 10 | **No common affixes** — shared prefixes/suffixes across sibling variables/fields (`startDate`/`endDate`, `minPrice`/`maxPrice`) signal they belong in their own type | Two or more fields/params differing only by a prefix or suffix |

**Reading these in your language.** *Interface* covers interfaces, traits, protocols and pure abstract bases alike. *Getter/setter* covers explicit accessors and language-level properties (`@property`, `{ get; set; }`, computed properties). Rule 6 is about inheriting **implementation**, so it also catches embedding a concrete type or extending a base with bodies — conforming to a protocol or trait is not a violation. Where a language has no classes, rules 6, 8 and 9 mostly fall away; 1–5, 7 and 10 still apply.

## Workflow

1. Read the diff's changed methods only (rule scope is per-method, not per-file).
2. For each changed method, check it against the ten rules above.
3. Drop anything from **When NOT to flag**.
4. Report findings grouped by rule number, each with: file:line, the violation, and a one-line concrete fix (usually "extract `X` into its own method" or "replace with polymorphism on `Y`"). Don't just cite the rule — show the shape of the fix.
5. Keep this section clearly separate from any correctness/adversarial findings in the same review — different lens, different severity model.

## Common mistakes

- **Flagging every violation as equally severe.** A 6-line method one line over budget is not the same finding as a God-method mixing five abstraction levels. Say which.
- **Applying rule 9 to plain data carriers.** A DTO, record, struct, dataclass or schema type has no "behavior" to push data into — it's just a value holder. Not a violation.
- **Applying rule 4/5 to type-safety exhaustiveness checks.** An exhaustive match over an enum or union with no catch-all, where every arm returns, is rule 5's *approved* shape — that's compliance, not a finding.
- **Counting test doubles as implementers under rule 8.** A mock, stub or fake is not a second implementation; the interface still has one.
- **Using this as the whole review.** If the summary has no correctness section, the review isn't done.
