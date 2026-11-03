# How a dynamics processor hears a level

Why a compressor cannot just read a sample, why it works in decibels, why attack and release come after the curve, and why a gate needs two thresholds.

- **Kind:** explainer.
- **Used by:** [Dynamics, EQ, and Crossover specification](../design/dynamics-eq-crossover.md).
- **Code:** `crates/loom-dsp/src/dynamics.rs`, `crates/loom-dsp/src/loudness.rs`, wrapped by `loom.dyn.dynamics` in `crates/loom-modules/src/dynamics.rs`.
- **Sources:** D. Giannoulis, M. Massberg, and J. D. Reiss, "Digital Dynamic Range Compressor Design: A Tutorial and Analysis", JAES 60, 2012. ITU-R BS.1770.

## A sample is not a level

A compressor turns a signal down when it is loud. So it needs to know how loud the signal is. One sample does not say: a loud sine passes through 0 twice in every cycle.

There are two common answers. **Peak** takes the absolute value of each sample, and lets the release hold the gain between the peaks. It reacts at once, so it catches a drum hit. **RMS** averages the power over a short time, here 10 ms, and takes the square root. It is closer to how loud a sound seems, and it ignores a very short spike.

A sine's RMS is 3 dB under its peak. So the same threshold acts a little later with RMS.

## Why decibels

Hearing works in ratios, so the settings do too. A ratio of 4 means that above the threshold, 4 dB more in gives 1 dB more out. In decibels that is a straight line:

```text
gain = (1 / ratio - 1) × (level - threshold)
```

With a level 8 dB over the threshold and a ratio of 4, the gain is -6 dB, so the output is 2 dB over. The panel's curve draws this line. It calls the same function as the audio, so the curve cannot disagree with what you hear.

A **knee** rounds the corner where the line starts. Inside the knee, a short curve joins the two straight parts with no sudden change of slope, so the compressor eases in.

## Smoothing goes after the curve

The curve gives a target gain, and the target can jump on every sample. If the gain followed it, the gain itself would become a sound: distortion at the frequency of the signal.

So the gain moves toward its target with a time constant: attack when the signal rose, release when it fell. Each sample moves it by a fixed share of the distance left.

The order matters. Smoothing the level first and applying the curve afterward moves the effective threshold, because an average of levels is not the same as the level the curve expects. Smoothing the gain in dB after the curve keeps the threshold and the ratio where the knobs say, and it works the same way in every mode.

A slow release takes small steps. In an `f32` number near -40, a step that small rounds away, and the gain stops short of its target. So the gain is an `f64` inside. The output is still an `f32`.

## Attack means the signal rose

A compressor's gain falls when the signal rises. A gate's gain rises when the signal rises. If "attack" meant "the gain is falling", the word would swap its meaning between modes. Here, it always means the signal rose, because that is what a player is listening to.

## Why a gate needs two thresholds

A gate is a switch. If the level sits close to its threshold, a single threshold flips the switch many times a second, and it chatters. Two fixes work together:

- **Two thresholds.** It opens at the threshold, and closes only 3 dB below it. A level that wanders within those 3 dB does nothing.
- **A hold.** It closes only after the level has stayed below for 20 ms. A low note's level dips every cycle, and this keeps those dips from closing it.

Duck is the same switch the other way around.

## One gain for both sides

If each side of a stereo signal had its own compressor, a hit on the left would turn only the left down, and the sound would jump to the right. So Dynamics reads the larger side, and both sides get one gain. This is why its ports are `Stereo`: the engine's usual two copies cannot share a gain.

## A limiter with no lookahead

A limiter must never let a sample pass its ceiling. Here its attack is 0, so the gain falls on the very sample that goes over. There is no delay, so it is safe in a parallel path. The cost is a sudden change of gain, which adds some distortion on a fast peak.

Lookahead would fix that by delaying the audio, so the gain can fall before the peak arrives. [Output safety](output-safety.md) explains how the final limiter does this. That delay would comb-filter against a parallel path, until latency is reported and aligned ([#538](https://github.com/brylie/LoomDSP/issues/538)).

## Comparing fairly

Louder usually seems better, even when it is not. So compressing and then raising the level can fool you. Level match measures the input and the processed signal the same way, and turns the result to match.

It measures with the K-weighting of ITU-R BS.1770: a +4 dB shelf above about 1500 Hz, for the effect of the head, and a high-pass near 38 Hz. The power then goes through a filter with a 400 ms time constant. Being slow is the point. It removes the change of overall level and keeps the shape of each hit, so you hear what the compressor did.
