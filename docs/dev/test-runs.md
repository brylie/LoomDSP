# Test runs on a shared machine

**Status: Agreed** (2026-11-26, with Brylie's answers to the open questions: one slot, `check`, `test`, and `coverage` queue, the folder next to devtel's store, and the build runs before the queue). Decisions: [ADR-0050](../adr/0050-a-machine-wide-queue-for-test-runs.md) (the queue) and [ADR-0051](../adr/0051-the-local-check-tests-only-what-a-change-can-reach.md) (test selection). Code: [`scripts/test_run.py`](../../scripts/test_run.py).

Most changes to LoomDSP are made by agents working in parallel worktrees on one machine. Each one runs `mise run check` before it asks for a review, and the last step of that check runs about 4400 tests. Nobody knows when another session starts its tests. On 2026-11-26 two runs at once pushed the load average of a 10-core Mac to 41: each loom-app UI test took about 19 s, and one check ran for over 40 minutes. Two runs at once take longer than the same two runs one after the other, because they fight for the cores, the memory, and the GPU.

Two changes fix this, and each works without the other:

1. **A queue.** At most one test run at a time on the machine, across every worktree. A run that has to wait says who holds the slot. The run that holds it uses every core.
2. **Selection.** `mise run check` runs only the tests of crates that the change can reach. A change to docs runs no Rust tests at all.

The queue applies to the test runs of `mise run check`, `mise run test`, and `mise run coverage` on a contributor's machine. Selection applies to `mise run check` only. CI runs every test, with no queue, as it does now.

## The test step

`[tasks.check]` in `mise.toml` runs one command after the quick checks:

```sh
python3 scripts/test_run.py
```

The script does this, in order:

