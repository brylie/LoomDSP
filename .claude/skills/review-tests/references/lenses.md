# Lenses for a test health review

What to look for under each lens, where it has cost this workspace before, and which choices are deliberate. The examples are conclusions from one review at one commit. They show the pattern. They do not settle the case in front of you, so measure and read before you match an example.

## How to trust a number

- **Load first.** Every wall-clock time needs the load beside it. The script samples `os.getloadavg()` before each probe and at the start and end. Up to 0.5 per core is quiet. Above 1.0 per core, do not compare wall times with another run. Above 1.5 per core, the script refuses the full local run without `--force`. On the evening of 2026-11-26 this machine ran at 13 to 20 per core, because many agents were building at once. That is normal here, so plan for it.
- **CPU time holds up better than wall time.** User and kernel seconds for one process change little with load, because they count only the time the process ran. Their ratio is the clue: a test that spends ten times more in the kernel than in user code is walking a folder, opening files, or waiting on a system call, not computing.
- **CI and local runs have opposite blind spots.** CI runners are quiet and start with an empty temp folder, so their per-test times are comparable from run to run, but they never see a cost that grows with the machine's state. The local probe sees that cost, but its wall times depend on the load. Report both, and treat a large gap between them for the same test as a finding in itself.
- **Coverage instrumentation slows CPU-bound tests about two times.** The Coverage job's times are not the Linux test times. Use the macOS and Windows jobs for speed, and the Coverage job for "is it also slow on Linux".
- **The devtel archive has no load column.** A week in which test calls got slower may be a week in which more agents ran at once. Say so when you report a trend. The number of calls in the same week is a partial check.

## 1. Fixed cost per test process

nextest runs every test in a process of its own. `mise.toml` says why: concurrent offscreen tests that make separate GPU devices in one process can crash Ubuntu's Vulkan loader (#827). So any setup that runs once per process (a `static` initialised on first use, a `OnceLock`, a sweep, a font scan) is paid once per test, not once per run.

