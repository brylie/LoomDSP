# ADR-0049: Git hooks reuse a check that passed on the same tree

- **Status:** Accepted
- **Date:** 2026-11-26
- **Related decisions:** ADR-0048 (local development telemetry in files)
- **Specification:** [Reusing a check that passed on the same tree](../dev/check-reuse.md)

## Context

AGENTS.md asks agents to run `prek run --all-files` and `mise run check` before they ask for a review. Then `git commit` runs the quick checks again in its hook, and `git push` runs them a third time. Development telemetry (ADR-0048) found 817 commits or pushes in 30 days that came right after a check, with no change to the code in between. Those calls took 5.6 hours, though that figure includes the network and slow checks the push hook no longer runs. `prek run --all-files` is also the command agents most often repeat in a tight loop.

The quick checks (fmt, clippy, and a few scripts) depend only on the files, the tool versions, and a few settings. The devtel wrapper around each check already hashes the working tree. Running the same check again on the same inputs tells nothing new.

Constraints: hooks are never skipped with `--no-verify`, tools come through mise, CI stays the gate, and a check must never pass when it would have failed.

## Options considered

1. **Keep running everything.** Simple and always fresh. The waste stays.
2. **Stop asking agents to check by hand before a commit.** No code. But the agent finds a lint error only while committing, the push hook still repeats the commit hook, and loops of `prek run --all-files` stay.
3. **A pass record keyed on the inputs.** The wrapper records "check X passed on tree T with tools V and settings S" and a later run with the same key reuses it. It costs about 150 ms per check and some code, and a key that misses an input could reuse a pass that no longer holds.
4. **Let each tool cache its own result.** Cargo already skips unchanged crates, but clippy still starts cargo, checks fingerprints, and replays warnings. fmt and the scripts have no cache. This saves part of the time for one tool only.

## Decision

We use option 3, for the quick checks in the commit and push stages only, with the key, store, and output the specification describes. A pass is recorded only when the check exits 0 and the working tree after it is the same as before. Failures are never reused. Reuse can be turned off with `LOOM_CHECK_REUSE=0` and is never on in CI. `mise run check` runs the quick checks through prek, so one check by hand feeds the hooks, and AGENTS.md no longer asks for a separate `prek run --all-files`. The records live in the git common folder of the clone, apart from the devtel store, so reuse works with telemetry off.

## Consequences

- A commit or push right after a passing check takes a fraction of a second per quick check instead of seconds or minutes.
- The devtel wrapper no longer always runs its command. ADR-0048 says the wrapper passes the command's output and exit code through unchanged; that still holds when the check runs, and a reused pass exits 0 only where the same check exited 0 on the same inputs. The devtel specification changes with this.
- Each reusable hook shows its output (`verbose = true`), so a reused pass says so instead of reading as a fresh one.
- A key that misses an input can reuse a pass that no longer holds. CI runs every check from the start, so such a mistake cannot reach `main`, but it can surprise a contributor. The key errs towards including too much.
- Pass records are local files that any process could write. Writing one by hand counts as skipping a hook.
- Devtel records each reuse, so the saving is measured, not assumed.
