# Dynamics, EQ, and Crossover Specification

Three processors that finish a sound and shape a live vocal or instrument chain, and that show what they are doing.

- **Kind:** living specification.
- **Status:** Implemented, M2 (2026-11-03).
- **Decision behind it:** [ADR-0035](../adr/0035-pure-dsp-algorithms-in-loom-dsp.md) (algorithms in `loom-dsp`), [ADR-0018](../adr/0018-clean-room-module-development.md) (clean room), [ADR-0020](../adr/0020-inferred-channel-count.md) (stereo ports).
- **Use case and requirements:** FR-LIB-09, FR-LIB-15, FR-PANEL-02.
- **Background:** [How a dynamics processor hears a level](../learn/dynamics-detection.md).
- **Prior art:** none named. Each module is designed from its purpose and the sources under [Sources](#sources).
- **Issues:** [#541](https://github.com/brylie/LoomDSP/issues/541). Related: [#538](https://github.com/brylie/LoomDSP/issues/538) (latency), [#282](https://github.com/brylie/LoomDSP/issues/282) (read-only curve).
- **Code:** `crates/loom-dsp/src/dynamics.rs`, `loudness.rs`, `filter/eq.rs`, `filter/crossover.rs`, and `crates/loom-modules/src/dynamics.rs`, `eq.rs`, `filter.rs`. Tests: `crates/loom-modules/tests/dynamics_eq_crossover.rs`.

## Goal

> My vocal comes in through Audio In. I cut the rumble, lift the top a little, and compress it. I watch the meter to see how hard it works and the curve to see why. I switch on level match, so I judge the sound and not the loudness. Then a kick on the sidechain ducks my pad.

## Rules for all three

The [rules for M1 modules](p1-modules.md#rules-for-all-modules-here) apply.

1. **Zero latency.** None of the three delays its signal. *Why:* [#538](https://github.com/brylie/LoomDSP/issues/538) is not built, so a delayed branch would comb-filter against a parallel one.
2. **Algorithms live in `loom-dsp`,** following [DSP algorithms](dsp-algorithms.md). The curve on a panel calls the same function as the audio. *Why:* a preview that shares the math cannot disagree with the sound.
3. **A bad input is 0 for that frame,** and bad state cannot stay. The wrapper guards, as `dsp-algorithms.md` says.
4. **Every parameter can be modulated at audio rate** and stays stable. *Why:* every cable is a signal (ADR-0007), and the trapezoidal filter keeps its stability when its cutoff moves.

## Dynamics (MT-DYN-01)

`loom.dyn.dynamics`, category Dynamics, family Filters and effects.

| | |
|---|---|
| Inputs | `in` (audio, `Stereo`), `sidechain` (audio, `Downmix`, default 0) |
| Outputs | `out` (audio, `Stereo`), `gain` (modulation, mono): the gain applied, from 0 to 1, before `output` and `mix` |
| Parameters | See the table below |
| Display values | `reduction`: gain change in dB, from -36 to 0 |
| Panel | The transfer curve with the `reduction` meter beside it. A row with `mode`, `detect`, and `match`. Knobs for the rest. Controls that do nothing in the current mode are hidden ([ADR-0030](../adr/0030-panels-hide-controls-a-mode-does-not-use.md)). |

| Parameter | Range | Default | Mapping and notes |
|---|---|---|---|
| `mode` | 0 Compress, 1 Limit, 2 Expand, 3 Gate, 4 Duck | Compress | Choice |
| `threshold` | -60 to 0 dB | -18 dB | Smoothed |
| `ratio` | 1 to 20 | 4 | Logarithmic. Compress and Expand only |
| `knee` | 0 to 24 dB | 6 dB | Compress and Expand only |
| `attack` | 0.1 ms to 200 ms | 10 ms | Logarithmic. Not in Limit |
| `release` | 10 ms to 2 s | 150 ms | Logarithmic |
| `range` | 0 to 96 dB | 40 dB | The most the gain falls. Expand, Gate, and Duck only |
| `detect` | 0 Peak, 1 RMS | Peak | Choice. Not in Limit |
| `match` | 0 Off, 1 On | Off | Choice |
| `output` | -24 to 24 dB | 0 dB | Smoothed |
| `mix` | 0 to 1 | 1 | Smoothed |

### Dynamics rules

1. **One gain for both sides.** Detection reads the larger of the two sides, and both get the same gain. *Why:* the stereo image must hold still. This is why the ports are `Stereo`.
2. **A `Stereo` output stays stereo,** even from a mono input ([stereo, rule 7](stereo.md#rules)). Later modules then run as two copies.
3. **The sidechain replaces the main input for detection when a cable is connected.** It is `Downmix`, so it reads one signal, and the detector is the same. *Why:* TR-09. `io.is_connected` decides, so an unconnected input does not detect silence.
4. **Attack means the signal rose, and release means it fell, in every mode.** *Why:* in a compressor the gain falls on attack, and in a gate it rises. A user thinks about the signal, so one word keeps one meaning.
5. **Feed-forward, smoothed in dB.** The static curve maps the detected level to a target gain in dB, and the target is then smoothed: attack or release, with `exp(-1 / (time × sample_rate))`. *Why:* smoothing after the curve does not change the threshold or ratio, and it works the same in every mode (Giannoulis, Massberg, and Reiss, 2012).
6. **Detection.** Peak is the absolute value. RMS is the square root of the power through a one-pole filter with a 10 ms time constant. Levels below -120 dB read as -120 dB. *Why:* the logarithm of 0 is not finite.

### Static curves

`L` is the detected level in dB, `T` the threshold, `R` the ratio, `W` the knee width. The result is the target gain in dB.

| Mode | Target gain |
|---|---|
| Compress | Below the knee: 0. Above it: `(1 / R - 1) × (L - T)`. Inside the knee, the quadratic that joins the two with no corner. |
| Limit | `min(0, T - L)`. No knee. Attack is 0, so the gain applies on the same sample. |
| Expand | Above the knee: 0. Below it: `(R - 1) × (L - T)`, at least `-range`. Inside the knee, the quadratic that joins the two. |
| Gate | 0 while open, `-range` while closed. See rule 7. |
| Duck | `-range` while the level is above the threshold, otherwise 0. See rule 7. |

7. **Gate and Duck have two thresholds and a hold.** The state changes when the level passes the threshold going one way. It changes back only once the level is 3 dB past it the other way and has stayed there for 20 ms. *Why:* a level close to the threshold would otherwise flip the state on every cycle and chatter.
8. **Limit holds its ceiling on the detected signal.** With the main input as the detector and no sidechain, no sample leaves above the threshold before `output` and `mix`, within rounding. There is no lookahead, so a fast peak is turned down on its own sample, which adds some distortion. Lookahead waits for #538.

### Showing the effect

9. **The curve** shows the input level from -60 to 0 dB across and the output level from -60 to 0 dB up. It comes from the static curve function, and it uses the parameters with no modulation. *Why:* this is the shape preview that exists for Envelope, LFO, and Ladder. The live dot belongs to #282.
10. **The meter** shows `reduction`. It is the [meter on a display value](panel-widgets.md#meter-on-a-display-value), which this work builds. Each call to the module publishes the largest change in that call, so a short hit is seen. Inside a feedback loop a call is one frame, so there it shows the latest frame. It draws down from 0. `DisplaySpec` gains the range that spec already calls for.

### Level match

11. **`match` compares the loudness of the input and the processed signal, and turns the processed signal to match.** It runs before `output` and `mix`. *Why:* TR-10 says a louder setting must not seem better only for being louder.
12. **How it is measured.** Each signal goes through the K-weighting filter of ITU-R BS.1770, then a one-pole power filter with a 400 ms time constant. The gain is the square root of the ratio of the two powers, limited to ±24 dB. Both sides are summed in power, as the standard does.
13. **It holds still in silence.** While the input reads below -70 dB, the gain does not change. *Why:* the ratio means nothing there, and a jump at the next note would be heard.
14. **It is for comparing, not for mastering.** It is slow on purpose, and it undoes some of what a compressor does to long-term level. It is off by default.

### Mix and output

15. **`mix` is a linear blend of dry and processed.** *Why:* the two are in phase, because there is no latency. An equal-power law would raise the level in the middle by 3 dB.
16. **`output` comes after the blend.**

## EQ (MT-EQ-01)

`loom.eq.eq`, category EQ, family Filters and effects.

| | |
|---|---|
| Inputs | `in` (audio, `PerChannel`) |
| Outputs | `out` (audio, `PerChannel`) |
| Panel | Response curve at full width, then the two slopes, then columns of knobs, one for each section |

The signal passes through five sections in this order.

| Section | What it does | Parameters | Range | Default |
|---|---|---|---|---|
| Low cut | Removes everything below its frequency. For rumble. | `low_cut`, `low_slope` | 10 Hz to 1 kHz. 0 Off, 1 12, 2 24, in dB per octave | 40 Hz, Off |
| Low shelf | Raises or lowers everything below its frequency by the same amount. | `low_freq`, `low_gain` | 20 Hz to 2 kHz. -24 to 24 dB | 120 Hz, 0 dB |
| Mid band | A bell: a bump or dip centred on its frequency. Q sets how narrow it is. | `mid_freq`, `mid_gain`, `mid_q` | 40 Hz to 16 kHz. -24 to 24 dB. Q 0.1 to 10 | 1 kHz, 0 dB, 1 |
| High shelf | Raises or lowers everything above its frequency by the same amount. | `high_freq`, `high_gain` | 1 kHz to 20 kHz. -24 to 24 dB | 8 kHz, 0 dB |
| High cut | Removes everything above its frequency. For hiss and harshness. | `high_cut`, `high_slope` | 1 kHz to 22 kHz. Same choices as low cut | 16 kHz, Off |

### EQ rules

1. **Runs per channel.** Stereo runs two copies, and mono stays mono. *Why:* a filter has no reason to link its sides, unlike a level detector.
2. **Five fixed sections, with one mid band.** A second mid band is a second EQ. *Why:* parameters are static in a descriptor, and the Mixer uses the same reasoning: a bigger one is two in a row.
3. **All built on the state variable filter.** The mid band and shelves are mixes of its outputs, and the cuts are one or two of them in series. *Why:* it stays stable under audio-rate modulation, and a coefficient table of direct-form filters does not promise that.
4. **Flat means untouched.** With every gain at 0 and both cuts Off, the output equals the input exactly. *Why:* a user who bypasses by zeroing knobs must not get a changed signal.
5. **The mid band's gain is the gain at its centre.** Shelves reach half their gain in dB at their set frequency. *Why:* it is what the knob's name says.
6. **A cut switched On or Off crossfades over 5 ms,** with its filters always running. *Why:* no click, and the filter is already settled.
7. **The curve** shows 20 Hz to 20 kHz across on a logarithmic scale, and -24 to 24 dB up. It computes the analogue response of the same five sections.

## Crossover (MT-FILTER-07)

`loom.filter.crossover`, category Filter, family Filters and effects.

| | |
|---|---|
| Inputs | `in` (audio, `PerChannel`) |
| Outputs | `low`, `mid`, `high` (audio, `PerChannel`) |
| Parameters | `bands`: 0 Two, 1 Three (default). `low_split`: 20 Hz to 16 kHz, default 200 Hz. `high_split`: 40 Hz to 20 kHz, default 2 kHz. Both logarithmic. |
| Panel | `bands` control, then one knob each for `low_split` and `high_split`. `high_split` is hidden with two bands. |

### Crossover rules

1. **Linkwitz-Riley, 24 dB per octave.** Each split is two second-order Butterworth low-passes in series, and two high-passes in series. *Why:* the bands are steep enough for multiband dynamics (MT-DYN-03), and they sum to a flat level.
2. **The bands sum to a flat level, not the same waveform.** The sum has the same level as the input at every frequency, within 0.01 dB, and its phase is rotated. It does not null against the dry signal. *Why:* a steep crossover cannot rebuild the exact waveform with no latency. This changes the catalogue's wording: "sum back to the original" becomes "sum to a flat level".
3. **Three bands stay aligned.** The low band also passes through an all-pass at `high_split`, the sum of that split's low and high parts. *Why:* the mid and high bands have been through that split, so without this the bands would sum with a dip.
4. **The splits stay in order.** While `high_split` is below `low_split`, the module uses `low_split` for both. *Why:* a modulated split must not swap the bands. The mid band is then a narrow band around that split, and the sum stays flat.
5. **With two bands,** only `low_split` acts: `low` and `high` carry the bands, and `mid` is silent. A change of `bands` crossfades over 5 ms. *Why:* no click. The level during the crossfade is not held flat, as it lasts only 5 ms.
6. **All filters always run.** *Why:* a change finds them already settled.

## Acceptance criteria

- [x] All three pass the harness at block sizes 1, 77, and 512, with reference renders.
- [x] Each algorithm has direct tests against theory, and is added to `no_alloc.rs` and `static_checks.rs`.
- [x] Given Compress at any tested threshold and ratio and a steady tone, the settled output level matches the static curve within 0.1 dB.
- [x] Given Limit and a signal far above the threshold, no output sample passes the threshold, at any block size.
- [x] Given a signal that sits at the threshold of Gate or Duck, the state does not chatter.
- [x] Given a stereo input with a hit on one side only, the two sides get the same gain.
- [x] Given a cable on `sidechain`, the gain follows it and not `in`.
- [x] The curve's function and the audio agree: the settled gain equals the curve within 0.1 dB, across the modes.
- [x] With `match` on, a compressed steady signal settles within 0.5 dB of the input's weighted loudness, and the filter matches the BS.1770 response.
- [x] Given EQ with every gain at 0 and both cuts Off, the output equals the input bit for bit.
- [x] Each EQ section's measured response matches its analogue response within 0.1 dB, away from the top of the range where warping applies.
- [x] The Crossover's sum is flat within 0.01 dB from 20 Hz to 20 kHz, with two and with three bands.
- [x] Each Crossover band falls at 24 dB per octave well past its split.
- [x] `docs/learn/` has an explainer on dynamics detection.
- [x] A gain reduction meter works on a display value, in the app, and in `--dump-ui`.

Manual, done: `--screenshot-window`, `--screenshot`, and `--dump-ui --frames` on a patch that chains all three, in Compress and Limit. The curves follow their settings, hidden controls close up, and the meter text read `Reduction: -22.0 dB`. Not done: listening, and driving it on a live audio device.

## Out of scope

- **Lookahead,** which waits for [#538](https://github.com/brylie/LoomDSP/issues/538).
- **Level match on EQ.** The catalogue gives the Mix and output trait to Dynamics only, and two copies of a matcher would each move their own side and shift the image.
- **EQ presets.** The catalogue names a one-knob tilt and a three-band cut. Modules have no preset system, so these wait for one.
- **Multiband dynamics** (MT-DYN-03), an abstraction for M4.
- **A live dot on the curve,** which is [#282](https://github.com/brylie/LoomDSP/issues/282).
- **A Range widget for the two splits.** Two knobs until the [range](panel-widgets.md#range) widget exists.
- **Oversampling** (TR-06). Limit's distortion could use it, and it brings latency, so it waits as well.

## Sources

What was checked for each is in [DSP algorithms](dsp-algorithms.md#algorithms-and-sources).

| Subject | Source |
|---|---|
| Static curves, smoothing in dB | D. Giannoulis, M. Massberg, and J. D. Reiss, "Digital Dynamic Range Compressor Design: A Tutorial and Analysis", JAES 60, 2012 |
| Bell and shelf responses | R. Bristow-Johnson, "Audio EQ Cookbook", W3C Working Group Note, 2021 |
| State variable mixes | V. Zavalishin, "The Art of VA Filter Design" |
| Crossover | S. H. Linkwitz, "Active Crossover Networks for Noncoincident Drivers", JAES 24, 1976 |
| Loudness | ITU-R BS.1770, "Algorithms to measure audio programme loudness and true-peak audio level" |

## Settled in review

Agreed with the maintainer on 2026-11-03.

- **Stereo:** Dynamics is linked, with `Stereo` ports.
- **Crossover:** Linkwitz-Riley. The bands sum to a flat level and do not null against the dry signal.
- **Lookahead:** none now, so no dependency on #538.
- **The `gain` output on Dynamics:** included. One detector can drive anything else in a patch, which suits a modular tool, and it costs one buffer.
- **EQ sections:** five fixed sections with one mid band, as a second EQ adds another. "Peak" in the catalogue is called the mid band, or a bell, here: the word also means a signal's highest level, as Dynamics uses it.
- **Gate and Duck timing:** 3 dB and 20 ms are fixed. A knob can be added later, and a removed knob breaks patches once released.
- **Level match:** K-weighted. It costs more than plain RMS, but it is a published standard, and plain RMS misjudges a change in the low end.
