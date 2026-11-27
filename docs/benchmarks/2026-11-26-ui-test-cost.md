# Cost of each loom-app UI test

On 2026-11-26, the unit tests of `loom-app` took **1,694 s** under nextest, and most tests that used the headless `Ui` harness took 13 to 64 s each. The cause was not fonts, layout, or unoptimised code. It was the kernel: once per test process, the harness read the whole system temporary folder, which held 112,772 entries. With the copies of example patches moved into a folder of their own, the same run takes **98 s**, and the test that took 39 s takes 0.55 s.

## What was measured

All runs used a debug build on macOS 27.0, an Apple M1 Pro with 10 cores and 32 GB of memory, with Cargo 1.100 and 5 test threads (`--test-threads 5`). Other agents were building and testing on the same machine the whole time, so the load average was 35 to 148. No quiet period came. The absolute seconds are therefore loose. The split between user and kernel time, the instruction counts, and the stack samples are firm, and the change is far larger than the noise.

The runs:

1. `cargo nextest run -p loom-app --lib`, the unit tests of the crate (1,709 tests, about 290 of them through `Ui::open`), with `/usr/bin/time -l` around it. Before and after the fix.
2. Single tests run directly from the test binary (`loom_app-… --exact NAME`), one process each, as nextest runs them, with `/usr/bin/time -l`.
3. The same single tests with `TMPDIR` pointed at an empty folder.
4. `sample` on a running UI test process for 5 s, to see where its time goes.
5. A probe test, not committed, that timed each start-up cost inside one process: Iced's font system, the first Parley measurement, and two `Ui::headless` calls.
6. `cargo test -p loom-app --lib`, which runs every test as a thread of one process, after the fix.

## Results

### The whole crate

| Run | Wall | User | Kernel (sys) | Median test | 90th percentile | 99th percentile | Slowest |
|---|---:|---:|---:|---:|---:|---:|---:|
| nextest, before (load 43 to 148) | 1,694 s | 317 s | 3,653 s | 0.04 s | 13.8 s | 57.6 s | 64.3 s |
| nextest, after (load 96 to 100) | 98 s | 125 s | 46 s | 0.06 s | 0.62 s | 2.3 s | 14.3 s |
| `cargo test`, one process, after (load 58 to 76) | 40 s | 54 s | 3 s | | | | |

The system temporary folder held 112,772 entries before and 112,778 during the run after, so the fix was measured against the same folder.

### One test, one process

| Test | Temp folder | Wall | User | Kernel | Instructions |
|---|---|---:|---:|---:|---:|
| `toolbar::tests::the_toolbar_is_one_tab_stop` | the shared one, 112,772 entries | 38.9 s | 0.69 s | 16.3 s | 34.5 G |
| the same | empty | 0.81 s | 0.13 s | 0.05 s | 1.1 G |
| the same, after the fix | the shared one | 0.55 s | | | |
| `ui_test::tests::a_number_steps_by_one_and_shows_the_base_value` | the shared one | 35.9 s | 0.67 s | 11.6 s | 34.0 G |
| the same | empty | 1.06 s | 0.21 s | 0.05 s | 1.5 G |
| the same, after the fix | the shared one | 0.62 s | | | |

### What each test process pays at start

From the probe, inside one process, at load 40 to 115:

| Cost | Time |
|---|---:|
| Iced's font system (`cosmic-text` loads the system fonts through `fontdb`) | 80 to 190 ms |
| The first Parley measurement (`fontique` scans the system fonts) | 140 to 240 ms |
| The first `Ui::headless` (app, bridge, first frame) | 50 to 170 ms |
| A second `Ui::headless` in the same process | 20 to 100 ms |
| A trivial test, the whole process | 0.02 s |

## Reading the results

- **The kernel did the work.** Before the fix, the unit tests spent 11 times more time in the kernel than in their own code. Every stack sample of the slow test was inside `sweep_old_copies` in `crates/loom-app/src/ui_test.rs`. For each entry of the temporary folder it read the entry's metadata, then resolved two paths with `realpath` (the temporary folder itself, and the entry's parent) before it looked at the name. That is about 225,000 `realpath` calls and 112,000 metadata reads for each test process.
- **The tests fed the cost.** Of the 112,772 entries, 107,536 started with `loom`: folders that other tests make with the process ID in their name and never remove (`loom-settings-…`, `loom-control-…`, `loom-prefs-…`, and others, about 1,500 of each). Every run of the tests left more, so every later UI test was slower. This is why a UI test took 19 s earlier on the same day and up to 64 s here.
- **Process per test multiplied it.** The sweep runs once per process. `cargo test` runs all tests in one process, so it paid the walk once. Nextest gives each test its own process (kept for the Vulkan loader crash in #827), so about 290 UI tests paid it about 290 times.
- **The integration tests did not pay it.** `Ui::headless`, which the tests in `crates/loom-app/tests` use, opens the example patch as it is, with no copy and no sweep.
- **CI probably never saw it.** A CI runner starts with an empty temporary folder. The cost grows only where tests run again and again on one machine, as they do on the development Mac.
- **What is left is the fixed cost of a process.** After the fix, nextest takes 98 s and `cargo test` takes 40 s. The gap is about 70 s of user time: each UI test process loads the system fonts twice, once for Iced and once for Parley, and builds the app and its first frame, about 0.3 s in all. The font scans are measured here and were not changed.

## The fix

The harness now makes every copy folder inside one folder of its own, `$TMPDIR/loom-app-ui/<process ID>-<count>`, and the sweep reads only that folder. Its cost follows the number of copies left behind, not what else is in the shared temporary folder. A copy folder is still removed only when it sits directly in that folder, so nothing outside it is ever removed. The test `copies_live_in_a_folder_of_their_own` checks both.

Folders from the old layout (`$TMPDIR/loom-app-ui-<process ID>-<count>`, 55 on this machine) are no longer swept and stay until the system clears its temporary folder.

## Options not taken in this change

- **Tests that clean up their temporary folders.** About 30 test helpers make a folder in the temporary folder and leave it. They no longer slow the UI tests, but they still fill the disk (the folder is 1.0 GB here) and slow anything else that reads it. One shared helper that removes its folder on drop would fix all of them.
- **Bundled fonts for tests.** Loading a small bundled font set instead of the system fonts would save up to about 0.4 s per UI test process and make text measurement the same on every OS. It changes what screenshots and layout tests measure, so it needs a decision first.
- **Fewer processes.** Running the tests that never touch the GPU in one process would remove most of the remaining fixed cost, about 60 s of CPU per run of the crate. Nextest has no way to run several tests in one process, so this needs a second runner for those tests. #827 decides where process per test is still needed.
- **Optimised text crates in test builds.** The text and layout crates (`fontdb`, `ttf-parser`, `cosmic-text`, `fontique`, `skrifa`, `parley`, `swash`, `taffy`) build at opt-level 0 in tests. User time is now 54 s for the whole crate in one process, so the gain is small, and every build of those crates would cost more.
