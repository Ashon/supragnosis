## What this changes

<!-- One paragraph. The decision, not the file list. -->

## Why

<!-- What was wrong, or what could not be done before. -->

## Principles

<!--
Which clauses of docs/principles.md does this touch, and do they still hold?
"None" is the common and correct answer. If a clause moves state (Deferred -> Scenario,
or the reverse), say so and update the coverage registry in the same PR.
-->

None.

## Checks

- [ ] `task check` passes (clippy, tests, viewer lint)
- [ ] `cargo fmt --all -- --check` passes
- [ ] New behavior has a test; new policy has a policy case
- [ ] Docs updated where they now claim something the code no longer does
- [ ] No AI-assistant trailers in the commit messages
