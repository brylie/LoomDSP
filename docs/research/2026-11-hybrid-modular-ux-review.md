# UX Review: LoomDSP with a Modular Synthesizer

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at e59132e (after #515).
- **Follows:** the persona reviews of the same day ([producer](2026-11-producer-ux-review.md), [generative](2026-11-generative-ux-review.md), [learner](2026-11-learner-ux-review.md), [module developer](2026-11-module-developer-ux-review.md)).
- **Used by:** issues #520 and #521, and comments on #77 and #506. Decisions go in specs, ADRs, or the product docs.

This review reads LoomDSP as a hybrid modular user: someone with a Eurorack system and a DC-coupled audio interface, who uses the computer as more modules in the rack. LoomDSP sends pitch, gates, clocks, and slow modulation out as control voltages, takes the rack's clock, pitch, and envelopes back in, and processes the rack's audio. The [preferences spec](../design/preferences.md) already quotes this user: "My audio interface sends control voltages to my modular, so I run with output safety off." Do not update this snapshot later; write a new one after the fixes.

## Method

- Followed one signal each way: a sequencer's pitch and gate out to an oscillator in the rack, a clock from the rack into a patch, and the rack's audio back through a filter.
- Read the output safety spec and its modes, ADR-0007 (signal ranges), the triggers and gates explainer, the audio input spec, the clock spec, and the module catalogue's I/O and gate entries; the Audio In and Audio Out help pages; and how the host writes output channels (`crates/loom-host-desktop/src/lib.rs`).
- Searched the open issues for the Hardware CV module, the gate shaper, and per-channel safety.

**Limits.** No interface or rack was connected. Findings come from the specs, the code, and the electrical behaviour of converters in general, not from a measured trigger or a tuned oscillator.

Priority means: **High** blocks the task or misleads the user; **Medium** slows them or looks broken; **Low** is polish.

## Summary

LoomDSP's signal model fits a modular rack better than most software. Pitch at 0.1 per octave with 1.0 = 10 V is exactly 1 V per octave, with 0 V at middle C, so pitch needs no conversion. A gate is high above 0.5, which is 5 V. Cables are never clipped, timing is sample-accurate, and Audio In reads up to eight raw channels with no filtering, so control voltages from the rack arrive as they are.

The difficulties are all at the boundary with hardware. A one-sample trigger, the right choice inside a patch, is about 20 µs long at a converter and can be missed by modules that sample their inputs. Output safety is one mode for the whole machine and only one output pair exists, so the user chooses between protecting their speakers and sending control voltages. Even in Off mode, every patch load ramps the outputs from 0 V, so the rack's oscillators swoop. There is no way to calibrate an output's volts, and the transport cannot follow the rack's clock. The project already plans most of the answers (per-channel safety, a gate shaper, a Hardware CV module), but in M2 to M4 and without issues.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Triggers | A one-sample trigger is about 20 µs at the converter; modules that sample their inputs can miss it | High |
| 2 | Safety | One safety mode for the machine and one output pair: protect the speakers or send control voltages, not both | Medium |
| 3 | Safety | Even in Off, opening a patch ramps every output from 0 V, so the rack swoops | Medium |
| 4 | Pitch | No way to calibrate an output's volts; 1 V per octave tracking depends on the interface | Medium |
| 5 | Clock | The transport cannot follow a clock from the rack | Medium |
| 6 | Plan | The Hardware CV module is M4 and has no issue; per-channel safety waits for more outputs (#506, M3) | Low |
| 7 | Input | Control voltages come in through Audio In as stereo pairs and red "audio" cables | Low |

## Findings

### 1. One-sample triggers at the converter (High)

A trigger output is 1.0 for exactly one sample ([triggers and gates](../learn/triggers-and-gates.md), rule 2). The explainer gives good reasons: a fixed 1 ms pulse merges close triggers and changes with the sample rate. It solves the one cost it names, that a one-sample trigger is invisible on a scope, by stretching triggers in the display only.

The hardware boundary has the same problem as the display, and nothing solves it there. At 48 kHz a one-sample trigger is about 21 µs, and the interface's reconstruction filter smears it into a short, lower bump. Many digital modules read their inputs a few thousand times a second and can miss a pulse that short; even analogue trigger inputs are often designed for pulses of a millisecond or more. So Clock Divide, Phase Trigger, and every other trigger output may fire nothing in the rack, or fire unreliably.

Today the only workaround is to send a gate instead: Clock's gate output has a width in percent of its cycle. The gate shaper that would set a gate's length (MT-GATE-01) is in the catalogue for M2, with no issue.

Direction: stretch triggers at the hardware boundary, as the display does: an output in control voltage use turns each trigger into a pulse of a set length, in milliseconds, so the cable inside the patch stays exact. Until then, build the gate shaper, and add the hardware boundary to the triggers explainer.

### 2. Speakers or control voltages, not both (Medium)

Output safety has three modes, set once per machine ([output safety](../design/output-safety.md)). Off is documented as the mode "for DC-coupled interfaces that send control voltages to hardware", because the DC blocker removes a held voltage within a few hundred milliseconds and the limiter would compress any voltage above about 8.9 V. That is right.

But there is one Audio Out, it plays only on the first pair (the host writes silence to the others), and the mode applies to all of it. A user who wants the patch's audio on their monitors, protected, and its control voltages on outputs 3 to 8, raw, cannot have both. Per-channel modes are listed as out of scope "until outputs beyond stereo"; output pairs on Audio Out are #506 (M3).

Direction: plan the two together. When Audio Out can pick a pair (#506), each pair has its own safety mode, and a pair marked for control voltages skips the DC blocker and the limiter. The machine's default stays protected.

### 3. Opening a patch swoops the rack (Medium)

The fade-in runs "when the engine starts, in every mode", and the app builds a new engine for every Open and New (output safety, rule 8). Changing the mode dips the output to 0 and back (rule 9). For audio this prevents clicks. For a control voltage it means every held pitch falls to 0 V and climbs back over 10 ms, every gate drops, and every slow modulation jumps. Each oscillator in the rack swoops at each patch load, and an envelope may retrigger.

Direction: decide in the per-pair safety work (finding 2) what a control voltage pair does at start, stop, and panic: hold its last value, go to a set rest value, or ramp. A ramp is right for audio; for a pitch it is a glide nobody asked for.

### 4. No calibration (Medium)

The model assumes 1.0 is 10 V at the jack. Interfaces differ: their full scale varies by model, and each output has a small gain and offset error. For gates that does not matter. For pitch it does: a 1% scale error is 12 cents per octave, about an eighth of a semitone, so a melody across three octaves is 36 cents out at the top, clearly out of tune against the rack.

Today a user can put a Scale module before Audio Out and tune its factor and offset by ear. That works, but it lives in the patch, so it is wrong on any other interface, and it is lost when the patch is shared.

Direction: per-output scale and offset, stored with the machine (like the safety mode), not the patch, with a simple tuning step: send two known pitches, measure, and solve. This belongs in the Hardware CV module (MT-IO-10).

### 5. The transport cannot follow the rack (Medium)

The transport's source is internal, a host, or MIDI clock (M2, #77) ([clock](../design/clock.md)). A clock pulse from the rack can drive triggers in a patch directly, through Audio In into Clock Divide or a sequencer, and that already works. But the transport keeps its own tempo, so everything synced to the bar or beat phase (synced LFOs, Clock, Phase Scale from the transport) drifts against the rack.

Direction: a pulse clock source for the transport: N pulses per quarter note from an input channel, with the tempo worked out from the pulse spacing. It fits beside MIDI clock in the clock spec (#77), which already handles following an external tempo.

### 6. The plan exists, but late and unfiled (Low)

The Hardware CV module (MT-IO-10, "exchanges DC-coupled control voltages with modular hardware through an audio interface") is planned for M4. Per-channel safety modes wait for more outputs. The gate shaper is M2. None has an issue. Findings 1 to 4 meet in the Hardware CV module, and finding 2 depends on #506 (M3).

Direction: move MT-IO-10 to M3, with #506, so control voltage outputs arrive with output pairs rather than a milestone later, and give it an issue with a spec.

### 7. Control voltages arrive as audio (Low)

Audio In reads raw channels, with no filtering, a gain of 1 by default, and up to eight channels, so a pitch or envelope from the rack arrives exactly. It does work. But it reads pairs, so one control voltage needs a Split, and its output is an audio cable: red, round, and labelled audio, though it carries pitch. Types are hints and nothing breaks; it only reads oddly on the canvas.

Direction: when the Hardware CV module exists, its inputs say what they carry (pitch, gate, modulation), one channel each.

## What works well

- **Pitch is 1 V per octave already.** 0.1 per octave with 1.0 = 10 V is exactly 1 V per octave, with 0 V at middle C (ADR-0007). No conversion module, no scaling.
- **Gates are high above 0.5, which is 5 V,** close to the usual threshold of modular gate inputs.
- **Cables are never clipped,** so a control voltage keeps its full range through the patch; only the final output clips to ±10 V.
- **Off mode is documented for this case,** remembered per machine, and switchable at run time without a restart (preferences spec).
- **Audio In is raw:** no high-pass filter, unity gain by default, up to eight channels, so control voltages arrive as sent.
- **Sample-accurate, deterministic timing** means a gate leaves on the sample the patch says, every time.
- **Triggers inside a patch are exact,** and the explainer is honest about why; the hardware boundary only needs the same kind of stretch the display already has.

## Issues

Grouped to keep the backlog small. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#520](https://github.com/brylie/LoomDSP/issues/520) Hardware CV (MT-IO-10): per-pair safety, trigger stretching, start and stop behaviour, and calibration, with output pairs (#506) | 1, 2, 3, 4, 6, 7 | M3 | P2 | L |
| [#521](https://github.com/brylie/LoomDSP/issues/521) Gate shaper (MT-GATE-01): set a gate's length, delay it, or turn an edge into a trigger | 1 | M2 | P2 | S |
| [#77](https://github.com/brylie/LoomDSP/issues/77) (comment): a pulse clock from hardware as a transport source, beside MIDI clock | 5 | | | |
| [#506](https://github.com/brylie/LoomDSP/issues/506) (comment): per-pair safety modes for control voltage outputs | 2, 3 | | | |

## Suggested order

1. The gate shaper (1, #521). Small, useful inside patches too, and the workaround for triggers until the Hardware CV module exists.
2. Add the hardware boundary to the triggers explainer (1), with the Hardware CV issue (#520).
3. Output pairs (#506) with per-pair safety (2, 3).
4. The Hardware CV module (1, 4, 7, #520), in M3.
5. Pulse clock as a transport source (5), with MIDI clock (#77).
