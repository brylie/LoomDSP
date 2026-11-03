# DSP Algorithms Specification

The conventions of `loom-dsp`, the crate of pure algorithms that modules wrap.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** [ADR-0035](../adr/0035-pure-dsp-algorithms-in-loom-dsp.md), with [ADR-0018](../adr/0018-clean-room-module-development.md) (clean room) and [ADR-0022](../adr/0022-copies-take-state-from-the-running-instance.md) (state copy).
- **Requirements:** FR-SDK-06, NFR-PORT-02.
- **Issues:** [#537](https://github.com/brylie/LoomDSP/issues/537)
- **Code:** `crates/loom-dsp`.

## Conventions

| Item | Rule | Why |
|---|---|---|
| `tick` | Processes one sample. | A feedback loop needs one sample at a time, and block size cannot change a result that has no blocks. |
| `reset` | Returns to the state of `new`, with no allocation. `Noise` has `restart` instead, because it needs a seed. `Ladder::reset` and `Pulse::reset` take the value to start from. | A reset object must equal a new one, so tests can say so. |
| Coefficients | A small `Copy` type, computed apart from `tick`. | The caller decides how often to pay for a tangent. |
| Several settings of one type | A struct with named fields, as in `envelope::Settings`, `limiter::Settings`, and `ladder::Settings`. | Four `f32` values in a row are easy to swap, and the compiler cannot see it. |
| A choice | An enum, as in `Colour`, `Slope`, and `limiter::Mode`. | A bad value cannot compile. A `usize` slope could panic at run time. |
| Units | Plain `f32` with the unit in the name, such as `cutoff_hz`. No newtypes yet. | Nobody needs them yet. |
| Samples | `f32`. An algorithm may use `f64` inside. | It is what cables carry. `Phasor`, `Slew`, and `Dynamics` need `f64`: a small step on a large value rounds away in `f32`, and the state stalls. |
| `new` | The only method that allocates. | The audio thread must never allocate. |
| `copy_from` | An algorithm with heap memory has one. It returns false, with nothing changed, if the two have different room. It never allocates. | Stereo copies take state on the audio thread (ADR-0022), and a derived `clone_from` allocates. |

## Two layers for band-limited shapes

A band limit needs the phase step. Each shape has a function that takes it (`saw`, `triangle`, `pulse`) and a struct that works it out from the last phase (`Saw`, `Triangle`, `Pulse`).

- **The function** suits a caller who knows its rate. A reset or a hard sync jump is not mistaken for a very high pitch. The step is a size: for a phase running backward, a caller passes the absolute value of the negative step from `Phasor::step_for`.
- **The struct** suits a phase from a cable, which can be bent or modulated with nothing to say how fast it moves. This is what the modules use today.

The sync API itself waits for its own specification. See [phase and cycles](../learn/phase-and-cycles.md).

## Keeping coefficients

Coefficients are `Copy`, and their settings can be compared. So a wrapper keeps them until a setting moves (`crates/loom-modules/src/cached.rs`). A tangent per sample costs nothing to skip when no knob moves, and audio-rate modulation still gets a new value each sample.

## What stays out

Ports, parameter ranges, knob mapping, smoothing, panels, and display values stay in modules. So do `loom.phase.scale`, `wrap`, and `trigger`, and patching logic. The pitch scale, `WrapDetector`, `EdgeDetector`, and `trigger` stay in `loom-core`, because they describe LoomDSP's cables.

## Guards

An algorithm documents the input it accepts and does not check it in a release build. A debug build asserts it, so every module test also checks that its wrapper guards. Two kinds of check stay in the algorithm, because a caller cannot do them:

- one inside a feedback path, such as the all-pass's check on what it writes;
- one that keeps a read inside memory, such as a delay time.

`Phasor::step_for` limits its step and turns NaN into 0, because a caller gets a valid step from it with no other work.

## What a caller must do

- **Stay in the documented range,** and smooth parameters.
- **Handle denormals.** `DcBlocker` and `Allpass` flush for themselves. `Svf`, `OnePole`, and `Ladder` do not, so a caller turns on flush-to-zero where the CPU has it. LoomDSP's desktop host does. WebAssembly has no such setting.
- **Expect repeatable results within one build on one target.** Rust does not fix the precision of `tan`, `powf`, `exp`, or `sin` across targets and versions, and flush-to-zero changes results.

## Checks

| Check | What it enforces | Where |
|---|---|---|
| No allocation | `tick`, `reset`, and `copy_from` of every algorithm, including the limiter's fullest queue | `crates/loom-dsp/tests/no_alloc.rs` |
| No user dependencies | Any `[dependencies]` or `[build-dependencies]` entry fails | `crates/loom-dsp/tests/purity.rs` |
| Lints | No `unsafe`, panics, or printing; docs, `Debug`, and `#[inline]` on public items | `crates/loom-dsp/src/lib.rs` |
| Banned APIs | Locks, threads, files, sockets, clocks, environment, and random hashing | `crates/loom-dsp/clippy.toml` |
| Compile-time | Every algorithm is `Send`, and every coefficient type is `Copy` | `crates/loom-dsp/src/static_checks.rs` |
| Input ranges | `debug_assert!` on documented ranges | each `tick` and `new` |
| WebAssembly | The crate builds for `wasm32-unknown-unknown` | `mise run wasm` |
| Same sound | A move of code keeps reference renders identical, bit for bit | `crates/loom-modules/tests` |

## Adding an algorithm

1. Put it in `crates/loom-dsp`, as a plain struct, following the conventions above. A new family gets its own module.
2. Write down its sources from published theory (ADR-0018), or say it is built from first principles. Do not cite from memory.
3. State the input range in the docs, and `debug_assert!` it.
4. Give the module a short runnable doc example.
5. Test it against its theory in `foo/tests.rs`, with no module API.
6. Add it to `tests/no_alloc.rs` and `src/static_checks.rs`.
7. Break it on purpose once, and see a test fail.
8. Wrap it in a module for ports, mapping, smoothing, and guards.

## Algorithms and sources

"Confirmed" means a publisher listing or an index shows the reference exists with these details. It does not mean the text was compared with the code. "Citing source" means it was seen only in another work's references.

| Algorithm | Source | Reference |
|---|---|---|
| `Svf`, `OnePole` | V. Zavalishin, "The Art of VA Filter Design" | Not checked |
| `Ladder` | T. Stilson and J. Smith, "Analyzing the Moog VCF", ICMC 1996; Zavalishin | Not checked |
| `DcBlocker`, `Envelope`, `Slew`, `Phasor`, `phase::wrap` | First principles | Not applicable |
| `saw`, `pulse`, `triangle` | V. Välimäki and A. Huovilainen, IEEE Signal Processing Magazine, 2007 | Not confirmed |
| `SlowWave`, `sine` | First principles | Not applicable |
| `Noise` (pink) | R. F. Voss and J. Clarke, Nature, 1975 | Not confirmed |
| `Noise` (pink) | M. Gardner, Scientific American, 1978 | Citing source |
| `Noise` (white), `random::xorshift32` | G. Marsaglia, "Xorshift RNGs", Journal of Statistical Software 8(14), 2003 | Confirmed. The text was not compared with the code: a test checks the sequence the 13, 17, 5 shifts give |
| `random::Rng` (mixing a word) | A. Appleby, MurmurHash3, the 32-bit finaliser | Confirmed against the published source code, including its constants |
| `euclid::Pattern` | G. Toussaint, "The Euclidean Algorithm Generates Traditional Musical Rhythms", BRIDGES 2005, pages 47 to 56 | Confirmed. The closed form is from first principles, and a test checks it against the paper's patterns and its even spread |
| `DelayLine` | E. Catmull and R. Rom, 1974 | Citing source |
| `Allpass` | M. R. Schroeder and B. F. Logan, JAES 9, 1961 | Confirmed |
| `Limiter` | D. Giannoulis, M. Massberg, and J. D. Reiss, JAES 60, 2012 | Confirmed |
| `Limiter` (sliding minimum) | D. Lemire, Nordic Journal of Computing, 2006 | Confirmed |
| `Dynamics` | D. Giannoulis, M. Massberg, and J. D. Reiss, JAES 60, 2012 | Confirmed. The full text was not read: the curves are tested against their formulas |
| `Dynamics` (two thresholds, hold) | First principles | Not applicable |
| `eq::Section` | R. Bristow-Johnson, "Audio EQ Cookbook", W3C Working Group Note, 2021 | Confirmed. The bell and shelf formulas were compared with the note, and a test writes them out |
| `Crossover` | S. H. Linkwitz, JAES 24, 1976 | Confirmed |
| `KWeighting`, `LevelMatch` | ITU-R BS.1770 | Partly confirmed. The high-pass and part of the shelf were seen in BS.1770-1. The rest of the table a test uses was not |