- **Measure it.** `--probe` runs the fastest test of each binary alone, several times, and reports wall, user, and kernel time. A floor above about 0.1 s for a binary of 1,000 tests costs minutes per run.
- **Where it bit.** `sweep_old_copies` in `crates/loom-app/src/ui_test.rs` walked the whole system temp folder once per process: 112,772 entries, with a `stat` and two `realpath` calls each. One UI test took 38.9 s, of which almost all was kernel time (3,653 s kernel against 317 s user for one `loom-app` run). The loom-app unit tests went from 1,694 s to 98 s once the copies moved to a folder of their own (#917, benchmark `docs/benchmarks/2026-11-26-ui-test-cost.md`). The leaks that fed it came from helpers that name a folder after the process ID and never remove it, such as `folder` in `crates/loom-app/src/settings/tests.rs`: about 1,470 folders per test. Nobody noticed for weeks, because CI's temp folder is empty.
- **Fonts.** Loading the system fonts costs about 0.1 to 0.4 s per process on this Mac (measured 2026-11-26, not pursued). Watch it if the font set grows.

## 2. Tests that depend on the machine

A test that reads the machine's state is slow or wrong on one machine and fine on CI.

- **Temp folder contents.** Anything that lists, sweeps, or searches `$TMPDIR`. The probe runs each test twice, with the shared folder and with an empty one; a large difference names the test.
- **Files left behind.** The probe counts what one test leaves in an empty temp folder. Each leak grows the shared folder for every later run on the machine, which is how the sweep above grew to 112,772 entries. The static count ("temp folders and never removes one") is only a hint; the probe's count is the measurement.
- **GPU.** CI sets `LOOM_REQUIRE_GPU=1`, so drawing tests fail, naming the cause, when no adapter is found, instead of passing silently. Locally a missing adapter skips them. Event tests need no GPU (the `Ui` harness lays out on `Measure`).
- **Audio and MIDI devices.** Tests that need hardware are `#[ignore]` with a reason and live in `device_tests.rs` files, which coverage leaves out. A test that opens a real device without being ignored is a finding.
- **Environment.** `HOME`, `TMPDIR`, `XDG_*`, and the user's preferences file. Tests must run on any OS and on CI; never a device name from this machine.

## 3. Waiting

- **Sleeps and polls.** A test that waits on real time is slow, and flaky under load. Prefer a step the test drives: `App::process(frames)`, a stepped `Tape` from the fake host (`crates/loom-app/src/qa_tests.rs`), or a channel the code under test signals.
- **A negative wait costs its whole budget.** `assert!(!wait_for(..))` can only pass by running out of time. At `db63db4c`, `a_learned_panic_trigger_is_saved_and_runs_panic` and `a_shared_note_ends_with_its_last_input` (`crates/loom-app/src/settings/tests.rs`) each poll 400 times with a 5 ms sleep: 2.06 s on Linux, 2.2 to 2.5 s on Windows, and 12.5 to 12.7 s on the macOS runner.
- **macOS sleeps overshoot.** On the macOS CI runner a 5 ms sleep lasted about 31 ms (the numbers above). A poll loop sized by its count of sleeps is about six times longer there than on Linux. Size a budget in time (`Instant` plus a deadline), not in iterations.
- **The grep has false positives.** The `poll` pattern matches any deadline or `elapsed()` comparison, and a variable named `waiting`. Read each hit.

## 4. The slowest tests

CI's per-test times are the map. Read the top of each job's list.

- **Sweeps over every module.** `non_finite::*`, `copy_state::*`, and `extreme::*` in `loom-modules::integration` run every module through a scenario: 12 to 30 s each per OS at `db63db4c`. They are deliberate (they enforce rules for every module, including ones not yet written), so the finding is never "delete". Their cost grows with the catalogue. The direction is usually one test per module (a macro or a generated list), so nextest spreads them over cores and a failure names the module, or fewer frames per module where the scenario allows.
- **Long renders.** `transport_stays_in_step_for_an_hour` and similar long-duration tests: check that the length is what the rule needs.
- **A test slow on one OS only** usually waits on something (section 3) or on the OS (file system, GPU driver).

## 5. Build cost

- **Binaries.** Each test binary links the whole dependency tree, so each one adds link time and disk. #904 made each crate's integration tests one binary (`tests/main.rs`, `autotests = false`), and `mise run test-placement` enforces it. At `db63db4c` nextest listed 29 binaries. Count them again; a new one needs a reason (a global allocator).
- **Build time.** CI's "Finished `test` profile ... in" line is the build time on a runner with dependencies cached (about 2.5 minutes at `db63db4c`). Locally, time `cargo nextest run --workspace --no-run`, with the load. A full local rebuild time means little; a build after touching one crate's file means more.
- **Doc tests** build a binary per example. `mise run doctest` runs them; watch their count.

## 6. Coverage

- **Never run coverage locally** (#876). CI's Coverage job uploads `lcov.info` as the `coverage` artifact of every run; the script downloads it with `gh run download`. Artifacts expire, so name the run and the commit. When the commit is not `HEAD`, say that line numbers may have moved.
- **Per crate, per file.** The gate is 80% of lines over the workspace (`LOOM_COVERAGE_MIN`). A crate well under the workspace average, or a file under 50%, is a place to read.
- **Low files that are deliberate.** The window shell's event loop (`loom-ui/src/shell.rs`), the Iced bridge's window, and device capture run only with a real window or device. Their low coverage is the cost of the headless test design. The finding there is logic that could move out of the shell into a tested part, not the percentage.
- **Public functions no test runs.** The script lists `pub fn`s none of whose body lines ran, with a count of call sites by crate. Read the count before the list:
  - *No callers:* dead code, which is a better finding than a missing test.
  - *Callers in other crates only:* probably not unrun. A small function called from another crate is compiled into that crate (`#[inline]`, or Rust's own cross-crate inlining of small functions), and llvm-cov then counts no hits in the function's own file. At `db63db4c`, `PortIo::uses_default` (`crates/loom-core/src/module.rs:342`), called by five modules, showed as never run. The script skips `#[inline]` and `const fn` for this reason; the rest need a look.
  - *Callers in its own crate:* a real gap, unless the name is common (`new`, `name`, `run`), where the count is every function of that name.
  - The first version of the script counted per lcov function record and listed about 1,600 functions; joining line counts brought it to 33. Check a few entries by hand on every run.
  - In `loom-app`, a binary, `pub` is still private to the crate.
- **Acceptance criteria.** Specs in `docs/design/` end with a checklist. A ticked criterion names no test, so "ticked" does not mean "tested". Sample the specs that changed most recently: for each ticked criterion, find the test (grep its key words in the crate's tests). Report the ones with no test and no "(Manual check.)" mark. Open criteria on an Implemented section are a finding for the spec's owner, not for the tests.

## 7. Hygiene

- **Flaky tests.** nextest retries are off (`.config/nextest.toml` has no `retries`), so a log never says FLAKY. Look instead at failed or re-run CI runs on `main` (the script counts the last 50) and at the error rate of agents' test calls in devtel. An agent that reruns a failing test until it passes leaves a loop in `devtel_loops`.
- **Ignored tests.** Each must have a reason (`#[ignore = "..."]`). Every ignored test at `db63db4c` had one. A reason that names hardware is fine; one that says "slow" or "flaky" is a finding.
- **Placement.** `mise run test-placement` enforces that tests live in their own files and that integration tests share a binary. Report only what it misses.
- **Duplicated fixtures.** The script hashes the bodies of helper functions in test files. Groups of the same helper in three or more files are candidates for a shared fixture. Check #627 (WAV writing, temp folders, offline rendering across the test crates) first.

## 8. Trends

`mise run devtel-report` and the script's devtel section give agents' tool time by kind of work, by week. A rising median for `test` calls is the earliest sign that the suite got slower for the people waiting on it, before any single test stands out. Compare with the call count, and say that load is not in the data.

## Deliberate choices

| Choice | Where it is recorded | Why it can look like debt |
|---|---|---|
| A process per test (nextest) | `mise.toml`, `[tasks.test]`; #827 | It multiplies any per-process setup. The fix is cheaper setup, not threads |
| Tests run in debug builds | `mise.toml`, `[tasks.test]` | Slower tests. The no-allocation test needs debug assertions (NFR-RT-06) |
| No coverage locally | AGENTS.md; #876 | It hides coverage from the local check. CI is the gate |
| Half the cores for tests | `mise.toml`, `[tasks.check]` | Slower local runs. Agents share the machine |
| `LOOM_REQUIRE_GPU=1` in CI | `.github/workflows/ci.yml` | Drawing tests fail on a runner with no adapter instead of skipping |
| Device tests ignored, out of coverage | `device_tests.rs`; `mise.toml`, `[tasks.coverage]` | Low coverage in the device code |
| `Ui::open` copies the example patch | AGENTS.md, App | A file copy per test. It keeps tests from rewriting shipped patches; the cost was the sweep, not the copy |
| Every-module sweeps | `crates/loom-modules/tests/` | The slowest tests. They enforce rules on modules not yet written |
