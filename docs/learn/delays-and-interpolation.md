# Delays and interpolation

How a delay line works inside a circular buffer, why moving the delay time needs cubic interpolation, why a delay-based all-pass needs at least two samples of delay, and how to tune a delay inside a feedback loop.

- **Used by:** [Delay line and all-pass spec](../design/delay.md), [Module catalogue](../product/module-catalogue.md) (`MT-DELAY-02`, `MT-DELAY-03`).
- **Code:** [`crates/loom-modules/src/time.rs`](../../crates/loom-modules/src/time.rs).

## A circular buffer moves two pointers, not the audio

A 2 second delay at 48 kHz holds 96,000 samples. Shifting 96,000 numbers by one place on every sample would copy 384 KB 48,000 times a second.

Instead, the samples stay where they are in a fixed array, and a **write index** steps forward by one slot on every frame, wrapping back to 0 at the end of the array. To read a sample from $k$ frames ago, step backward $k$ slots from the write index (wrapping around the end). Only one sample is written and a few are read on each frame.

Because the audio thread can never allocate memory (NFR-RT-01, NFR-RT-02), the array is allocated once on the control thread for the maximum delay time (`2.0 s` for `loom.time.delay`, `0.2 s` for `loom.time.allpass`) when the module is created.

## Why fractional delays happen

A delay time in seconds rarely lands on a whole number of samples:

- A chorus LFO sweeping a 10 ms delay by $\pm 2\text{ ms}$ at 48 kHz moves the read point continuously between 384 and 576 samples. On almost every frame, the read point sits between two stored samples, such as `480.37` samples ago.
- A plucked string tuned to A3 ($220\text{ Hz}$) at 48 kHz needs a period of $48\,000 / 220 = 218.1818\dots$ samples. Rounding to 218 samples plays sharp (`220.18 Hz`), and rounding to 219 plays flat (`219.18 Hz`).

## Why nearest-sample and linear interpolation fail

Suppose the buffer holds samples at integer ages $k$ and $k + 1$, and we need the value at $d = k + t$, where $t \in [0, 1)$.

- **Nearest sample (rounding $d$):** as the delay sweeps, the read point jumps from one sample to the next and clicks on every step.
- **Linear interpolation (`(1 - t) * y[k] + t * y[k + 1]`):** when $t = 0$, the output is $y_k$ with full treble. When $t = 0.5$, the output is the average of two adjacent samples, $\tfrac{1}{2}(y_k + y_{k+1})$. Averaging two adjacent samples is a two-point low-pass filter that cancels Nyquist completely and dulls the top octave.
  - **In a chorus or flanger:** as the LFO sweeps $t$ through `0.0`, `0.5`, `1.0`, the high frequencies fade out and back in on every sample boundary, making the treble flutter.
  - **In a feedback loop or all-pass:** the signal passes through the interpolation on every echo. Each pass removes more high end, so a plucked string or reverb tail turns dull much faster than intended (Laakso et al., 1996).

## 4-point Catmull-Rom cubic interpolation

To keep a smooth curve and preserve high frequencies better across fractional positions, `loom.time.delay` and `loom.time.allpass` read four neighbouring samples around the delay $d = k + t$:

| Point | Age in samples | Meaning |
|---|---|---|
| $y_{-1}$ | $k - 1$ | One sample newer than $y_0$ |
| $y_0$ | $k$ | Just before the fractional read point ($t = 0$) |
| $y_1$ | $k + 1$ | Just after the fractional read point ($t = 1$) |
| $y_2$ | $k + 2$ | One sample older than $y_1$ |

A **Catmull-Rom spline** (cubic Hermite interpolation, Catmull and Rom, 1974) fits a cubic polynomial from $y_0$ (at $t = 0$) to $y_1$ (at $t = 1$) whose slope at $t = 0$ is $\tfrac{1}{2}(y_1 - y_{-1})$ and whose slope at $t = 1$ is $\tfrac{1}{2}(y_2 - y_0)$. Because adjacent intervals share the same values and slopes at their endpoints, both the signal and its first derivative stay continuous when a modulated delay sweeps across integer sample boundaries.

## Why the delay position uses `f64`

A 32-bit float (`f32`) has 24 bits of significand. At a long delay of 2 seconds at 192 kHz, the delay in samples is `384,000`, which lies between $2^{18}$ (`262,144`) and $2^{19}$ (`524,288`). In that range, the smallest step `f32` can represent is $2^{18 - 23} = 1 / 32 = 0.03125$ samples. A slow modulation of `time` at a long delay would step in jumps of `1/32` sample instead of moving smoothly.

Computing `(base_time + time_in) * sample_rate`, clamping, and splitting $d$ into $k = \lfloor d \rfloor$ and $t = d - k$ in `f64` removes that stepping completely, while the sample buffer and the four-point polynomial stay in `f32`.

## Why the minimum delay is 1 sample for a delay line and 2 samples for an all-pass

Look at the newest of the four points in the Catmull-Rom read: $y_{-1}$ at age $k - 1$.

