# ADR-0051: The local check tests only the crates a change can reach

- **Status:** Accepted
- **Date:** 2026-11-26
- **Related decisions:** ADR-0050 (a machine-wide queue for test runs)
- **Specification:** [Test runs on a shared machine](../dev/test-runs.md)

## Context

`mise run check` runs every test in the workspace, about 4400, whatever changed. Many changes touch one crate near the top of the graph, such as `loom-app`, or only docs and scripts. A change to `loom-app` cannot change the result of a `loom-dsp` test, and a change to a Markdown file outside `docs/modules` cannot change any. With several agents on one machine, every test run that is not needed makes the queue (ADR-0050) longer.

Constraints: CI stays the gate and runs every test, a change must never pass the local check by having a test it can break skipped, and the output must say what was left out.

## Options considered

1. **Run every test.** Always right. The waste stays.
2. **Crates changed since the merge base, and their reverse dependents.** Uses `cargo metadata`, which knows every dependency edge. Files outside `crates/` that tests read (`examples/`, `docs/modules/`) and files that change every build (`Cargo.lock`, `mise.toml`, `.config/`) run everything. Any file it does not know runs everything.
3. **Finer selection, by test or by source file.** Needs coverage data or a call graph kept up to date. Much more code for a smaller gain.

## Decision

Option 2, in `mise run check` only. Files that select nothing are listed by name; an unknown file runs every test. Build with `--workspace` as CI does and pick the tests with a nextest filter, so features resolve the same for every selection. `LOOM_TEST_ALL=1` or `mise run check -- --all` runs every test.

## Consequences

- A change to one crate near the top of the graph, or to docs only, checks in a fraction of the time.
- A test that reads another crate's files by path, not through a dependency, is missed unless that folder is in the run-everything list. The specification says so, and today no test does it outside `examples/` and `docs/modules/`.
- A green local check means "the reachable tests pass". CI still runs them all before merge.
