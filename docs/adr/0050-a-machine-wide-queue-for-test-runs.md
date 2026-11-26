# ADR-0050: A machine-wide queue for the test runs of the local check

- **Status:** Accepted
- **Date:** 2026-11-26
- **Related decisions:** ADR-0048 (local development telemetry in files)
- **Specification:** [Test runs on a shared machine](../dev/test-runs.md)

## Context

Several agents work on LoomDSP at once, each in its own worktree on one machine. Each runs `mise run check` before review, and its last step runs about 4400 tests with nextest at half the cores. Nobody knows when another session starts its tests. Two runs at once on a 10-core Mac pushed the load average to 41, each loom-app UI test took about 19 s instead of about 1 s, and one check took over 40 minutes. Two runs that fight for the cores, memory, and GPU take longer than the same runs one after the other.

Builds already share sccache and half the cores. Constraints: tools come through mise, CI is not affected, a crash or Ctrl-C must never leave the machine blocked, and it must work on macOS and Linux.

## Options considered

1. **Keep half the cores per run.** No code. Two runs still overload the machine, and one run alone uses only half of it.
2. **Fewer test threads.** Less overload, but a run alone is slower still, and three runs still collide.
3. **A queue of N slots, held with `flock` on files in a shared folder.** At most N runs at once, and the holder uses every core. The kernel frees a `flock` when its holder dies, so no stale lock is left. Python's `fcntl` provides it on macOS and Linux, where `flock(1)` is not on macOS.
4. **A lock made with `mkdir`, with a PID check for stale holders.** Works everywhere, but a reused PID or a holder on another machine sharing the folder can look alive, and the check is code that must be right.

## Decision

Option 3, with one slot by default, for the test runs of `mise run check`, `mise run test`, and `mise run coverage`. Brylie chose the slot count, the tasks, the folder (next to devtel's store), and that the build runs before the queue, so only the run waits (coverage excepted: its instrumented build and run are one command). The waiting run prints who holds the slot, and the wait is recorded as a devtel span. The queue is off in CI and with `LOOM_TEST_QUEUE=0`, and turns itself off where `flock` or the folder is not available.

## Consequences

- Two checks started together take about as long as one after the other, instead of much longer, and each run alone uses every core.
- An agent may wait minutes for the slot. It sees who holds it, and the check should run in the background.
- The test step of `mise run check` is now a Python script, not one line in `mise.toml`.
- CI runs the same tasks, so the wrapper must run in any shell, including cmd on Windows, and do nothing there.
