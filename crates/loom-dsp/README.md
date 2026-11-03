# loom-dsp

Pure DSP algorithms from [LoomDSP](https://github.com/brylie/LoomDSP): filters, oscillator shapes, a phase generator, noise, a seeded random stream, Euclidean rhythms, an envelope, a slew limiter, a delay line, an all-pass, and a limiter. There are no ports, no graph, and no real-time host, and the crate has no dependencies. Not published yet, and the API can change until it is.

An algorithm is a plain struct that holds state. Usually `tick` processes one sample and `reset` clears it. Coefficients are a separate small `Copy` type, so you decide how often to compute them. Audio-rate modulation computes them each sample, and a fixed filter computes them once.

```rust
use loom_dsp::filter::svf::{Coefficients, Svf};
use loom_dsp::oscillator::Saw;
use loom_dsp::phase::Phasor;

const SAMPLE_RATE: f32 = 48_000.0;

let step = Phasor::step_for(110.0, SAMPLE_RATE);
let coefficients = Coefficients::new(800.0, 0.5, SAMPLE_RATE);
let (mut phasor, mut saw, mut filter) = (Phasor::new(), Saw::new(), Svf::new());

let mut output = [0.0; 512];
for sample in &mut output {
    let wave = saw.tick(phasor.tick(step));
    *sample = filter.tick(wave, coefficients).low;
}
assert!(output.iter().all(|sample| sample.is_finite()));
```

## What you must do as a caller

An algorithm does the math and little else. These jobs are yours.

- **Stay in range.** Each method documents the input it accepts, such as a finite sample and a cutoff below half the sample rate. It does not check, so a caller reading clean files does not pay for it. A debug build asserts the range. NaN or infinity can stay in a filter's state until `reset`. Two kinds of check stay inside: one in a feedback path, and one that keeps a read inside memory.
- **Smooth your parameters.** A cutoff that jumps gives a click.
- **Handle denormals.** Very small values are slow on some CPUs. Turn on flush-to-zero on your audio thread where the CPU has it. `DcBlocker` and `Allpass` flush for themselves, because their feedback holds a value for a long time. `Svf`, `OnePole`, and `Ladder` do not, so silence after a sound can make them slow without this. WebAssembly has no such setting.
- **Allocate before you play.** `new` is the only method that allocates. To take another instance's state on an audio thread, use `copy_from`, because `clone` allocates.

## What you can expect

- **Real-time safe.** Processing never allocates, locks, waits, or does I/O. A test checks this for every algorithm.
- **Independent of block size.** There is only `tick`, so grouping samples into blocks changes nothing.
- **Repeatable within one build.** The same input gives the same bits on the same target with the same build. Across targets or compiler versions results can differ in the last bits, because Rust does not fix the precision of `tan`, `powf`, `exp`, or `sin`. Flush-to-zero changes results too.

Each module's documentation gives its sources, or says it is built from first principles. The design is in [ADR-0035](https://github.com/brylie/LoomDSP/blob/main/docs/adr/0035-pure-dsp-algorithms-in-loom-dsp.md) and the [specification](https://github.com/brylie/LoomDSP/blob/main/docs/design/dsp-algorithms.md).
