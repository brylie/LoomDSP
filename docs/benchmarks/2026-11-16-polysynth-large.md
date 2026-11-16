# Layered polysynth: a full instrument as a share of real time

On 2026-11-16 the engine benchmark measured the second reference patch of [NFR-PERF-01](../product/non-functional-requirements.md#performance-perf), `examples/patches/polysynth-large.ron`, for [#787](https://github.com/brylie/LoomDSP/issues/787). It follows [polyphony rule 29](../design/polyphony.md#cost): the engine's mean time per block with 8 notes held, as a share of the block's real time. The app's UI is not part of it. The patch is described in [The large reference patch](../design/polyphony.md#the-large-reference-patch).

- **Machine:** Apple M1 Pro, MacBookPro18,1 (10 cores, 32 GB), macOS 27.0.1 (26A434). The maintainer's laptop, the same machine as [the polysynth record](2026-11-15-polysynth.md).
- **Build:** `cargo bench`, the release profile, on the branch for #676, #787, #793, and #794, after commit `84a0c9fd` with the large patch added.
- **Engine:** `EngineConfig::default()`: 48 kHz, 128-frame blocks, output safety at Low latency, as the app plays.

## The patch

The patch file has 96 modules and 119 cables. MIDI Note at 8 voices feeds a Voice Assigner at 8 voices. After it, 64 modules run once per voice: four sound layers of 14 modules each (detune, phase, oscillator; a key-tracked LFO made of a pitch Add, its phase, the LFO, a slew, and a depth Multiply; a filter envelope, its depth Multiply, the cutoff Add, and the filter; and an amplitude envelope and Amplifier), the vibrato Add, and the voice's tail of 7 modules (mixer, noise level, noise sum, drive, clip, ladder filter, and output Amplifier). Voice Merge sums the voices. The stereo effect chain after it starts with a Join, and its six all-pass stages, the EQ, the wet and dry mix, and the master Amplifier run once per side.

So each block has **553 module runs**: 512 for the voice chain (64 modules times 8 voices), 18 for the stereo stages run once per side, and 23 for the modules that run once. The engine adds Audio Out and the output safety stage. `tests/polyphony.rs` counts this from the compile report, so the number cannot drift from the patch. The polysynth has 53 module runs per block, about a tenth of this.

The notes are MIDI 48, 52, 55, 59, 60, 64, 67, and 71, all sent on frame 0, so every voice plays. The benchmark plays 375 blocks (1 s) to warm up and checks that the output is not silent. Then it times 3,750 blocks (10 s) with one clock around the loop.

## Results

One run:

| Patch | Module runs per block | Mean per block | Share of real time |
|---|---:|---:|---:|
| Polysynth, as shipped | 53 | 61.7 µs | 2.32% |
| Polysynth, Quietest with the `level` loop | 53 | 84.4 µs | 3.17% |
| Layered polysynth | 553 | 489.4 µs | 18.35% |

Criterion's own `patch/polysynth-large.ron` measured 486.2 to 488.3 µs per block in the same run, and `patch/polysynth.ron` 61.9 to 62.2 µs, which agree.

The run printed:

```text
polysynth: 8 voices, 8 notes held, 48 kHz, 128 frames: 61.7 µs per block, 2.32% of real time (budget 25%)
polysynth (Quietest, level loop): 8 voices, 8 notes held, 48 kHz, 128 frames: 84.4 µs per block, 3.17% of real time (budget 25%)
polysynth-large: 8 voices, 8 notes held, 48 kHz, 128 frames: 489.4 µs per block, 18.35% of real time (budget 25%)
```

**The layered polysynth is within the 25% budget, with less room than the polysynth.** It uses about three quarters of the budget. Its cost per module run, about 0.88 µs, is close to the polysynth's 1.16 µs, so it costs what its size predicts: there is no single module that is unusually expensive. A patch about a third larger would reach the budget on this machine. That is a finding to keep in mind for M3 and later: a full instrument of this size fits, with little room for a second one.

## How much to trust it

This is one laptop, not a quiet benchmark machine. It was doing other work during the run: the desktop, a chat app, and another session. The load average was about 4 to 9. The polysynth's numbers here are about 6% above its record from the day before, which shows the size of that noise. Treat the numbers as an estimate. The check against the budget is a person's job, on Manual QA: M2 ([#643](https://github.com/brylie/LoomDSP/issues/643)).

## How to run it

```sh
cargo bench -p loom-modules --bench engine -- polysynth-large
```

The line prints after criterion's results. `-- polysynth` prints all three lines. They report and never fail, because the number depends on the machine. CI only compiles the benchmark (`cargo bench --no-run`), because shared runners are too noisy to judge time.