1. **Select** the crates to test (see [Selection](#selection)). If none, print why and exit 0.
2. **Build** the test binaries, outside the queue: `cargo nextest run --workspace --no-run`, with the extra arguments of the run that change the build (such as `--release`, `--features`, or `--cargo-profile`), so the run builds nothing while it holds the slot. Arguments for the run only, such as `--no-fail-fast`, stay off the build, since nextest refuses them beside `--no-run`. Building uses the worktree's `[build] jobs` (half the cores, from `scripts/setup-worktree-target.sh`), and sccache shares the work between worktrees, so building beside another run is fair. Only the run waits.
3. **Take a slot** (see [The queue](#the-queue)). Print who holds it while waiting.
4. **Run** the selected tests: `cargo nextest run --workspace --test-threads N -E FILTER`. The binaries are already built, so this builds nothing. A selected run adds `--no-tests=warn`, since a selected crate may have no tests of its own. `N` is every core while the run holds a slot, and half the cores when the queue is off. In CI the script passes no thread count, so nextest uses its own default, as before. A thread count given after `--` (`--test-threads N` or `-j N`) wins, and the script adds none of its own, since nextest refuses two. With the queue off, step 2 is skipped: the run builds what it needs, as a plain `cargo nextest run` does.
5. **Free the slot**, record the wait and the run as devtel spans, and exit with nextest's exit code.

The build and the run both pass `--workspace`, as CI does, so cargo resolves features the same way for every selection and nothing is rebuilt between a selected run and a full one. The filter only picks which built tests run.

### Settings

| Variable | Default | Effect |
|---|---|---|
| `LOOM_TEST_QUEUE` | on | `0` turns the queue off. Off in CI (`CI` set) unless `LOOM_TEST_QUEUE=1`. |
| `LOOM_TEST_SLOTS` | `1` | How many test runs may run at once on the machine. |
| `LOOM_TEST_QUEUE_DIR` | `$XDG_DATA_HOME/loomdsp/test-queue`, else `~/.local/share/loomdsp/test-queue` | The shared folder for the lock files. |
| `LOOM_TEST_THREADS` | every core with a slot, half without, nextest's own in CI | nextest's `--test-threads`. |
| `LOOM_TEST_ALL` | off | `1` runs every test, with no selection. The same as `mise run check -- --all`. |
| `LOOM_TEST_BASE` | `origin/main` | The branch the change is compared with. |

## The queue

### Slots

The queue folder holds one lock file per slot, `slot-0.lock` up to `slot-(N-1).lock`. A run takes a slot with `fcntl.flock(LOCK_EX | LOCK_NB)` on one of them, trying each in turn, and tries again every second until one is free.

`flock` is held by the open file, not by a process ID. The kernel frees it when the last process that holds the file closes it or dies, even after `kill -9` or a crash. So there is no stale lock to detect and no PID to check. The lock file is not passed to the command the script runs. A long-lived process started under the run, such as the sccache server that a build starts, would otherwise keep the lock after the run and block the queue for good. The cost: when the script itself is killed with `kill -9`, the slot frees at once while its tests may still run for a while, so two runs overlap briefly. Ctrl-C reaches nextest (the script forwards SIGINT, SIGTERM, and SIGHUP, as devtel's wrapper does), nextest stops its tests, and the script frees the slot when nextest exits.

`flock` works on macOS and Linux. On Windows, or when the folder cannot be written, the script prints one line saying the queue is off and why, and runs without it. A test run is never blocked by the queue failing.

### Who holds the slot

Next to each lock file, the holder writes `slot-K.json`: worktree path, branch, process ID, start time, and what it runs (every test, or the tests of how many crates). This file only informs. The lock is the truth: a JSON file whose lock is free is ignored.

A waiting run prints, once at the start and then every 60 seconds:

```text
test queue: waiting for a test slot (1 of 1 in use)
  slot 0: gifted-khorana-ee8ad6 on claude/gifted-khorana-ee8ad6, the tests of 4 crates, since 14:02:11 (3m 40s)
  LOOM_TEST_QUEUE=0 skips the queue.
```

When it gets the slot, it prints how long it waited.

### Nested runs

While the script holds a slot, it sets `LOOM_TEST_SLOT_HELD=1` for its children. A run started inside a run that holds a slot does not queue again, so a task that calls another task never waits for itself.

### Which tasks queue

- `mise run check`: its test step, as above.
- `mise run test`: its nextest run, through `python scripts/test_run.py --all` (`python`, because CI runs this task on Windows, where mise's Python may have no `python3` command). It builds first and selects nothing, so it runs every test, with every core while it holds the slot. Its doc tests (`mise run doctest`) run after it, outside the queue, because they are few and light.
- `mise run coverage`: its `cargo llvm-cov nextest` line, through `python scripts/test_run.py queue -- COMMAND`. The instrumented build cannot be split from the run there, so this one waits before it builds. Agents do not run coverage locally (CI is the coverage gate), so this costs little.
- `mise run check-full` runs `coverage`, so it queues through it.

`queue -- COMMAND` is the general form: it takes a slot, runs the command with the slot held, and frees it. Every task line stays a plain command, so it runs in cmd on Windows too, where the queue turns itself off.

## Selection

### What changed

The changed files are every file that differs between the working tree and the merge base of `HEAD` with `LOOM_TEST_BASE` (`origin/main`): committed, staged, unstaged, and untracked but not ignored, with both the old and the new path of a rename. Agents run the check before they commit, so uncommitted files count.

If the base branch is missing or has no merge base with `HEAD`, every test runs.

### What a file reaches

Each changed file falls in one of three classes. The first rule that matches wins.

1. **Everything.** Files that any crate's build or tests may read, or that change how every crate builds: `Cargo.lock`, the root `Cargo.toml`, each crate's `Cargo.toml` (it can change the features of shared dependencies), `mise.toml`, `rust-toolchain` and `rust-toolchain.toml`, `.config/`, `.cargo/`, `examples/`, and `docs/modules/`. These run every test.
2. **One crate.** A file under `crates/NAME/` belongs to that workspace member. It selects that crate and every crate that depends on it, through normal, dev, or build dependencies, as `cargo metadata` lists them.
3. **Nothing.** Files no Rust test reads: `docs/` (but not `docs/modules/`), `scripts/`, `site/`, `.github/`, `.claude/`, `fuzz/`, the Markdown files at the root, `prek.toml`, `_typos.toml`, `rustfmt.toml`, `deny.toml`, `codecov.yml`, `skills-lock.json`, `.gitignore`, `LICENSE`, and `AI_POLICY.md`. Their own checks (typos, the script tests, the ADR checks) still run in `mise run check`.

Any other file runs everything. The list of files that select nothing is an allow list on purpose: a new kind of file outside `crates/` is tested in full until someone checks that no test reads it and adds it here.

A test that reads another crate's files by path, not through a dependency, would be missed. Today none does: tests outside their own crate read only `examples/` and `docs/modules/`, which are in class 1. A new test that reads across crates must add the folder it reads to class 1.

### What the output says

`python3 scripts/test_run.py --explain` prints the choice below and exits, with nothing built or run.

Before the build, the script prints what it chose and why:

```text
test selection: 4 of 14 crates (changed since the merge base with origin/main)
  run:  loom-dsp (changed), loom-modules, loom-patch, loom-cli (depend on it)
  skip: loom-app, loom-canvas, loom-ui, loom-ui-core, ... (no changed crate reaches them)
  LOOM_TEST_ALL=1 or `mise run check -- --all` runs every test.
```

For a full run it names the first file that caused it (`examples/patches/sine.ron changed`), and for an empty one it says that only files no Rust test reads changed.

## Devtel

When devtel is on ([devtel](devtel.md)), the script records two spans as children of the current trace (`TRACEPARENT`), with service `tests`:

- `test queue wait`, from the moment the build finished until the slot was taken, or until the run was cancelled while waiting. Attributes: `loom.test.wait_ms`, `loom.test.slots`, `loom.test.holder` (the worktree that held the slot when the wait began).
- `test run`, from the slot until the end of the run. Attributes: `loom.test.selection` (`all`, `some`, or `none`), `loom.test.crates`, `loom.test.reason`, `loom.test.threads`, and `process.exit.code`.

`mise run devtel-report` shows the total queue wait and the selected, full, and empty runs over the report's days, so the effect of both changes can be measured.

## Out of scope

- Queueing builds. Builds already share sccache and half the cores.
- Selecting tests inside a crate (by module or by file). Crates are the unit cargo and nextest already know.
- Selection in CI, or in `mise run test`, `coverage`, and `check-full`.
- Queueing the doc tests and the other tasks that build or run code (`docs`, `wasm`, `bench`).
- Windows support for the queue.

## Testing

`scripts/tests/test_test_run.py` runs in `mise run check` (`mise run test-run-test`), with only Python:

- The classes: each rule in [What a file reaches](#what-a-file-reaches), with a made-up package graph.
- The reverse closure follows dev and build dependencies.
- The changed set from a temporary git repository: committed, staged, untracked, renamed, and a missing base.
- Two processes and one slot: the second waits and prints the holder; it gets the slot when the first exits, and at once when the first is killed with SIGKILL, though its child still runs.
- `LOOM_TEST_SLOT_HELD`, `LOOM_TEST_QUEUE=0`, and `CI` skip the queue.
- An unwritable folder runs without the queue.
- The devtel spans, written to a temporary store.
