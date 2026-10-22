# Delay Line and All-Pass Specification

Two time-domain building blocks (`MT-DELAY-02` and `MT-DELAY-03`) for echoes, chorus, flanging, reverbs, and plucked-string models.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (memory allocated on the control thread), [ADR-0002](../adr/0002-graph-execution-and-feedback.md) (feedback loops and one-sample delays), [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md) (primitives and traits), [ADR-0018](../adr/0018-clean-room-module-development.md) (clean-room development), [ADR-0020](../adr/0020-inferred-channel-count.md) (stereo channel rules).
- **Use case and requirements:** UC1, UC2, FR-LIB-06, FR-SIG-07, NFR-RT-01, NFR-RT-02, NFR-SND-02.
- **Background:** [Delays and interpolation](../learn/delays-and-interpolation.md): how a circular buffer works, why linear interpolation loses high frequencies, why the all-pass needs at least two samples of delay, and how to tune a delay loop.
- **Prior art:** hardware and modular delay lines with a voltage-controlled time input, and Manfred Schroeder's 1962 reverberation all-pass diffuser. Described in our own words and names.
- **Issues:** [#40](https://github.com/brylie/LoomDSP/issues/40)
- **Code:** `crates/loom-modules/src/time.rs`, `crates/loom-modules/tests/reference.rs`, `crates/loom-modules/tests/no_alloc.rs`, `crates/loom-modules/tests/engine.rs`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"I want a clean delay line whose time I can sweep smoothly by a few milliseconds with an LFO for chorus, or patch into a feedback loop with a filter to build an echo or a plucked string. And I want a delay-based all-pass so I can diffuse echoes into a reverb."

`MT-DELAY-02` and `MT-DELAY-03` are primitives. Each does one job well, so later effects (`MT-DELAY-04` Delay, `MT-DELAY-05` Modulated delay, `MT-FX-01` Reverb) and user patches can be built from them.

## Behaviour

### Delay Line (`loom.time.delay`, `MT-DELAY-02`)

- **Name:** `Delay Line`
- **Category:** `Time`
- **Description:** `"Interpolated delay line. The time input adds seconds to the time knob."`
- **Panel:** `3 × 3` grid units, with one knob for `time` at `(0, 0)`.

| Port | Direction | Label | Signal type | Polarity | Channels | Default | Description |
|---|---|---|---|---|---|---|---|
| `in` | Input | `In` | `Audio` | `Bipolar` | `PerChannel` | `0.0` | `"The signal to delay."` |
| `time` | Input | `Time` | `Modulation` | `Bipolar` | `Downmix` | `0.0` | `"Adds to the delay time, in seconds."` |
| `out` | Output | `Out` | `Audio` | `Bipolar` | `PerChannel` | `0.0` | `"The delayed signal."` |

| Parameter | Label | Range | Default | Unit | Mapping | Smoothing | Description |
|---|---|---|---|---|---|---|---|
| `time` | `Time` | `0.0001` to `2.0` | `0.2` | `"s"` | Logarithmic | `0.01` s | `"How long the signal is delayed."` |

### All-Pass (`loom.time.allpass`, `MT-DELAY-03`)

- **Name:** `All-Pass`
- **Category:** `Time`
- **Description:** `"Delay-line all-pass filter. Passes all frequencies at equal steady-state level while smearing phase, for reverbs and diffusers."`
- **Panel:** `6 × 3` grid units, with knobs for `time` at `(0, 0)` and `gain` at `(3, 0)`.

| Port | Direction | Label | Signal type | Polarity | Channels | Default | Description |
|---|---|---|---|---|---|---|---|
| `in` | Input | `In` | `Audio` | `Bipolar` | `PerChannel` | `0.0` | `"The signal to pass through the filter."` |
| `time` | Input | `Time` | `Modulation` | `Bipolar` | `Downmix` | `0.0` | `"Adds to the delay time, in seconds."` |
| `out` | Output | `Out` | `Audio` | `Bipolar` | `PerChannel` | `0.0` | `"The all-pass filtered signal."` |

| Parameter | Label | Range | Default | Unit | Mapping | Smoothing | Description |
|---|---|---|---|---|---|---|---|
| `time` | `Time` | `0.0001` to `0.2` | `0.01` | `"s"` | Logarithmic | `0.01` s | `"Delay time of the all-pass loop."` |
| `gain` | `Gain` | `-0.95` to `0.95` | `0.5` | `""` | Linear | `0.005` s | `"Feedback and feed-forward coefficient. 0 is a plain delay."` |

### Equations and order of operations

Both modules share one circular buffer and one 4-point Catmull-Rom (cubic Hermite) reader. Each frame $n$ runs these steps in order:

1. **Base parameter values.** The `time` knob is smoothed in its normalised 0 to 1 position over 10 ms (`ParamSpec::normalize` and `ParamSpec::denormalize`), matching the engine's `ParamFill` glide when an assignment is added or removed. On the all-pass, `gain` is smoothed linearly over 5 ms and clamped to $[-0.95, 0.95]$.

2. **Effective delay in samples (`f64`).** Let $t_{\text{knob}}[n]$ be the smoothed (or modulated) `time` parameter in seconds, $t_{\text{in}}[n]$ the `time` input sample in seconds, and $f_s$ the sample rate in Hz. Compute the delay in `f64`:

   $$d_{\text{raw}} = \big(t_{\text{knob}}[n] + t_{\text{in}}[n]\big) \cdot f_s$$

   $$d = \begin{cases} d_{\min} & \text{if } d_{\text{raw}} \text{ is NaN} \\ \operatorname{clamp}(d_{\text{raw}},\; d_{\min},\; N_{\max}) & \text{otherwise} \end{cases}$$

   where $N_{\max} = \lceil t_{\max} \cdot f_s \rceil$, and the minimum delay is:
   - $d_{\min} = 1$ sample for `loom.time.delay`,
   - $d_{\min} = 2$ samples for `loom.time.allpass`.

   Split $d$ in `f64` into an integer sample offset and a fractional part in `f32`:

   $$k = \lfloor d \rfloor \in \mathbb{Z}, \qquad t = d - k \in [0, 1)$$

3. **4-point Catmull-Rom cubic read.** Let $b(m)$ be the sample in the circular buffer at age $m$ samples behind the current write index ($m = 0$ is the current write slot, $m = 1$ is the sample written on frame $n - 1$, and so on). Read the four points surrounding $d$:

   $$y_{-1} = b(k - 1), \quad y_0 = b(k), \quad y_1 = b(k + 1), \quad y_2 = b(k + 2)$$

   Evaluate the Catmull-Rom cubic polynomial in Horner form in `f32`:

   $$c_0 = y_0$$

   $$c_1 = \tfrac{1}{2}(y_1 - y_{-1})$$

   $$c_2 = y_{-1} - \tfrac{5}{2}y_0 + 2y_1 - \tfrac{1}{2}y_2$$

   $$c_3 = \tfrac{1}{2}(y_2 - y_{-1}) + \tfrac{3}{2}(y_0 - y_1)$$

   $$\operatorname{read}(d) = ((c_3 \cdot t + c_2) \cdot t + c_1) \cdot t + c_0$$

4. **Module step:**
   - **Delay Line (`loom.time.delay`):** write the incoming sample $x[n]$ at age $0$ first, then read at $d \ge 1$, then advance the write index:

     $$b(0) \leftarrow x[n], \qquad y[n] = \operatorname{read}(d)$$

   - **All-Pass (`loom.time.allpass`):** read $r[n] = \operatorname{read}(d)$ first (since $d \ge 2$, $k - 1 \ge 1$, so all four points $b(k - 1), \dots, b(k + 2)$ are past state samples $w[n - 1]$ or older), flush denormals on the feedback product, compute the new state $w[n]$ and output $y[n]$, write $w[n]$ at age $0$, and advance the write index:

     $$r[n] = \operatorname{read}(d)$$

     $$f[n] = \begin{cases} 0 & \text{if } |g \cdot r[n]| < 10^{-20} \\ g \cdot r[n] & \text{otherwise} \end{cases}$$

     $$w[n] = x[n] + f[n]$$

     $$y[n] = r[n] - g \cdot w[n]$$

     $$b(0) \leftarrow w[n]$$

### Worked example: a plucked string with a delay line in a loop

When you patch `loom.time.delay` in a feedback loop with cables, the compiler runs the loop one sample at a time and requires a one-sample delay (`loom.time.delay1`) in the cycle (ADR-0002). If you do not place one, the compiler inserts one automatically.

Suppose you build a Karplus-Strong plucked string (`examples/patches/plucked-string.ron`):
1. A short noise burst from `loom.osc.noise` shaped by `loom.env.adsr` excites `loom.math.add`.
2. The sum feeds `loom.time.delay`, then a low-pass filter (`loom.filter.svf`) to soften each echo, a `loom.math.mul` with `gain` just below `1.0` (for example `0.985`), and `loom.time.delay1` back into `loom.math.add`.

The total delay around the loop is the sum of three parts:

$$\text{loop period} = t_{\text{delay}} + \frac{1}{f_s} + \tau_{\text{filter}}(f_0)$$

- $t_{\text{delay}}$ is the delay line's `time` setting.
- $1 / f_s$ is the extra sample from `loom.time.delay1`.
- $\tau_{\text{filter}}(f_0)$ is the phase delay of the loop's low-pass filter at the fundamental frequency $f_0$.

Ignoring the filter, a string tuned to A3 ($f_0 = 220\text{ Hz}$) at $f_s = 48\,000\text{ Hz}$ has a period of $48\,000 / 220 \approx 218.18\text{ samples}$ ($4.5455\text{ ms}$). Subtracting $1\text{ sample}$ for `delay1` leaves $217.18\text{ samples}$, and subtracting the phase delay of `loom.filter.svf` at $220\text{ Hz}$ (about $4.28\text{ samples}$ when `cutoff` is $3\,500\text{ Hz}$ and `resonance` is $0.0$) leaves $212.90\text{ samples}$ ($\approx 0.004435\text{ s}$).

## Rules

1. **A `time` input in seconds in addition to the `time` knob.** The effective delay time is `knob + input`. *Why:* parameter modulation without cables (ADR-0014) scales depth across the knob's full logarithmic span (`0.1 ms` to `2.0 s`, over 14 octaves). Chorus, flanging, and vibrato need a small, exact swing such as $\pm 2\text{ ms}$ around any knob setting. A `time` port in seconds lets an LFO scaled by `0.002` add $\pm 2\text{ ms}$ anywhere on the dial.

2. **Delay memory is allocated once in the module factory, sized from `ParamSpec::max`.** Each buffer holds $\lceil t_{\max} \cdot f_s \rceil + 4$ floats (`~384 KB` mono for the 2 s delay line at 48 kHz, `~38 KB` mono for the 0.2 s all-pass). `ModuleConfig` is unchanged. When the sample rate changes, the host rebuilds the engine with the new `ModuleConfig`, so new buffers are allocated on the control thread. *Why:* hard rule 1 (`NFR-RT-01`, `NFR-RT-02`). Tying the buffer size to `ParamSpec::max` guarantees the knob and the buffer can never disagree.

3. **With a stereo input, the engine runs two copies (`PerChannel`), while `time` is downmixed (`Downmix`).** *Why:* stereo audio needs independent left and right delay buffers (`~768 KB` total for a stereo 2 s delay line at 48 kHz), while a single delay time keeps both channels at the same delay unless the user splits them on purpose ([stereo spec](stereo.md)).

4. **Catmull-Rom 4-point cubic interpolation, always on in P1.** *Why:* linear interpolation attenuates high frequencies whenever the delay sits between samples, which makes treble flutter during chorus sweeps and turns feedback loops dull (Laakso et al., 1996). Catmull-Rom (cubic Hermite) has continuous tangents ($C^1$) across sample boundaries, so sweeps stay smooth. A `Quality` selector (`TR-06`) waits until a library-wide pattern is chosen for all modules with `TR-06`.

5. **Read position, clamping, and splitting into $k$ and $t$ use `f64`; sample storage and interpolation math use `f32`.** *Why:* at `2.0 s` and `192 kHz`, the buffer index reaches `384,000`, where `f32` has a step size of `1/32` of a sample (`0.03125`). Computing the delay in `f64` keeps fractional steps smooth at every delay length and sample rate, while keeping the buffer in `f32` halves memory bandwidth.

6. **Minimum delay is 1 sample for `loom.time.delay` and 2 samples for `loom.time.allpass`.** Non-finite `NaN` on the `time` input maps to the minimum delay before clamping. *Why:* a 4-point cubic read centered between $k$ and $k + 1$ needs one newer sample at $k - 1$. In `loom.time.delay`, $x[n]$ is written at age $0$ before the read, so $k \ge 1$ has $k - 1 \ge 0$. In `loom.time.allpass`, age $0$ will hold $w[n] = x[n] + g \cdot \operatorname{read}(d)$, which is not known until after the read; clamping to $d \ge 2$ ($k \ge 2$) guarantees $k - 1 \ge 1$, so all four points are already known state values $w[n - 1], w[n - 2], \dots$ and the filter remains a true all-pass.

7. **Both modules are ordinary `ModuleRole::Dsp` modules.** External feedback loops made with cables must still pass through `loom.time.delay1` (placed by the user or inserted by the compiler), which adds 1 sample to the loop period. The all-pass's internal feedback stays inside the module and does not form a graph cycle. *Why:* only a fixed 1-sample tap managed by the engine can break a cycle without making the schedule depend on block size or parameter values (ADR-0002, hard rule 2).

8. **Turning the `time` knob glides the read head over 10 ms and bends pitch.** *Why:* sweeping the read position changes the rate at which stored samples are read, just like changing tape speed. Choice of time-change behaviour (pitch glide, crossfade, or jump) belongs to the P2 finished delay (`MT-DELAY-04`).

9. **With `gain = 0`, `loom.time.allpass` is a plain delay and is bit-identical to `loom.time.delay` for the same delay in samples ($d \ge 2$).** Denormals in the all-pass are flushed to `0.0` only on the feedback product $g \cdot r[n]$ when $|g \cdot r[n]| < 10^{-20}$ (matching `safety.rs`). *Why:* flushing $w[n]$ itself would zero out quiet input samples even when $g = 0$. Flushing only $g \cdot r[n]$ stops idle reverb tails from decaying into denormals while leaving $g = 0$ exact. (Because the two `time` knobs have different maximums, setting the same number of seconds on both knobs can round by 1 ULP in normalised position; the bit-identical guarantee is for the same delay $d$ in samples.)

10. **Modulating the all-pass `time` colours the magnitude response slightly.** *Why:* a time-varying delay inside a feedback loop shifts frequencies and interpolates between samples, so the transfer function is strictly flat only when `time` is held still.

## Acceptance criteria

- [x] Both modules register in `loom_modules::ALL`, pass `panels_follow_the_spec` and `parameters_in_hz_and_seconds_are_logarithmic`, and build for `wasm32-unknown-unknown`. (test)
- [x] Given an impulse into `loom.time.delay` at an integer delay of $N$ samples, the output is `1.0` at sample $N$ and `0.0` everywhere else. (unit test)
- [x] Given a ramp or sine into `loom.time.delay` at a fractional delay, the output matches the 4-point Catmull-Rom formula, and a slow sweep near 2.0 s at 192 kHz changes smoothly without `f32` stepping. (unit test)
- [x] Given negative values, sub-sample values, `NaN`, `+inf`, and `-inf` on the `time` input, `loom.time.delay` clamps to $[1, N_{\max}]$, `loom.time.allpass` clamps to $[2, N_{\max}]$, and every output sample is finite. (unit test)
- [x] Given `gain = 0` and the same delay in samples ($d \ge 2$), `loom.time.allpass` produces output equal (`==`) to `loom.time.delay`, including for quiet inputs down to $10^{-25}$. (unit test)
- [x] Given a steady-state sine wave at several frequencies into `loom.time.allpass` with an integer sample delay and `gain = 0.6`, after settling for several hundred echo periods, the output RMS matches the input RMS within `1e-4` relative error. (unit test)
- [x] Calling `reset()` on either module clears all buffered echoes to `0.0` and settles its parameter smoothers. (unit test)
- [x] Both modules have reference tests in `crates/loom-modules/tests/reference.rs` with audio-rate `time` modulation ($\pm 2\text{ ms}$) that pass block-size invariance across block sizes 1, 77, and 512. (test)
- [x] `crates/loom-modules/tests/no_alloc.rs` runs both modules while their `time` and `gain` parameters and `time` input change during playback, with zero allocations on the audio thread. (test)
- [x] Given `loom.time.delay` patched in a cable feedback loop without `loom.time.delay1`, the compiler reports 1 inserted delay, the echo period is `delay_samples + 1`, and the output is bit-identical at host buffer sizes 1, 32, 77, and 1024. (engine test)
- [x] Both modules declare `in: PerChannel`, `time: Downmix`, and `out: PerChannel`, and independent left and right delay and all-pass paths through the engine process each channel without crosstalk. (engine test)
- [x] `examples/patches/plucked-string.ron` passes `loom-cli check` with no compiler-inserted delays and renders a plucked note with `loom-cli render`. (CLI check)

## Out of scope

- **A frequency-based all-pass filter (`MT-FILTER-12`)** tuned by pitch for building phasers (`MT-FX-02`). Added to the module catalogue as a separate P2 entry.
- **A `Quality` selector parameter (`TR-06`).** Waits for a library-wide design for the `Quality` trait across oscillators, delays, and samplers.
- **A pitch input (`SignalType::Pitch`) for tuned delay lines.** Planned for string models and comb filters (`MT-FILTER-05`, `MT-SRC-10`).
- **Multi-tap, feedback, filtering, ping-pong, tempo sync, and time-change modes (`MT-DELAY-04`).** Planned for P2.
- **Patch-wide memory limits (`NFR-SEC-02`).** A per-module cap does not stop a patch from adding many delays; a patch-level memory budget belongs in its own specification and issue.
- **`Module::copy_state_from` (`ADR-0022`, [#229](https://github.com/brylie/LoomDSP/issues/229)).** Tracked in #229 and #23 alongside stereo copies.

## Open points

None.

## References

1. M. R. Schroeder, "Natural Sounding Artificial Reverberation", *Journal of the Audio Engineering Society*, vol. 10, no. 3, pp. 219-223, 1962.
2. T. I. Laakso, V. Valimaki, M. Karjalainen, and U. K. Laine, "Splitting the Unit Delay: Tools for Fractional Delay Filter Design", *IEEE Signal Processing Magazine*, vol. 13, no. 1, pp. 30-60, Jan. 1996.
3. E. Catmull and R. Rom, "A Class of Local Interpolating Splines", in *Computer Aided Geometric Design*, R. E. Barnhill and R. F. Riesenfeld, Eds. New York: Academic Press, 1974, pp. 317-326.
4. J. O. Smith III, *Physical Audio Signal Processing*, Center for Computer Research in Music and Acoustics (CCRMA), Stanford University, https://ccrma.stanford.edu/~jos/pasp/.
5. J. O. Smith III, *Introduction to Digital Filters with Audio Applications*, CCRMA, Stanford University, https://ccrma.stanford.edu/~jos/filters/.
6. J. Dattorro, "Effect Design, Part 1: Reverberator and Other Filters", *Journal of the Audio Engineering Society*, vol. 45, no. 9, pp. 660-684, Sept. 1997.
