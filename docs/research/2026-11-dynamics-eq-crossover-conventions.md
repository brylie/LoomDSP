# Conventions Review: Dynamics, EQ, and Crossover

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-03, on PR 553 at 027632a.
- **Reviews:** [Dynamics, EQ, and Crossover specification](../design/dynamics-eq-crossover.md).
- **Used by:** nothing yet. Decisions go in specs, ADRs, or the product docs.

This review asks one question of each behaviour: would an experienced DSP practitioner or a user of professional tools find it familiar, and where it differs, is there a good reason? Do not update this snapshot later; write a new one after any changes.

## Method

- Read the spec and the code in `loom-dsp` and `loom-modules`.
- Compared them with public manuals and published theory, under [ADR-0018](../adr/0018-clean-room-module-development.md). No other project's source code was read.
- Measured the bell filter through `loom_dsp::filter::eq` with sine waves, and compared the result with `analogue_gain_db`, which draws the panel's curve.

**Limits.**

- Nothing was listened to.
- The manuals show which controls exist and what they mean. They do not show how a product works inside, so a match in controls is not a match in sound.
- The full text of Giannoulis, Massberg, and Reiss could not be opened in this review, so its final recommendations were not checked here. Its formulas were checked earlier for the spec, as [DSP algorithms](../design/dsp-algorithms.md#algorithms-and-sources) records.
- The FabFilter Pro-G and Pro-Q details come from search excerpts of their manuals, not from reading each manual in full.
- Nothing official on Bitwig's Gate parameters or on numeric Q ranges was found.
- Stereo linking and the 3 dB gate hysteresis were not compared with any source.

Priority means: **High** can mislead the user about what they hear; **Medium** is a gap that experienced users will notice; **Low** is worth writing down, with no need to change.

## Summary

The core is conventional. The compressor and expander curves, the smoothing in dB, the EQ shapes, the Linkwitz-Riley crossover, and the K-weighting all follow published designs. The one result that misleads a user is the EQ curve near the top of the range, which can differ from the sound by several dB. The largest gap in features is a hold control for Gate. LoomDSP's own conventions are few, and they are stated in the spec: Match, one meaning for attack, and Duck as a mode.

## Findings, ranked

### 1. High: the EQ curve is wrong near the top of the range

The curve draws the analogue response. The filter is digital, and the bilinear transform squeezes the top of the range toward half the sample rate. So the sound and the curve agree at the bell's centre, and they move apart around it.

Measured with a bell at +12 dB and Q 1:

| Sample rate | Bell at | Measured at | Sound | Curve shows |
|---|---|---|---|---|
| 44.1 kHz | 16 kHz | 20 kHz | 1.6 dB | 9.6 dB |
| 44.1 kHz | 16 kHz | 8 kHz | 1.4 dB | 4.0 dB |
| 48 kHz | 16 kHz | 20 kHz | 3.4 dB | 9.6 dB |
| 96 kHz | 16 kHz | 20 kHz | 8.7 dB | 9.6 dB |
| 44.1 kHz | 8 kHz | 20 kHz | 0.1 dB | 2.6 dB |

The spec's rule 2 gives the reason for sharing math: a preview that shares the math cannot disagree with the sound. EQ rule 7 draws an analogue response, so here it can. This narrowing is called cramping, and it is a [known limit of the bilinear transform](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7831645).

There are two separate problems, with separate fixes:

- **The curve is untrue.** Draw the digital response instead, by evaluating the same analogue function at the warped frequency, `tan(pi f / rate) / tan(pi f0 / rate)`. This is small, but the curve function needs the sample rate.
- **The bell gets narrower.** Published fixes change the filter so that it follows the analogue shape up to half the sample rate: [Orfanidis's design, described by Vicanek](https://www.vicanek.de/articles/BiquadFits.pdf), and Vicanek's matched filters. Oversampling also helps, and it brings latency (TR-06). Any of these must keep rule 4, stability under audio-rate modulation, which is a real question for these designs.

**Recommendation:** fix the curve now. Leave the filter as it is, and record cramping in the spec and the explainer as a known limit.

### 2. Medium: Gate has no hold control

Hold is a normal gate control, and LoomDSP fixes it at 20 ms. [Cubase's Gate](https://archive.steinberg.help/cubase_plugin_reference/v9/en/_shared/topics/plug_ref/gate_r.html) offers it, [FabFilter Pro-G](https://www.fabfilter.com/downloads/pdf/help/ffprog-manual.pdf) offers it, and the [MATLAB expander](https://www.mathworks.com/help/audio/ref/expander-system-object.html) has a hold time. Cubase's [Compressor](https://www.steinberg.help/r/cubase-pro/cubaseplugref/15.0/en/_shared/topics/plug_ref/compressor_r.html) also has one, and its manual says ducking needs it: short for DJ-style ducking, and longer for music under speech. So Duck needs it as much as Gate does.

The spec already notes that a knob can be added later, and that a removed knob breaks patches once released, so adding one is the safe direction.

**Recommendation:** add `hold` for Gate and Duck, from 0 to about 2 s with a default of 20 ms. It keeps today's sound by default. Keep the hysteresis fixed.

### 3. Medium: Match is not the auto make-up that users will look for

Auto make-up is common: [Cubase's Compressor](https://www.steinberg.help/r/cubase-pro/cubaseplugref/15.0/en/_shared/topics/plug_ref/compressor_r.html), [VCV Compressor](https://vcvrack.com/Pro), and the [MATLAB limiter](https://www.mathworks.com/help/audio/ref/limiter-system-object.html) all have a form of it. It is a level setting, and in MATLAB it is fixed by the threshold and the knee. Match is different: it measures both signals over about 400 ms, and it is meant for comparing. Using it in place of make-up gain would undo slow changes of level on purpose.

A user who wants make-up gain has the Output knob, which is correct but manual.

**Recommendation:** say this in the help page now. Whether to add an automatic make-up gain is a product decision, so leave it open.

### 4. Low: "attack" and "release" do not state their measure

There is no one standard, and the numbers differ. LoomDSP's time is one time constant, `exp(-1 / (time × rate))`. The [MATLAB limiter](https://www.mathworks.com/help/audio/ref/limiter-system-object.html) measures from 10% to 90%, with `exp(-ln 9 / (time × rate))`. So a LoomDSP attack of 10 ms takes about 22 ms from 10% to 90%, and would be set near 22 ms in MATLAB.

**Recommendation:** keep the formula, and state the measure in the explainer and the parameter's description.

### 5. Low: the matcher's window is an approximation

K-weighting follows ITU-R BS.1770. The standard's measurement uses blocks and gating, and the one-pole filter with a 400 ms time constant is LoomDSP's own simplification. That suits a live comparison, which has no end to measure over. It should not be described as a BS.1770 loudness reading.

**Recommendation:** say "K-weighted, with a one-pole window" wherever this is described. Do not show it as LUFS.

### 6. Low: slopes and ranges are narrower than some tools

- **Cuts.** LoomDSP has 12 and 24 dB per octave. [FabFilter Pro-Q 4](https://www.fabfilter.com/downloads/pdf/help/ffproq4-manual.pdf) goes from 6 to 48. A gentle 6 dB option is the most useful addition.
- **Shelves** have a fixed slope. This is a reasonable simplification.
- **Gate range** ends at 96 dB, while [Cubase's](https://archive.steinberg.help/cubase_plugin_reference/v9/en/_shared/topics/plug_ref/gate_r.html) can close fully. At 96 dB this is not a practical difference.
- **Limit** has no lookahead or true-peak mode. Commercial limiters often have them, and the spec already defers them to #538 and TR-06. Say plainly in the help that Limit is not a mastering limiter.

**Recommendation:** no change now. Reconsider a 6 dB slope when users ask.

## What follows convention

| Behaviour | LoomDSP | Convention and source | Result |
|---|---|---|---|
| Compressor curve and quadratic knee | Spec's Static curves table | Giannoulis, Massberg, and Reiss; same form in [MATLAB](https://www.mathworks.com/help/audio/ref/limiter-system-object.html) | Follows |
| Expander slope and knee | Gain is `(R - 1) × (L - T)` | [MATLAB expander](https://www.mathworks.com/help/audio/ref/expander-system-object.html): output is `T + R × (L - T)`. The two are the same. | Follows. A [Pro-G excerpt](https://www.fabfilter.com/downloads/pdf/help/ffprog-manual.pdf) describes 4:1 as 4 dB of reduction per dB, which may count differently. Not confirmed. |
| Smoothing | One-pole on the gain, in dB, after the curve | [MATLAB limiter](https://www.mathworks.com/help/audio/ref/limiter-system-object.html) does the same | Follows |
| Attack and release in a gate | Attack when the signal rises | [Cubase](https://archive.steinberg.help/cubase_plugin_reference/v9/en/_shared/topics/plug_ref/gate_r.html): attack opens the gate, release closes it | Follows |
| Parameter names | Threshold, ratio, knee, attack, release, range | [Cubase](https://www.steinberg.help/r/cubase-pro/cubaseplugref/15.0/en/_shared/topics/plug_ref/compressor_r.html), [VCV Compressor](https://vcvrack.com/Pro), [Softube Weiss](https://www.softube.com/user-manuals/weiss-compressor-limiter) | Follows |
| Attack range | 0.1 to 200 ms | Cubase's Compressor: 0.1 to 100 ms | Close |
| Release range | 10 ms to 2 s | Cubase: 10 to 1000 ms | Close |
| Threshold range | -60 to 0 dB | Cubase's Gate: -60 to 0 dB | Follows |
| Limiter structure | Threshold, with Output after it | [MATLAB](https://www.mathworks.com/help/audio/ref/limiter-system-object.html) has this form. [Pro-L 2](https://www.fabfilter.com/help/pro-l/using/outputoptions) and [Cubase's Limiter](https://archive.steinberg.help/cubase_plugin_reference/v9/en/_shared/topics/plug_ref/vst_dynamics_r.html) use a drive-style form. | One of two common forms. See [the explainer](../learn/dynamics-detection.md#two-common-limiter-designs). |
| Mix before output, ceiling before mix | `mix`, then `output` | [SSL X-Limit](https://www.solidstatelogic.com/assets/uploads/downloads/Brochures/X-Limit%20User%20Guide.pdf) has its ceiling before its mix | Follows, on one example |
| Bell and shelves | Analogue responses of the Audio EQ Cookbook | Bristow-Johnson, W3C Note | Follows |
| Crossover | Linkwitz-Riley, 24 dB per octave, with all-pass alignment for three bands | Linkwitz, 1976 | Follows |
| K-weighting filter | Shelf and high-pass fitted to BS.1770 | ITU-R BS.1770 | Follows |

## LoomDSP's own conventions

These have no direct source. They are stated in the spec, and each has a reason.

- **Match** as a measured comparison inside the module.
- **Duck as a mode.** Elsewhere, ducking is often a sidechain compressor. A Duck mode says what it is for.
- **One signal path through modes.** One module holds five dynamics types, and hidden controls keep each simple. This is the reason Limit uses a threshold.
- **A `gain` output.** It suits a modular patch, where one detector can move anything.
- **Gate hysteresis of 3 dB.** Chosen from first principles, and not compared with a source.
- **Stereo linking by the larger side.** Chosen from first principles, and not compared with a source.

## Sources

- [Cubase Gate](https://archive.steinberg.help/cubase_plugin_reference/v9/en/_shared/topics/plug_ref/gate_r.html), [Compressor](https://www.steinberg.help/r/cubase-pro/cubaseplugref/15.0/en/_shared/topics/plug_ref/compressor_r.html), and [VSTDynamics](https://archive.steinberg.help/cubase_plugin_reference/v9/en/_shared/topics/plug_ref/vst_dynamics_r.html)
- [FabFilter Pro-L 2 output options](https://www.fabfilter.com/help/pro-l/using/outputoptions), [Pro-G manual](https://www.fabfilter.com/downloads/pdf/help/ffprog-manual.pdf), and [Pro-Q 4 manual](https://www.fabfilter.com/downloads/pdf/help/ffproq4-manual.pdf)
- [MATLAB limiter](https://www.mathworks.com/help/audio/ref/limiter-system-object.html) and [expander](https://www.mathworks.com/help/audio/ref/expander-system-object.html)
- [VCV Pro modules](https://vcvrack.com/Pro)
- [Softube Weiss Compressor/Limiter manual](https://www.softube.com/user-manuals/weiss-compressor-limiter)
- [SSL X-Limit user guide](https://www.solidstatelogic.com/assets/uploads/downloads/Brochures/X-Limit%20User%20Guide.pdf)
- [Bitwig dynamics devices](https://www.bitwig.com/userguide/latest/dynamic/)
- [Vicanek, "Matched Second Order Digital Filters"](https://www.vicanek.de/articles/BiquadFits.pdf), which describes Orfanidis's design
- The sources in the [spec](../design/dynamics-eq-crossover.md#sources)