- **In `loom.time.delay`:** the module writes the current input sample $x[n]$ into the buffer at age $0$ *before* it reads. When $d \ge 1$, $k = \lfloor d \rfloor \ge 1$, so $k - 1 \ge 0$. At $k = 1$, the four points are ages `0, 1, 2, 3`, which are $x[n], x[n-1], x[n-2], x[n-3]$. All four exist. Below 1 sample ($0 \le d < 1$), $k - 1$ would be $-1$, meaning the future sample $x[n + 1]$, which does not exist yet. So `loom.time.delay` clamps $d$ to at least `1` sample.
- **In `loom.time.allpass`:** the buffer does not store the dry input $x[n]$; it stores the feedback state $w[n] = x[n] + g \cdot \operatorname{read}(d)$. Because $w[n]$ depends on $\operatorname{read}(d)$, age $0$ ($w[n]$) does not exist yet when $\operatorname{read}(d)$ runs! The newest sample in the buffer during the read is $w[n - 1]$ at age $1$. Clamping `loom.time.allpass` to $d \ge 2$ samples guarantees $k \ge 2$ and $k - 1 \ge 1$, so all four points ($w[n-1], w[n-2], w[n-3], w[n-4]$) are already known state values.

**A wrong turn:** writing $x[n]$ into age $0$ before the all-pass read so that $1 \le d < 2$ has a value at age $0$. For any $g \ne 0$, $x[n] \ne w[n]$, so the cubic read mixes the dry input into $y_{-1}$ and the filter is no longer an all-pass. Clamping the all-pass to 2 samples ($0.042\text{ ms}$ at 48 kHz, well below its `0.1 ms` knob minimum at standard sample rates) keeps the math exact.

## How the Schroeder all-pass keeps a flat magnitude response

A delay line with feedback alone (a feedback comb filter, $w[n] = x[n] + g \cdot w[n - D]$) boosts frequencies whose period divides the delay and cuts the frequencies halfway between them, like a row of teeth.

Manfred Schroeder (1962) added a feed-forward path with the opposite sign ($-g$) around the same delay state:

$$w[n] = x[n] + g \cdot w[n - D]$$

$$y[n] = w[n - D] - g \cdot w[n]$$

For an integer delay $D$ and a fixed gain $g \in (-1, 1)$, its transfer function is:

$$H(z) = \frac{z^{-D} - g}{1 - g z^{-D}}$$

On the unit circle $z = e^{j\omega}$, the numerator has the same magnitude as the denominator:

$$|H(e^{j\omega})| = \frac{|e^{-j\omega D} - g|}{|1 - g e^{-j\omega D}|} = \frac{|e^{-j\omega D}(1 - g e^{j\omega D})|}{|1 - g e^{-j\omega D}|} = 1$$

Every frequency comes out at the same steady-state amplitude as it went in, while different frequencies take different times to pass through. A single impulse into the all-pass produces $-g$ immediately, followed by $(1 - g^2)$ at $D$, $(1 - g^2)g$ at $2D$, $(1 - g^2)g^2$ at $3D$, and so on. Chaining several all-passes with incommensurate delay times smears a sharp click into a dense cloud of echoes without colouring the tone, which is how algorithmic reverbs build diffuse tails (Schroeder, 1962; Dattorro, 1997).

When $g = 0$, the feedback and feed-forward paths vanish ($w[n] = x[n]$ and $y[n] = w[n - D] = x[n - D]$), and the all-pass becomes a plain delay line.

## Tuning a delay line inside a feedback loop

In an analog Eurorack system, a feedback cable is a continuous circuit with no sample clock, so signals feed back immediately. In digital modular tools such as VCV Rack and Bitwig Grid, any feedback cable needs at least one sample of delay so the loop can be computed one sample at a time. In LoomDSP, `loom.time.delay1` provides that one-sample loop break (placed by the user or inserted by the compiler, [ADR-0002](../adr/0002-graph-execution-and-feedback.md)).

Because `loom.time.delay` is an ordinary DSP module whose time can change at any moment, it does not replace `loom.time.delay1`. When you patch a loop around `loom.time.delay` and a damping filter (as in `examples/patches/plucked-string.ron`), the total time a wave takes to travel once around the loop is:

$$\text{1 period} = \text{delay time} + \text{1 sample} + \text{filter delay}$$

For example, at $48\,000\text{ Hz}$, one period of A3 ($220\text{ Hz}$) is $48\,000 / 220 \approx 218.18\text{ samples}$ ($4.5455\text{ ms}$):
1. Subtract `1` sample ($0.0208\text{ ms}$) for `loom.time.delay1`, leaving $217.18\text{ samples}$.
2. Subtract the phase delay of the loop's low-pass filter (`loom.filter.svf`) at $220\text{ Hz}$. With `cutoff` at $3\,500\text{ Hz}$ and `resonance` at $0.0$, the filter delays $220\text{ Hz}$ by about $4.28\text{ samples}$, leaving $212.90\text{ samples}$ ($\approx 0.004435\text{ s}$) for the delay line's `time` knob. A darker cutoff delays the wave by more samples and lowers the pitch slightly unless the delay time is shortened to match.
