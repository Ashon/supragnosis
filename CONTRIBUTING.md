# Contributing to supragnosis

Thanks for looking. This document covers how to build and test, what a reviewable change
looks like here, and the one constraint that makes this project different from most.

## The one constraint

[`docs/principles.md`](docs/principles.md) is a normative document, not a design essay. Every
change is justified against it, and a convenient decision that conflicts with a principle is
not accepted without amending the document first.

This sounds heavier than it is. In practice it means one question during review: *which
clause does this change touch, and does it still hold?* Most changes touch none. When one
does, [Appendix B](docs/principles.md) is the checklist to run, and the answer belongs in the
PR description rather than in a reviewer's memory.

If your change makes a principle unenforceable, that is a legitimate outcome. Say so, and
amend the principle in the same PR. What is not accepted is a change that quietly makes a
declared clause false.

## Build and test

Requires a recent stable Rust toolchain. No C or C++ toolchain is needed.

```bash
cargo build            # default: keyword search, lightweight
cargo test             # unit and integration tests
cargo clippy --all-targets
cargo fmt --all -- --check
```

[`Taskfile.yml`](Taskfile.yml) wraps the common loops (`brew install go-task`, then `task` to
list everything). `task check` runs exactly what CI runs, so a green run locally is a green
run there.

```bash
task dev             # viewer on your build, isolated store, socket, and port
task dev:snapshot    # same, but against a copy of the live daemon's knowledge
task check           # clippy + viewer ESLint + tests
```

The desktop shell needs platform webview toolchains, so it is excluded from the default
members. Build it explicitly with `cargo build -p supragnosis-app`.

`e2e/` is a real-model measurement suite, not a regression guard. Its tests are `#[ignore]`d
by default and need a local Ollama or an API key.

## Formatting

`rustfmt.toml` is tuned to the density the code was already written at, not to the defaults.
Run `cargo fmt --all` before committing. `.git-blame-ignore-revs` points at the formatting
pass so `git blame` reads through it:

```bash
git config blame.ignoreRevsFile .git-blame-ignore-revs
```

## Tests

Three kinds live here, and picking the right one matters.

**Scenario tests** (`principle_scenarios.rs`) answer *does the feature work*. Do X, expect Y.

**Policy cases** (`policy_cases.rs`) answer *did the rule hold across an act*. Most principles
here are statements about a difference rather than a state: nothing may be forgotten, a
generator may not commit, a proposal may not move the canon before its verdict. A final state
looks identical whether the rule held or was never exercised, so these snapshot the store
before and after and assert a named clause about the delta.

If a principle cannot be phrased as a constraint on change, it probably wants a scenario test,
and noticing which one it is usually clarifies what the principle actually claims.

**Coverage registry** (`principle_coverage.rs`) is the declaration that each clause is checked,
and it is itself a test. Adding a principle without declaring how it is checked fails the
completeness test. Renaming or deleting a guard reports its clause as unguarded.

## Sending a change

- One concern per PR. A refactor and a behavior change in one diff cannot be reviewed.
- Commit messages describe what changed and why, in the imperative. Look at `git log` for the
  house style: the subject line states the decision, not the file list.
- Please do not add AI-assistant trailers (`Co-Authored-By: Claude`, `Generated with ...`) to
  commits. Tooling is not authorship.
- New behavior needs a test. New *policy* needs a policy case, not just a scenario.
- If your change adds a field to a model type, the compiler will point you at the functions
  that enumerate fields (identity hashing, attestation ordering, merge, signing bytes). Those
  errors are deliberate. Decide explicitly whether the new field is content identity, an
  attestation-distinguishing axis, a merge target, and whether it is inside the signed bytes.

## Good first issues

Issues labelled `good first issue` are scoped so that reading one document is enough context.
If none are open and you want one, open an issue saying what area interests you.

Areas that are approachable without absorbing the whole design:

- Store adapters. The port conformance suite defines the contract, so a new adapter is a
  well-specified target.
- The credential detector in `core`. Patterns are deliberately narrow; adding a
  self-identifying shape is a small, testable change.
- CLI ergonomics and error messages. Failures are meant to be written for an agent to
  self-correct from.
- Documentation. Especially: places where a doc claims something the code no longer does.

## Reporting a security issue

Do not open a public issue. See [SECURITY.md](SECURITY.md).

## License

By contributing you agree that your work is dual licensed under MIT OR Apache-2.0, matching
the project, with no additional terms.
