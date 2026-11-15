# Polysynth: 8 voices as a share of real time

On 2026-11-15 the polysynth benchmark ran on the reference machine, for [#677](https://github.com/brylie/LoomDSP/issues/677). It measures the first reference patch of [NFR-PERF-01](../product/non-functional-requirements.md#performance-perf), `examples/patches/polysynth.ron`, as [polyphony rule 29](../design/polyphony.md#cost) says: the engine's mean time per block with 8 notes held, as a share of the block's real time. The app's UI is not part of it.

- **Machine:** Apple M1 Pro, MacBookPro18,1 (10 cores, 32 GB), macOS 27.0.1 (26A434). The maintainer's laptop.
- **Build:** `cargo bench`, the release profile, on PR #788's branch; the engine, module, and patch code is that of `86650796`.
- **Engine:** `EngineConfig::default()`: 48 kHz, 128-frame blocks, output safety at Low latency, as the app plays.

## The patch

The patch has 12 modules: MIDI Note at 8 voices, Voice Spread, Phase, Saw, LFO, Multiply, SVF, Envelope, Amplifier, Voice Merge, Audio Out, and Transport. Six of them run once per voice (Voice Spread, Phase, Saw, SVF, Envelope, Amplifier), which is 48 module runs. MIDI Note, the LFO, Multiply, Voice Merge, and the Transport run once each. So each block has 53 module runs, and the engine adds Audio Out's sum of the voices and the output safety stage.

The notes are MIDI 48, 52, 55, 59, 60, 64, 67, and 71, all sent on frame 0, so every voice plays. The benchmark plays 375 blocks (1 s) to warm up and checks that the output is not silent. Then it times 3,750 blocks (10 s) with one clock around the loop.

The Quietest variant is the same patch with the Envelope's `out` cabled to MIDI Note's `level` and Mode set to Quietest. That cable closes one feedback loop, so MIDI Note and the 8 envelopes run one sample at a time.

## Results

Two runs, one after the other:

| Patch | Mean per block | Share of real time |
|---|---:|---:|
| Polysynth, as shipped | 58.1 µs | 2.18% |
| Polysynth, Quietest with the `level` loop | 80.5 to 81.4 µs | 3.02% to 3.05% |

Criterion's own `patch/polysynth.ron` measured 58.2 to 58.3 µs per block in the same run, which agrees.

The first run printed:

```text
polysynth: 8 voices, 8 notes held, 48 kHz, 128 frames: 58.1 µs per block, 2.18% of real time (budget 25%)
polysynth (Quietest, level loop): 8 voices, 8 notes held, 48 kHz, 128 frames: 81.4 µs per block, 3.05% of real time (budget 25%)
```

A block of 128 frames at 48 kHz has 2.67 ms. The shipped patch uses about a twelfth of the 25% budget. The `level` loop costs about 23 µs more per block, or 40%, because a per-sample loop calls MIDI Note and each envelope once per frame instead of once per block. It still uses about an eighth of the budget.

## How much to trust it

This is one laptop, not a quiet benchmark machine. It was doing other work during the runs: the desktop, a chat app, file indexing, and another session's Rust build. The load average was about 10 to 13. Treat the numbers as an estimate. The check against the budget is a person's job, on Manual QA: M2 ([#643](https://github.com/brylie/LoomDSP/issues/643)).

## How to run it

```sh
cargo bench -p loom-modules --bench engine -- polysynth
```

The two lines print after criterion's results. They report and never fail, because the number depends on the machine. Without a filter, `cargo bench -p loom-modules --bench engine` prints them after every group. CI only compiles the benchmark (`cargo bench --no-run`), because shared runners are too noisy to judge time.
