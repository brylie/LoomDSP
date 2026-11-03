# How a dynamics processor hears a level

Why a compressor cannot just read a sample, why it works in decibels, why attack and release come after the curve, why a gate needs two thresholds, and what a limiter's ceiling does and does not promise.

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

A limiter turns down any sample that would pass its ceiling. Here its attack is 0, so the gain falls on the very sample that goes over. There is no delay, so it is safe in a parallel path. The cost is a sudden change of gain, which adds some distortion on a fast peak.

Lookahead would fix that by delaying the audio, so the gain can fall before the peak arrives. [Output safety](output-safety.md) explains how the final limiter does this. That delay would comb-filter against a parallel path, until latency is reported and aligned ([#538](https://github.com/brylie/LoomDSP/issues/538)).

## Comparing fairly

Louder usually seems better, even when it is not. So compressing and then raising the level can fool you. Level match measures the input and the processed signal the same way, and turns the result to match.

It measures with the K-weighting of ITU-R BS.1770: a +4 dB shelf above about 1500 Hz, for the effect of the head, and a high-pass near 38 Hz. The power then goes through a filter with a 400 ms time constant. Being slow is the point. It removes the change of overall level and keeps the shape of each hit, so you hear what the compressor did.

## What a limiter's ceiling promises

Limit can have Match on, and Match runs after the limit. So the signal can leave above the ceiling. Take a steady sine with peaks at 0 dB and a ceiling at -12 dB:

| Stage | Peak |
|---|---|
| Input | 0 dB |
| After the limit | about -12 dB |
| After Match, once settled | about 0 dB |

This looks like a bug in a safety feature. It is not, because Limit is not the safety feature. The promise of any ceiling ends where the next gain begins, and in a patch something can always come next. A hardware limiter works the same way: its ceiling holds at its output jack, and the next amplifier can raise the signal again.

Only the [output stage](output-safety.md) can protect speakers and ears, because nothing but the final clip follows it. It holds every sample at -1 dBFS in the Low latency and Clean modes, whatever Dynamics does. In Off mode, the user has chosen to give that limiter up. Dynamics also sets the level of the signal, so its ceiling is a choice about sound.

Limit's ceiling is still useful. A following effect can rely on it when Match is off, Output is at 0 dB, Mix is fully processed, and there is no sidechain. A sidechain breaks the promise too, because the limiter then listens to a different signal.

### Two common limiter designs

Both of these are normal in professional and research tools. They limit in the same way, and they differ in where the level is set, so level matching moves in opposite directions.

| | Drive into a fixed ceiling | Threshold, with make-up gain after it |
|---|---|---|
| How you limit harder | Raise the input gain | Lower the threshold |
| Result compared with the input | Usually louder | Quieter |
| To compare at equal level | Turn the output down | Turn the output up |
| Can the result pass the setting? | No, when the output is turned down | Yes, by the make-up gain |
| Examples | [FabFilter Pro-L 2](https://www.fabfilter.com/help/pro-l/using/outputoptions), whose Unity Gain button sets the output to the inverse of the gain. Bitwig's Peak Limiter, as a [community guide](https://polarity.me/posts/bitwig-guides/2023-04-03-peak-limiter-bitwig-audio-fx-guide/) describes it. | The [MATLAB Audio Toolbox limiter](https://www.mathworks.com/help/audio/ref/limiter.html), which cites the same Giannoulis paper as this module. [Beutl](https://docs.beutl.beditor.net/reference/library/audio-effects/limiter), whose documentation gives the final peak as threshold plus make-up gain. |

The two are the same limiter at different levels. A threshold of -12 dB with +12 dB of output limits like +12 dB of drive into a ceiling at 0 dB.

Dynamics uses a threshold, because all of its modes share one curve and one threshold. So Match has to turn a limited signal up. Match is not quite make-up gain, though. The MATLAB limiter's automatic make-up gain is fixed by the threshold and the knee, so a steady 0 dB input gives a 0 dB output. Match measures the loudness of both signals and follows them.

### Five ways to make the ceiling hold, and what each costs

Each of these looks like an obvious fix.

**Disable Match in Limit.** The promise gets one condition shorter, but it still fails. Output comes later and can add 24 dB, and Mix can bring back dry peaks that the limit removed. It also removes the comparison from the mode that needs it most. A limiter can make a mix seem better just because it allows more level, and what it costs in distortion and lost punch is easier to hear at equal loudness.

**Limit after Match.** The ceiling now holds at that point, but the two stages fight. Match raises the level to make up for the limiting, so the limiter turns it down harder, so Match reads a quieter signal and raises it again. Match can climb to its own limit, and the comparison stops being fair.

**Keep Match's gain below what would pass the ceiling.** A limited signal already sits at the ceiling, so Match could never raise it. This is Match switched off without saying so, and a control that silently does nothing teaches the wrong thing.

**Put the ceiling after Output and Mix.** The module's output now really is limited, but the Output knob stops raising a limited signal, and the dry path in Mix gets limited too. This is a different tool: a final limiter for a chain. The next module can still raise the level, so it would not make a patch safe either.

**Use a fixed ceiling and a drive knob in Limit.** Matching then turns the signal down, and the ceiling holds with Match on, as in the first design above. Output above 0 dB and Mix can still pass it. Limit would also stop sharing the threshold and the curve with the other modes, and moving between modes would change what the main knob means. An Amplifier before a threshold limiter already gives the same result in a patch.

So Match stays available in Limit, and the rule is plain: when a ceiling must hold, leave Match off. When you want to protect your ears, leave output safety on.
