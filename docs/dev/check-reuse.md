# Reusing a check that passed on the same tree

**Status: Agreed.** Decision: [ADR-0049](../adr/0049-git-hooks-reuse-a-pass-on-the-same-tree.md). Builds on [development telemetry](devtel.md) (ADR-0048). Code: [`scripts/devtel/reuse.py`](../../scripts/devtel/reuse.py), called from `run_step` in [`devtel.py`](../../scripts/devtel/devtel.py) through [`run.sh`](../../scripts/devtel/run.sh).

When a quick check has already passed on exactly the code a git hook is about to check, with the same tools and settings, the hook reuses that pass instead of running the check again. The goal is to stop agents paying for the same check two or three times: once by hand, once in the commit hook, once in the push hook.

CI never reuses a pass. It always runs every check from the start, so it stays the gate.

## Why

Devtel imported 30 days of Claude Code transcripts (2026-10-27 to 2026-11-26) and found 817 `git commit` or `git push` calls that came right after the session had run a check, with no change to the code in between ("hook after check" in `devtel_reruns`):

| Git call | Count | Median | 90th percentile | Total |
|---|---|---|---|---|
| commit | 465 | 3.5 s | 26.6 s | 1.5 h |
| push | 352 | 5.0 s | 167 s | 4.2 h |

