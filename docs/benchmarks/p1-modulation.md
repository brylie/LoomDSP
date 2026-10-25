# Phase 1 Modulation Benchmarks

- **Date:** 2026-10-25
- **Machine:** Apple M1 Pro (10 cores, 32 GB), macOS 27.0.1 (26A434). The maintainer's Mac, not a quiet benchmark machine.
- **Build:** `cargo bench -p loom-modules` (release, Rust 1.99)
- **Settings:** 48 kHz, 128-frame blocks. The real-time budget per block is **2,667 µs**.
- **Issue:** [#139](https://github.com/brylie/LoomDSP/issues/139). Spec: [modulation.md](../design/modulation.md#engine-and-api).

## Question

A modulated parameter gets a per-sample buffer (#133). For each sample of a modulated filter cutoff, the engine's fill adds the assignments and maps the result back to Hz with `ParamSpec::denormalize` (a `powf`). The SVF then turns Hz into pitch (`pitch::from_hz`) and back (`pitch::to_hz`). How much does this cost, and how does the fill grow with the number of assignments?

## Method

Two groups in `crates/loom-modules/benches/engine.rs`:

- **`modulation/svf_cutoff`:** noise into an SVF lowpass, with a sine LFO at about 2 Hz. The LFO runs in every case and is sent to the right output, so only the cutoff path changes.
  - **still:** the cutoff knob is not moved.
  - **cable:** the LFO is cabled to the SVF's cutoff input (pitch units).
  - **assigned:** the LFO is assigned to the cutoff parameter, depth 0.1.
- **`modulation/assignments`:** noise into an SVF lowpass, with 16 constants. The first N constants are assigned to the cutoff (N = 0, 1, 4, 16). All 16 run in every case, so only the fill changes.

The `patch` group also gained `modulation.ron`, the example patch from [#150](https://github.com/brylie/LoomDSP/issues/150).

**The numbers are noisy.** Other builds and apps ran on the machine at the same time: the load average was 5 to 7 on 10 cores during these runs, and another agent may have been building. The first of three runs was thrown away. The tables give the medians of the next two runs, each with 3 s of warm-up and 10 s of measurement. Differences under about 0.5 µs are not meaningful here.

## Results

Time per 128-frame block (median, two runs):

| Case | Run 2 | Run 3 |
|---|---:|---:|
| svf_cutoff / still | 4.05 µs | 4.01 µs |
| svf_cutoff / cable | 4.40 µs | 4.11 µs |
| svf_cutoff / assigned | 6.40 µs | 5.69 µs |
| assignments / 0 | 8.77 µs | 8.74 µs |
| assignments / 1 | 10.34 µs | 10.42 µs |
| assignments / 4 | 10.87 µs | 11.03 µs |
| assignments / 16 | 12.54 µs | 12.58 µs |

Example patches (one note playing), median of the same two runs (`fm-feedback.ron` from one run):

| Patch | Time per block | Share of real-time budget |
|---|---:|---:|
| `subtractive.ron` (9 modules, cutoff by cable) | 5.5 µs | 0.21% |
| `fm-feedback.ron` (8 modules, one 4-module loop) | 10.0 µs | 0.38% |
| `modulation.ron` (9 modules, two assignments on the cutoff) | 8.1 to 8.3 µs | 0.31% |

## What this means

1. **A modulated cutoff costs about 1.7 to 2.4 µs more per block than a still one.** That is about 13 to 18 ns per sample, or 40 to 60% more for this small patch. In absolute terms it is small: under 0.1% of the real-time budget.
2. **A cable to the cutoff input costs almost nothing.** The SVF already turns pitch into Hz and computes `tan()` every sample. The extra cost of an assignment is the fill itself: the sum, the smoothing of the base, and `denormalize`, then `from_hz` in the SVF.
3. **The first assignment costs the most.** Going from 0 to 1 adds about 1.6 µs per block. After that each assignment adds about 0.15 µs per block (1.1 ns per sample): 16 assignments cost about 2.2 µs more than 1. The fill grows linearly and slowly.
4. **`modulation.ron` costs about 2.7 µs more per block than `subtractive.ron`.** It has the same number of modules (it adds the LFO and drops the multiply that scaled the filter envelope), plus two assignments on the cutoff.

The modulated cutoff does cost noticeably more than the still one, in relative terms. It is not a P1 problem. If a patch with many modulated filters needs it, these are the places to cut, from the most to the least promising:

- **Skip the Hz round trip on logarithmic parameters.** The fill computes `min × (max / min)^t` with `powf`, and the SVF at once takes `log2` of the result with `from_hz`, then `exp2` with `to_hz`. If the fill handed the SVF a pitch (or position) buffer for its cutoff, one `powf` and one `log2` per sample would go away.
- **Make `denormalize` cheaper.** For a logarithmic parameter, `min × exp2(t × log2(max / min))` with `log2(max / min)` worked out once is cheaper than `powf`. This keeps today's API.
- **Skip the base smoothing once it has settled.** When the knob is not moving, the fill could use one base value for the whole block.

Run these benchmarks again on a quiet machine once [#383](https://github.com/brylie/LoomDSP/pull/383) is merged. It changes how modules read modulated parameters ([#138](https://github.com/brylie/LoomDSP/issues/138), `PortIo::param_at`).

## How to reproduce

```sh
cargo bench -p loom-modules --bench engine -- modulation --measurement-time 10 --warm-up-time 3
cargo bench -p loom-modules --bench engine -- '^patch/' --measurement-time 10 --warm-up-time 3
```

Close other heavy apps and builds first. Run each command twice and keep the second result.
