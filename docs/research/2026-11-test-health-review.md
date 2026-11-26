# Test Health Review: whole suite

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-26, on `main` at `db63db4c` (after #914).
- **Follows:** none. This is the first test health review.
- **Used by:** to be filled in after filing.

The first run of the `review-tests` skill (`.claude/skills/review-tests`). It was written on the day a session found that every `loom-app` UI test spent about 39 s walking the system temp folder, for weeks, with nobody noticing. The aim is to measure the whole suite once, so that the next review can show a trend, and to find any other cost of that kind.

## Method

- `mise run test-health -- --ci-run 45882353971 --probe --repeat 2 --timeout 240` (the script is `.claude/skills/review-tests/scripts/measure_tests.py`).
- CI timings and coverage come from CI run 45882353971, the green run on `main` at `db63db4c` (2026-11-26 19:16 UTC): the nextest lines in the logs of the macOS, Windows, and Coverage (Linux) jobs, and the `coverage` artifact's `lcov.info`. No coverage was run on this machine.
- The probe ran three tests of each of the 29 test binaries alone (the fastest, median, and 90th-percentile test by macOS CI time), twice with the shared temp folder and twice with a new empty one, and read wall, user, and kernel time per process.
- Devtel: agents' `test`, `check`, and `build` tool calls, by week, for 56 days (aggregates only).
- A subagent read three specs' ticked acceptance criteria against the tests: `control-channel.md` and `clock.md` in full, and the first 25 ticked criteria of `panels.md`. Two of its claims were checked by hand.
- The cold build of every test binary was timed: `nice -n 10 cargo nextest run --workspace --no-run`.

### Machine

10 cores (Apple Silicon). The machine was heavily loaded the whole time, by other agents' builds:

| Measurement | Load average (1 min) | Per core |
|---|---|---|
| Cold build of the tests, start and end | 128 and 252 | 12.8 and 25.2 |
| First measurement run, start and end | 191 and 197 | 19.1 and 19.7 |
| Probe run, start and end | 124 and 54 | 12.4 and 5.4 |
| `loom-app` probe rows | 117 | 11.7 |

So no local wall-clock time in this snapshot is comparable with another run. The findings rest on CI times, on CPU times (user and kernel), and on the ratio between a shared and an empty temp folder measured within minutes of each other. The full local run (`--local`) was not attempted.

## Summary

The suite is broad and mostly fast: 4,450 tests in 29 binaries, a median test of 13 to 45 ms on CI, every ignored test with a reason, no test outside its own file, and no CI failure on `main` in the last 50 runs. Its costs are concentrated. One is the temp-folder sweep in the UI harness, which CI cannot see, and it is still on `main`: the probe measured one UI test at 53 s here against 0.6 s with an empty temp folder, with 23 times more kernel than user time. The others are two tests that sleep through their whole polling budget (12.5 s each on macOS), and a handful of every-module sweeps that run serially in one test each (13 to 30 s). Agents feel it: the median agent test call grew from 5.1 s to 20.1 s over six weeks. First: merge the sweep fix and the leak cleanup already in flight, then turn the negative waits into stepped checks, then split the every-module sweeps per module.

## What to preserve

- **Process per test.** It costs per-process setup, but it isolates GPU devices (#827) and makes every test's time visible. The fix for slow setup is cheaper setup.
- **One integration binary per crate** (#904). 29 binaries in all; the cold build took 389 s of user CPU here, and 104 to 157 s on CI.
- **Placement rules** (`mise run test-placement`): no inline test modules were found.
- **Ignored tests carry a reason.** All 16 do; every reason names hardware, a by-hand tool, or a 12-hour render.
- **CI as the clean comparison.** Three OSes, the same tests, empty temp folders: the per-test times there are the baseline this review compares with.
- **High line coverage:** 93.3% of non-test lines, with the 80% gate in CI.

## Findings

| # | Lens | Finding | Measured | Priority | Size |
|---|---|---|---|---|---|
| 1 | Fixed cost, machine | The UI harness sweeps the whole temp folder in every test process (fix in flight, not merged) | 53.0 s wall, 17.8 s kernel, 0.78 s user here; 0.63 s with an empty temp folder; 0.39 s on CI | High (filed as a branch) | S |
| 2 | Machine | Tests leave `loom-*` folders in the temp folder, which feed finding 1 and grow without bound (fix in flight) | 108,531 `loom-*` entries of 113,647 | High (in flight) | M |
| 3 | Waiting | Two tests prove an absence by sleeping through their whole polling budget | 12.7 and 12.5 s on macOS, 2.1 s on Linux, 2.2 and 2.5 s on Windows | Medium | S |
| 4 | Slowest | Every-module sweeps run all modules serially inside one test | 13.9 s (macOS) to 30.4 s (Windows) for the slowest; 8 of the 10 slowest tests on every OS | Medium | M |
| 5 | Criteria | Ticked acceptance criteria with no test, or only part of one | 5 of 65 sampled with no test, 9 partly | Medium | S |
| 6 | Coverage | Dead public functions that coverage shows | 3 public functions with no caller | Low | XS |
| 7 | Trend | Agents wait longer on tests each week | Median test call 5.1 s to 20.1 s, p90 30 s to 93 s, in six weeks | Watch | none |

### 1. The UI harness sweeps the temp folder in every test process

- **Where:** `crates/loom-app/src/ui_test.rs:140` (`sweep_old_copies`), run once per process at `crates/loom-app/src/ui_test.rs:184` (`SWEPT.call_once`). It reads `std::env::temp_dir()` and checks every entry.
- **The problem:** nextest runs each test in a process of its own, so "once per process" is once per test. The sweep's cost grows with the temp folder, which on this machine held 113,647 entries.
- **The consequence:** every test that opens the `Ui` harness pays it. The harness is opened 403 times in 52 files of `loom-app`. CI never shows it: there the same test took 0.387 s.
- **Evidence (measured, probe):** `outline::tests::outline_cable_choices_reveal_a_destination_in_a_collapsed_section`, the binary's 90th-percentile test: 53.0 s wall, 0.776 s user, 17.8 s kernel with the shared temp folder; 0.634 s wall and 0.064 s kernel with an empty one (load 11.7 per core). The kernel-to-user ratio of 23 is the signature of a folder walk. The binary's fastest and median tests do not open the harness, and took 0.05 s.
- **Direction:** the fix is on branch `claude/sharp-blackwell-a6cc06` (copies live in a folder of their own, and the sweep reads only that), with a benchmark of the unit tests going from 1,694 s to 98 s. It has no pull request yet. Open and merge it.

### 2. Tests leave folders behind in the temp folder

- **Where:** test helpers across the workspace that make `loom-*` folders and never remove them. The static count found only two test files that make temp folders and remove none (`crates/loom-app/src/input_owner/tests.rs`, `crates/loom-app/src/screenshot/text_gate_tests.rs`), so most leaks are in helpers that remove on success only, or that the grep cannot see.
- **The problem:** 108,531 of the 113,647 entries in this machine's temp folder are `loom-*` folders. None of the 82 tests probed left anything in an empty temp folder, so the leaking tests are among those not probed.
- **The consequence:** the folder grows with every test run on every developer machine. With finding 1 that was a slowdown; without it, it is still disk, and a trap for the next sweep.
- **Direction:** a session started on 2026-11-26 is making the tests clean up their temp folders (about 30 helpers, by its count). It has no issue or pull request yet. When it lands, rerun the probe with `--repeat 1` over all binaries to confirm. Purging the existing folders is the owner's call, because other agents' runs use live folders there.

### 3. Negative waits sleep through their whole budget

- **Where:** `crates/loom-app/src/settings/tests.rs:957` (`a_learned_panic_trigger_is_saved_and_runs_panic`) and `crates/loom-app/src/settings/tests.rs:1046` (`a_shared_note_ends_with_its_last_input`). Each asserts `!wait_for(..)`, and `wait_for` polls 400 times with a 5 ms sleep (`crates/loom-app/src/settings/tests.rs:928` and `:970`).
- **The problem:** a check that something does not happen can only pass by running out of polls. The budget is sized in iterations, not in time, and a 5 ms sleep on the macOS runner lasts about 31 ms.
- **The consequence:** 25 s of every macOS CI run, and at least 4 s of every local run on any OS, is spent sleeping. It also contradicts CONTRIBUTING.md, which says to advance a simulated clock and keep real timeouts for guarding against a hang.
- **Evidence (measured, CI run 45882353971):** 12.727 s and 12.534 s on macOS; 2.068 s and 2.061 s on Linux (Coverage job); 2.208 s and 2.500 s on Windows. The sibling tests in the same file that only wait for something to happen take 0.04 to 0.2 s. A third negative wait (`:1258`) is in an ignored test.
- **Direction:** for "nothing happens", run the engine a known number of frames (the stepped fake host or `App::process(frames)`, as `crates/loom-app/src/qa_tests.rs` does), then check once. Size every remaining poll by a deadline in time, not a count of sleeps. S.

### 4. Every-module sweeps run serially inside one test

- **Where:** `crates/loom-modules/tests/non_finite.rs:69` to `:98` (four tests over `check_all`, `:43`), and the `copy_state::every_playing_module_hands_over_its_state_*` and `extreme::*` tests in the same binary.
- **The problem:** each test loops over every DSP module, each choice option, each input, three bad values, and two block sizes (`crates/loom-test/src/lib.rs:1020` to `:1028`), on one core.
- **The consequence:** they are the slowest tests on every OS, and they grow with the catalogue. On Windows the slowest, 30.4 s, is the floor of the whole 163 s test run. A failure message lists every failing module, which is worth keeping.
- **Evidence (measured, CI):** macOS 13.9, 12.6, 12.4, 9.2 s; Windows 30.4, 30.2, 27.0, 17.1 s; Linux under coverage 27.5, 25.9, 25.5, 19.3 s. In each job, 8 of the 10 slowest tests are these.
- **Deliberate in part:** they enforce rules for every module, including modules not yet written, so the finding is not "fewer checks".
- **Direction:** one test per module, generated from the registry list at build time or with a macro, so nextest spreads them over cores and a failure names its module. Keep one small test that the generated list matches the registry. M.

### 5. Ticked acceptance criteria with no test

- **Where:** a sample of 65 ticked criteria: all of `docs/design/control-channel.md` (16) and `docs/design/clock.md` (24), and the first 25 of `docs/design/panels.md`.
- **The problem:** criteria name no test, so a tick says that someone checked once, not that a test keeps checking. 51 had a test, 9 only partly, 5 none.
- **No test found:**
  - `control-channel.md:274`: `--measure-idle` counts no frames when no command arrives.
  - `control-channel.md:282`, `:283`, `:327`: the device-check script and the manual check's text. These are by-hand steps without the "(Manual check.)" mark, so this is a spec fix, not a missing test.
  - `panels.md:94`: Audio In pair labels, the Mixer's strip labels, the Button label, the Level Meter caption, and the Scope zero line.
- **Partly tested** (the part with no test): `panels.md:90` (the test at `crates/loom-modules/src/tests.rs:358` forbids radio columns but accepts a select list, and its comment allows one, so the test and the spec disagree; checked by hand); `panels.md:360` (Scope parameter names and ranges); `panels.md:362` (the tile menu includes Help); `panels.md:468` (every module has a non-empty panel); `panels.md:480` and `:481` (inspector and saving after a knob drag; dragging the header moves the tile); `clock.md:377` (Phase Trigger on a backward wrap, and a first frame that is not 0); `control-channel.md:276` and `:326` (typing one character per frame; a script file with a variable).
- **Direction:** add the missing tests as one issue with a checklist; mark the by-hand lines "(Manual check.)"; settle `panels.md:90` (spec or test). S.

### 6. Dead public functions that coverage shows

- **Where:** `crates/loom-core/src/graph.rs:239` (`is_intrinsic`), `crates/loom-patch/src/hierarchy/slots.rs:370` (`holds_slot`), and `crates/loom-test/src/lib.rs:971` (`Harness::check_reference`). No line of their bodies ran on CI, and no call to them was found in the workspace.
- **The consequence:** small. Each is API to keep, document, and test, for no user. `check_reference` matters more than the others: it is the documented way to compare with a reference file.
- **Evidence:** read and grepped. 33 public functions in all showed no body line run. Most of the rest are in the device and window shells (finding-free, see Deliberate), or are called only from other crates, where inlining hides their hits. One was checked by hand: `PortIo::uses_default` (`crates/loom-core/src/module.rs:342`), called by five modules, shows as never run.
- **Direction:** remove the three, or say in a doc comment who calls them. XS.

### 7. Trend: agents wait longer on tests

Devtel, agents' `test` tool calls by week:

| Week of | Calls | Minutes | Median s | p90 s | Error % |
|---|---|---|---|---|---|
| 2026-10-12 | 153 | 46 | 5.1 | 30.4 | 0.7 |
| 2026-10-19 | 337 | 115 | 8.9 | 46.8 | 1.8 |
| 2026-10-26 | 277 | 79 | 8.4 | 32.3 | 0.7 |
| 2026-11-02 | 409 | 400 | 13.2 | 60.7 | 0.7 |
| 2026-11-09 | 724 | 319 | 12.8 | 64.5 | 0.7 |
| 2026-11-16 | 681 | 427 | 15.6 | 87.3 | 0.4 |
| 2026-11-23 (partial) | 266 | 194 | 20.1 | 92.9 | 0.8 |

The median call quadrupled while the number of calls only doubled to quadrupled, so each call got slower, not only more frequent. Devtel has no load column, and more agents ran at once in the later weeks, so part of this is load. Finding 1 alone explains a large share: it cost the `loom-app` unit tests about 1,600 s per run on this machine. Compare the next snapshot's weeks with these, after findings 1 and 2 land.

## Coverage map

From the `coverage` artifact of run 45882353971 (the same commit). 93.3% of non-test lines in all.

| Crate | Lines | Covered % |
|---|---|---|
| loom-cli | 1,137 | 71.8 |
| loom-host-desktop | 1,481 | 72.9 |
| loom-modules | 4,014 | 90.9 |
| loom-ui | 9,111 | 91.2 |
| loom-app | 32,217 | 93.8 |
| loom-patch | 7,240 | 93.9 |
| loom-rt | 56 | 94.6 |
| loom-canvas | 6,635 | 96.0 |
| loom-core | 8,452 | 96.5 |
| loom-midi | 73 | 97.3 |
| loom-test | 789 | 98.2 |
| loom-analysis | 140 | 98.6 |
| loom-dsp | 1,402 | 99.4 |
| loom-ui-core | 312 | 99.7 |

The lowest files of 50 lines or more are the window shell (`crates/loom-ui/src/shell.rs`, 24.1%; `shell/gpu.rs`, 64.3%), device capture and input (`crates/loom-host-desktop/src/capture.rs`, 44.2%; `input.rs`, 53.6%), the Iced bridge (`crates/loom-app/src/iced_bridge/preedit.rs`, 45.5%; `window.rs`, 45.8%), and the command-line entry points (`crates/loom-cli/src/main.rs`, 56.1%). One module file is low: `crates/loom-modules/src/mix.rs`, 68.1%. It was not read in this run.

## Deliberate

- **Low coverage in the shells.** The window shell, the Iced bridge, and device code run only with a real window or device; tests reach their logic through `devices::Host` and the headless `Ui` harness. The Iced bridge is due to go in phase 4 of the UI layer plan. Not findings, unless logic in them could move out.
- **Process per test**, **debug test builds**, **no local coverage**, **half the cores for tests**, **`LOOM_REQUIRE_GPU=1` on CI**, and **ignored device tests**: see the table in `.claude/skills/review-tests/references/lenses.md`. None has grown in cost beyond finding 1.
- **`loom-patch::malformed`'s `changed_example_patches_load_safely`** (2.9 s on macOS CI) loads every changed example patch: it is as long as the examples.

## Already filed or in flight

- Finding 1: branch `claude/sharp-blackwell-a6cc06`, no pull request yet.
- Finding 2: a session started on 2026-11-26, no issue or branch named yet.
- A machine-wide queue for test runs and test selection by affected crate: branch `claude/gifted-khorana-ee8ad6` (ADR-0050, ADR-0051), not merged. It cuts how many full runs agents start, which matters for finding 7.
- Reuse of a check that passed on the same tree in git hooks: branch `devtel-check-reuse` (ADR-0049), not merged.
- Shared fixtures for WAV writing, temp folders, and offline rendering: #627 (open). The script found 28 groups of helpers with the same body in two or more test files; the largest repeat three times (`style` in the text tests, `is_test_file` and `is_user_dependency` in the purity tests). Add those to #627 rather than filing them apart.
- `loom-modules`' integration tests as one binary: #889 (open). The binary is already one at `db63db4c` (`loom-modules::integration`, 492 tests). #889 may be done; check and close it.

## Hygiene, nothing to report

- **Flaky:** the last 50 CI runs on `main` had no failure and no re-run. Agents' test calls failed 0.4% to 1.8% of the time per week. The devtel loop list (one command three or more times within ten calls) held two test commands, each in one session, which were not read; a rerun loop is a cheap thing to check next time. nextest retries are off, so a flaky test would show as a failure, and none did.
- **Ignored tests:** 16, all with a reason.
- **Placement:** no inline test module outside a test file.
- **Machine dependence other than the temp folder:** the probe found no other test whose shared and empty runs differ by more than the load explains.

## Limits

- Every local wall time was taken at a load of 5 to 25 per core. Only the ratio within one probe row, and CPU times, were used.
- The probe ran 82 of about 4,450 tests (fewer than three per binary where a binary has few tests). A leak or a per-test cost in a test not picked was not seen.
- No full local run; no per-test local times beyond the probe.
- Build cost: one cold build only (7 min 16 s wall, 389 s user, 37 s kernel, load 12.8 to 25.2). An incremental build after one edit was not timed here; `docs/benchmarks/2026-11-26-build-times.md` has those numbers.
- Acceptance criteria: three specs sampled, 65 of 691 ticked criteria. A subagent did the reading; two of its claims were checked by hand.
- Coverage of `#[inline]` and small cross-crate functions is under-counted by llvm-cov, so the public-function list is a map.
- Doc tests were neither timed nor counted.

## What to do next

1. Open and merge `claude/sharp-blackwell-a6cc06` (finding 1), and land the temp-folder cleanup (finding 2). Then rerun `mise run test-health -- --probe` on a quiet machine to confirm both.
2. File one issue for the negative waits (finding 3), P1 in the current milestone, because every macOS run pays it. Size S.
3. File one issue for splitting the every-module sweeps per module (finding 4). Size M.
4. File one issue with a checklist for the acceptance criteria (finding 5), and fix the unmarked by-hand lines in `control-channel.md` in the same change.
5. Add the three dead functions (finding 6) as a checklist row on an existing cleanup issue, or as one XS issue.
6. Check #889 against `db63db4c` and close it if its "Done when" is met.
7. Run this review again in about a month, and compare finding 7's table.