These times include all of the call, not only the checks: the push numbers include the network, and for most of the period the push hook still ran the slow checks, which have since moved to the manual stage. So the real waste today is smaller than 5.6 hours, mostly clippy and fmt. The saving will be measured with devtel after the change (see [Telemetry](#telemetry)), not assumed.

`prek run --all-files` is also the command agents most often repeat in a tight loop. A loop on an unchanged tree would reuse its passes too.

## What is reused

Only the wrapped quick checks in the commit and push stages, which depend on nothing but the files, the tool versions, and a few settings:

| Check | Reused | Why |
|---|---|---|
| `cargo-fmt` | Yes | Output depends on the files, rustfmt's version, and `rustfmt.toml` (in the tree) |
| `cargo-clippy` | Yes | Depends on the files, `Cargo.lock`, the toolchain, and the flags in the environment (see [The key](#the-key)) |
| `adr-citations`, `test-placement` | Yes | Scripts that read only the tree |
| `typos`, `adr-status` | Only an exact match | prek passes them the changed files, in batches, so the commit hook's file list rarely matches an earlier run's (decision 3) |
| Builtin prek checks (whitespace, line endings, merge markers) | No | prek runs them itself, so there is no entry to wrap. They take milliseconds |
| Manual stage: coverage, doc tests, docs, wasm, cargo-deny | No | Not run by the commit or push hooks, so nothing to save. Tests also depend on the machine, the GPU, and timing |

A pass is reused, never a failure. A failed check always runs again, so its output is fresh.

## How it works

Every check in `prek.toml` already runs through `scripts/devtel/run.sh`, which hashes the working tree before the check. Reuse adds three steps around that:

1. **Before the check**, compute the [key](#the-key). If a pass record with that key exists and is fresh, print one line saying so and exit 0 without running the check.
2. **Run the check** as now, with its output and exit code unchanged.
3. **After the check**, if it exited 0, hash the working tree again. Write a pass record only if the tree is the same as before. This matters for `cargo fmt`, which formats in place and exits 0: prek then fails the hook because files changed, so that run must not count as a pass. It also covers a file edited while the check ran.

Anything that goes wrong while computing the key or writing the record (no git, no Python, an unreadable file) means the check runs as if reuse did not exist. Reuse fails open, towards running the check.

### What the hook sees

prek stashes unstaged changes before the commit hooks and restores them after, but leaves untracked files in place. So the tree a commit hook checks is the index plus the untracked files. Tested with prek 0.4.8:

| What you did before committing | Tree in the hook equals the tree of the hand run? |
|---|---|
| Ran the checks, then `git add -A && git commit` (or `git commit -a`) | Yes: the pass is reused |
| Ran the checks, then staged only some of the changes | No: prek stashed the rest, so the check runs |
| Ran the checks with an untracked file present that you do not commit | Yes: the hook sees that file on disk too, so its result would be the same |

`prek run` by hand also sets `PRE_COMMIT=1`, so run.sh cannot tell a hand run from a git hook. It does not need to: a hand run on an unchanged tree is a repeat too.

### The key

A SHA-256 hash of these lines, in this order:

- The record format version (`1`).
- The check's name and its full command line, including any file names prek passed.
- The working tree's hash before the check (the same `git add -A` into a temporary index, then `git write-tree`, that devtel already does).
- The tool versions in use: the output of `mise ls --current --json` (about 45 ms). `mise.toml` pins Rust, prek, and typos to `latest` with no lockfile, so the file in the tree does not fix the versions. mise's resolved versions do.
- The environment variables that change what a check reports: `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `RUSTDOCFLAGS`, `CARGO_BUILD_RUSTFLAGS`, `CLIPPY_CONF_DIR`, and `RUSTUP_TOOLCHAIN`.
- The contents of the Cargo configuration files outside the tree: the worktree's `.cargo/config.toml` (ignored by git, written by `setup-worktree-target.sh`) and `$CARGO_HOME/config.toml`.

Settings that only change speed, such as the target folder, `[build] jobs`, and the sccache wrapper, are in those files too. Including them is safe: at worst a pass is not reused.

### The record

One small file per key, in `$(git rev-parse --git-common-dir)/loom-check-passes/` (decision 4). The git common folder is shared by every worktree of a clone and deleted with it, and is never in a commit. A record is written to a temporary file and renamed into place, so agents running at once never read half a record.

The file holds what the skip line shows: the check name, the tree hash, when it passed, how long it took, and the branch and commit. A record older than 7 days is not reused, and run.sh deletes records older than that when it writes a new one, so the folder stays small.

### The skip line

prek hides a passing hook's output unless the hook sets `verbose = true`. A reused pass would then read as an ordinary `Passed`, which would mislead. So each reusable hook gets `verbose = true`, and run.sh prints one line when it reuses (decision 5):

```text
cargo clippy.............................................................Passed
- hook id: cargo-clippy
- duration: 0.26s

  reused: passed on this tree 4 min ago (31.2 s); LOOM_CHECK_REUSE=0 runs it
```

When the check does run, its own output shows, as with any verbose hook. fmt and typos print nothing when they pass, and the scripts print one line. clippy prints its `Finished` line. prek prints the hook ID and duration of a verbose hook even when it printed nothing.

### Where the passes come from

`mise run check` runs the quick checks as `prek run --all-files` (its `quick-checks` task, decision 6), so they go through run.sh and record their passes. A commit made right after it with `git add -A` or `git commit -a` then reuses fmt, clippy, adr-citations, and test-placement, and the push hook reuses them again if nothing changed. AGENTS.md asks for `mise run check` alone, no longer for a separate `prek run --all-files` first, so each quick check runs once before the hooks.

CI runs the mise tasks one by one (`mise run clippy` and so on), never `check` or `quick-checks`, so CI does not change.

### Forcing a full run

- `LOOM_CHECK_REUSE=0` in the environment turns reuse off for that command (for example `LOOM_CHECK_REUSE=0 git commit`) or for a whole shell. Checks still write pass records, so the next run can reuse them.
- `mise run check-reuse-clear` (`devtel.py reuse-clear`) deletes every pass record of the clone.
- Reuse is never on in CI. run.sh already runs the command directly when `CI` is set, and `LOOM_CHECK_REUSE=1` does not change that.
- Reuse is separate from `LOOM_DEVTEL`: turning telemetry off does not turn reuse off, and the other way round.

Skipping a hook with `--no-verify` is still never allowed (AGENTS.md). Writing a pass record by hand counts as skipping a hook.

### Telemetry

A reused check still writes its devtel span, with `loom.devtel.reused = true` and the duration of the pass it reused in `loom.devtel.reused_ms`. That lets devtel measure the change:

- `devtel_reruns` ("same tree") leaves reused checks out, because they cost almost nothing.
- The report gets a "Reused passes" line: count and time saved.
- The target: "hook after check" minutes per commit fall, compared over the two weeks before and after.

## Cost

Per wrapped check, reuse adds about 150 ms to a run that devtel already slows by about 150 ms: 45 ms for the tool versions, a few ms to read the Cargo configuration, and about 90 ms to hash the tree a second time after a pass. Measured on 2026-11-26 (prek 0.4.8, macOS, this repository), a reused check takes 0.3 to 0.7 s in prek's report, most of it starting Python and git, against 1 to 3 s for the scripts and fmt, and 109 s for a clippy run that rebuilt the workspace.

## Decisions

Brylie settled these on 2026-11-26, taking the recommended answer each time. ADR-0049 records the decision.

1. **Which checks.** The wrapped quick checks in [the table above](#what-is-reused), never tests. The commit and push hooks run no tests, so there is nothing to save there.
2. **What goes into the key.** The list in [The key](#the-key). A new input found later is added to the key and the format version goes up, so older records stop matching.
3. **Checks that receive file names (typos, adr-status).** Reuse only an exact match of the file list. Both take about a second. Recording a pass per file and tree, and reusing when every file asked for has one, was left for later: it is more code for little time.
4. **Where the record lives.** The git common folder of the clone, not the devtel store, spool, or archive. The archive is written only by the import and is slow to open, and the spool would have to be scanned. This also keeps reuse working with telemetry off, and the records go away with the clone.
5. **How a skip shows.** `verbose = true` on each reusable hook, so the `reused:` line shows. The cost is a hook ID and duration line for each of those hooks in every commit. A summary hook at the end (it would need the hooks of one prek run to share state) and a note only in each hook's name were the other options.
6. **`mise run check` records passes.** `check` runs the quick checks as `prek run --all-files`, and AGENTS.md drops its separate `prek run --all-files` step. The other options were to wrap each quick mise task with run.sh too, or to rely on the `prek run --all-files` step.
7. **CI.** Never reuses, and no switch turns it on there.
8. **The no-code option** (no checks by hand before committing, so the commit hook is the check) was not taken alone: an agent would find a lint error only while committing, the push hook would still repeat the commit hook, and loops of `prek run --all-files` would stay. Decision 6 takes its useful part: one check by hand, not two.

## Out of scope

- Reusing test results, in a hook or in `mise run check`.
- Sharing pass records between machines, or with CI.
- Reusing a pass across trees that differ only in files a check does not read (for example clippy and a Markdown change). That would need each check's list of inputs, which is easy to get wrong.

## Testing

`mise run devtel-test` runs `scripts/devtel/tests/test_reuse.py`, which works in a temporary git repository and tests: a pass is reused on the same key; any change to the tree, tool versions, a listed variable, or a Cargo configuration file runs the check; a failure is never recorded; a check that changes the tree (a formatter) is not recorded; `LOOM_CHECK_REUSE=0` and `CI` always run the check; a damaged record runs the check; old records are not reused and are pruned; and the skip line names the time and the way to force a run.
