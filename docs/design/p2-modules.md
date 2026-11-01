# P2 Module Ports and Parameters Specification

The ports, parameters, ranges, defaults, and behaviour of the P2 modules that bring no new engine rules, so each can be built from its catalogue entry without guessing.

- **Kind:** living specification.
- **Status:** Agreed for Random, Chance, Phase reset, Euclidean rhythm, Selector, Phase shift and reverse, Macro, Pitch quantiser, Multi-lane sequencer, and Spectrum (2026-11-01). Each section is ready to build. Other P2 modules are added here as they are agreed.
- **Decision behind it:** none. It follows [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signals), [ADR-0014](../adr/0014-modulation-without-cables.md) (modulation without cables), [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) (port and parameter text), and [ADR-0018](../adr/0018-clean-room-module-development.md) (clean room).
- **Use case and requirements:** FR-LIB-01, FR-LIB-08, FR-LIB-12, FR-LIB-14, FR-LIB-19, FR-LIB-22, FR-LIB-29, FR-LIB-30, NFR-SND-04.
- **Background:** [Phase and cycles](../learn/phase-and-cycles.md).
- **Prior art:** none named. Every module is designed from its purpose. The Euclidean rhythm follows published theory: G. Toussaint, "The Euclidean Algorithm Generates Traditional Musical Rhythms", 2005.
- **Issues:** [#304](https://github.com/brylie/LoomDSP/issues/304) (Random), [#305](https://github.com/brylie/LoomDSP/issues/305) (Chance), [#174](https://github.com/brylie/LoomDSP/issues/174) (Phase reset), [#306](https://github.com/brylie/LoomDSP/issues/306) (Euclidean rhythm), [#310](https://github.com/brylie/LoomDSP/issues/310) (Selector), [#111](https://github.com/brylie/LoomDSP/issues/111) (Phase shift and reverse), [#78](https://github.com/brylie/LoomDSP/issues/78) (Macro), [#308](https://github.com/brylie/LoomDSP/issues/308) (Pitch quantiser), [#307](https://github.com/brylie/LoomDSP/issues/307) (Multi-lane sequencer), [#109](https://github.com/brylie/LoomDSP/issues/109) (Spectrum). Pitch modules follow [Scale and tuning](scale-and-tuning.md).
- **Code:** `crates/loom-modules/src`, once built.

## Goal

> A steady clock drives my patch. A Chance module lets three of every four hats through, a Random module picks a new filter cutoff on each bar, and a Phase reset restarts my bar-length LFO when the chorus starts. When I reload the patch, the "random" parts play exactly as before.

## Rules for all modules here

The [rules for P1 modules](p1-modules.md#rules-for-all-modules-here) apply: IDs by category, a label and description for every port and parameter, 5 ms smoothing for parameters that change a value over time, starting at non-default values, choices with names, and the module checklist with reference tests at block sizes 1, 77, and 512. In addition:

1. **Random modules are seeded** (TR-07). A `seed` parameter, a whole number from 1 to 65535, default 1, is kept in the inspector, as on Noise. The same seed gives the same output on every load and at every block size (NFR-SND-04). A right-hand stereo copy mixes its channel into the seed, as Noise does ([stereo](stereo.md), rule 27). *Why:* a patch varies while it plays, but sounds the same each time it loads.
2. **A `reset` trigger restarts the random sequence from the seed.** A reset and a trigger on the same frame act in that order: reset first, so the trigger gets the first value of the sequence. *Why:* a bar or phrase clock on `reset` makes a random phrase repeat every N bars.
3. **Triggers count rising edges** ([OQ-19](../product/open-questions.md#resolved)): above 0.5 after a frame at or below it. The first frame counts as following a low one.

## Random (MT-RAND-01)

`loom.rand.random`: a new random value on each trigger, within a range, or a random walk.

| | |
|---|---|
| Inputs | `trigger` (trigger, default 0), `reset` (trigger, default 0) |
| Outputs | `out`, `out2` |
| Parameters | `mode`: 0 Step (default), 1 Walk. `low`: −10 to 10, default 0. `high`: −10 to 10, default 1. `shape`: 0 Uniform (default), 1 Centred. `step`: 0 to 1, default 0.1. `glide`: 0 to 5 s, default 0. `seed` (rule 1) |
| Panel | `low` and `high` on one [range](panel-widgets.md#range) widget; until it exists, two knobs. Radio buttons for `mode` and `shape`. Knobs for `step` and `glide`. `seed` in the inspector only. |

- **Position inside the range.** The module keeps each output's position `u` from 0 to 1, and outputs `low + u × (high − low)`. If `low` is above `high`, the two are swapped. *Why:* a moved or modulated range rescales the output at once, and a walk can never leave it.
- `low` and `high` have the [control ranges](control-ranges.md) −1 to 1 (default) and −10 to 10.
- **Step mode:** on each trigger, `u` gets a new draw. `out2` gets its own, independent draw on the same trigger. *Why two outputs:* one clock often needs two unrelated random values, such as a pitch and a velocity.
- **Walk mode:** on each trigger, the position moves by a random amount. With one output used, `u` moves by a draw from −`step` to +`step`. The two outputs together move as one point in two dimensions: a random step of length up to `step`, in any direction, with an even spread over the disc. `step` is a fraction of the range.
- **Edges bounce** (settled). A move past 0 or 1 reflects back by the overshoot. *Why:* values stay spread over the whole range and never stick to an edge, as they would if they stopped there.
- **Shape** (settled). Uniform: every value equally likely. Centred: the mean of two uniform draws, so values near the middle are more likely (a triangle shape). In walk mode, the shape applies to each step's size.
- **Glide** (settled). With `glide` above 0, each output moves to its new value in a straight line over `glide` seconds. A trigger during a glide starts a new glide from where the output is. At 0 the output steps. When the Smoothing trait (TR-11, FR-SDK-07) exists, `glide` moves onto it.
- **Start:** both positions start at the middle (0.5). The outputs hold until the first trigger.
- **Reset** (rule 2) restarts the generator from the seed. In walk mode it also puts both positions back at the middle, so the same walk replays. In step mode the outputs hold until the next trigger.

## Chance (MT-RAND-03)

`loom.rand.chance`: four lanes. Each passes a trigger or gate to output A with a set chance, and to output B otherwise.

| | |
|---|---|
| Inputs | `trigger1` to `trigger4` (trigger, default 0), `reset` (trigger, default 0) |
| Outputs | `a1` to `a4`, `b1` to `b4` |
| Parameters | `chance1` to `chance4`: 0 to 1, default 0.5. `seed` (rule 1) |
| Panel | Four knobs, `chance1` to `chance4`. `seed` in the inspector only. |

- **The choice is made on the rising edge.** A draw `r` from 0 up to (not including) 1 sends the lane to A when `r < chance`, and to B otherwise. So a chance of 0 never picks A, and 1 always does. Use only A for a probability gate.
- **The chosen output follows the input** (settled). From the rising edge until the input falls to 0.5 or below, the chosen output carries the input's value; the other output is 0. *Why:* a trigger stays a trigger and a gate keeps its length, so Chance works on clocks and on note gates alike.
- **A lane with no cable on its trigger follows the lane above,** through `is_connected`. Lane 1 with no cable gets nothing. Each lane still makes its own choice. *Why:* one clock into lane 1 feeds all four, each with its own chance.
- **One seed, one stream per lane.** Each lane's generator starts from the seed mixed with the lane number. *Why:* the choices of one lane depend only on its own triggers, so patching another lane never changes them.
- **Modulation** of a chance is through assignments (ADR-0014), not extra inputs.
- **Reset** (rule 2) restarts every lane's stream from the seed.

## Phase reset (MT-PHASE-04)

`loom.phase.reset`: restarts a phase at zero on a trigger.

| | |
|---|---|
| Inputs | `phase` (phase, default **bar phase**), `reset` (trigger, default 0) |
| Output | `phase` |
| Parameters | none |
| Panel | empty |

- The output is `fract(input − offset)`. The offset starts at 0, so the input passes through unchanged. On a reset, the offset becomes the input's value on that frame, so the output is 0 there.
- **After a reset, the output moves at the input's speed** (settled). *Why:* the module changes where a phase starts, not how fast it runs. A phase with its own rate is the Phasor's job (MT-PHASE-01).
- A reset that is high on the first frame counts (rule 3).
- With nothing on `phase`, the module restarts the transport's bar phase from a trigger, and holds when the transport stops, as the bar phase does ([clock](clock.md#default-inputs)).
- A jump in the input moves the output by the same amount.

## Euclidean rhythm (MT-GATE-02)

`loom.gate.euclid`: spreads a number of hits as evenly as possible over a number of steps, with rotation.

| | |
|---|---|
| Inputs | `phase` (phase, default **bar phase**), `clock` (trigger, default 0), `reset` (trigger, default 0) |
| Outputs | `trigger`, `gate`, `accent` |
| Parameters | `source`: 0 Phase (default), 1 Clock. `steps`: whole number 1 to 32, default 16. `hits`: whole number 0 to 32, default 4. `rotation`: whole number 0 to 31, default 0 |
| Panel | Radio buttons for `source`, knobs for `steps`, `hits`, and `rotation`. A read-only [step grid](panel-widgets.md#step-grid) in the ring layout shows the pattern and the current step, once that widget exists. |

- **The pattern.** Step `i` (from 0) of the unrotated pattern is a hit when `(i × hits) mod steps < hits`. `hits` above `steps` counts as `steps`. Rotation moves the pattern later: step `i` plays pattern step `(i − rotation) mod steps`. *Why this rule:* it is a closed form of the Euclidean spread. It gives the published patterns up to rotation (3 in 8 is `x..x..x.`), and it fixes which rotation is 0, so tests are exact.
- **Source** is a setting, not chosen by a cable. *Why:* the `phase` input has a default, so a cable cannot be the switch, as on the LFO ([P1 modules](p1-modules.md#lfo-mt-mod-01)).
- **Phase:** the current step is `floor(phase × steps)`, clamped to `steps − 1`. With nothing on `phase`, the pattern fills one bar and holds when the transport stops. `clock` and `reset` are ignored.
- **Clock:** each `clock` trigger moves to the next step, wrapping. `reset` goes back to step 0 on its frame; a reset and a clock on the same frame give step 0. The module starts at step 0. `phase` is ignored.
- **Outputs.** `trigger` is high for one sample on entering a hit step, from either direction. `gate` is high from entering a hit step until leaving it. `accent` is a trigger on entering the step that holds pattern step 0, the pattern's first hit. With 0 hits, nothing fires.
- **Changes apply at the next step.** The pattern for a step is taken when the step is entered, so turning a knob never cuts a step in half.
- The first frame counts as entering its step (rule 3), so a pattern that starts on a hit fires at once.

## Selector (MT-ROUTE-01)

`loom.route.selector`: picks one of up to eight inputs.

| | |
|---|---|
| Inputs | `in1` to `in8` (default 0), `step` (trigger, default 0), `select` (default 0), `reset` (trigger, default 0) |
| Output | `out` |
| Parameters | `mode`: 0 Step (default), 1 Select, 2 Random. `count`: whole number 2 to 8, default 4. `fade`: 0 to 1 s, default 0.005 s. `repeats`: 0 Avoid (default), 1 Allow. `seed` (rule 1) |
| Panel | Radio buttons for `mode`, knobs for `count` and `fade`. `repeats` and `seed` in the inspector only. |

- **Mode** is a setting, not chosen by which input has a cable ([P1 modules, Sample and hold](p1-modules.md#sample-and-hold-loomrandsample-hold)).
- **Step:** each `step` trigger moves to the next input, wrapping after `count`. **Select:** the input is `floor(select × count)`, with `select` clamped to 0 to 1 and the result to `count − 1`; so 0.6 with 4 inputs picks the third. **Random:** each `step` trigger picks an input at random from the first `count`. With `repeats` set to Avoid, it never picks the current one again.
- `reset` goes to input 1 and restarts the random sequence (rule 2). The module starts on input 1.
- Lowering `count` below the current input moves to the last input in use.
- **Fade.** Each input has a gain that moves in a straight line toward 1 (the chosen input) or 0 (the others) over `fade` seconds. The output is the sum of each input times its gain, divided by the sum of the gains. At 0 the switch is instant. *Why:* an instant switch clicks on audio; a straight line never overshoots, so the output stays between the inputs, and dividing by the sum keeps the level steady when switches come faster than the fade.

## Phase shift and Phase reverse (MT-PHASE-05)

Two built forms of the Phase warp type (ADR-0017): small, cheap, and needed now to fire at a chosen point of a cycle ([phase and cycles](../learn/phase-and-cycles.md#crossing-a-point-shift-then-detect)). The rest of Phase warp (bend, S-curve, skew, mirror) comes later.

`loom.phase.shift`:

| | |
|---|---|
| Inputs | `phase` (phase, default **bar phase**), `offset` (default 0) |
| Output | `phase` |
| Parameter | `offset`: −1 to 1, default 0 |
| Panel | A knob for `offset` |

- The output is `fract(phase + offset parameter + offset input)`.

`loom.phase.reverse`:

| | |
|---|---|
| Input | `phase` (phase, default **bar phase**) |
| Output | `phase` |
| Parameters | none |
| Panel | empty |

- The output is `fract(1 − phase)`, so an input of 0 gives 0. *Why:* Phase Trigger's first-frame rule (a phase of exactly 0 on the first frame fires) keeps working on a reversed phase.

## Macro (MT-UI-02)

`loom.ui.macro`: one knob whose value is a signal. Assign its output to many parameters with different depths, so one knob shapes a whole sound ([modulation, Example: a macro](modulation.md#example-a-macro)).

| | |
|---|---|
| Inputs | none |
| Output | `out` |
| Parameter | `value`: 0 to 1, default 0 |
| Panel | One knob |

- The output is `value`, smoothed over 5 ms (P1 rule 3). *Why no polarity setting:* each modulation assignment already sets its own polarity.
- The ID is in the `ui` category, with Button (MT-UI-01). The issue proposed `loom.mod.macro`; this spec wins.

## Pitch quantiser (MT-PITCH-02)

`loom.pitch.quantise`: snaps a pitch signal to the nearest note of a scale.

| | |
|---|---|
| Inputs | `in` (pitch, default 0), `trigger` (trigger, default 0) |
| Outputs | `out` (pitch), `changed` (trigger) |
| Parameters | `mode`: 0 Continuous (default), 1 On trigger. `scale`: 0 Patch (default), 1 Own ([TR-08](scale-and-tuning.md#following-the-scale)). `root`: whole number 0 to 63, default 0 (Own only) |
| Data | `scale`: a one-lane gate pattern, one cell per pitch class (Own only) |
| Panel | Radio buttons for `mode` and `scale`. With Own, a step grid in the keys layout for the scale, with the playing note as the playing cell. |

- **Snapping:** `out` is the note of the scale nearest to `in` in pitch, in any octave of the tuning ([scale and tuning](scale-and-tuning.md#tunings)). A scale with no notes in gives `out = in`.
- **Hysteresis:** to move from the current note to a neighbour, `in` must pass the midpoint between them by 10% of the gap. *Why:* a slow or noisy input near a midpoint never flickers between two notes. On the first frame, an exact tie picks the lower note.
- **Mode** is a setting, not chosen by a cable on `trigger`. Continuous: `out` follows `in` all the time. On trigger: `in` is snapped and held on each rising edge of `trigger` (rule 3).
- **`changed`** is a one-sample trigger whenever `out` moves to another note, including after a key change. *Why:* it can fire an envelope, so a quantiser plays notes by itself.
- Snapping to the notes being held waits for the note model (OQ-16).

## Multi-lane sequencer (MT-SEQ-01)

`loom.seq.lanes`: four lanes of stored steps, each with its own length, for gates, pitches, or values.

| | |
|---|---|
| Inputs | `phase` (phase, default **bar phase**), `clock` (trigger, default 0), `reset` (trigger, default 0), `select` (default 0) |
| Outputs | `out1` to `out4`, `accent1` to `accent4` |
| Parameters | `read`: 0 Phase (default), 1 Clock, 2 Select. `division`: whole number 1 to 64, default 16 (Phase only). `steps1` to `steps4`: whole number 1 to 64, default 16. `type1` to `type4`: 0 Gate, 1 Pitch, 2 Value; defaults Gate, Pitch, Value, Gate. `length`: 0.05 to 1, default 0.5. `scale`: 0 Patch (default), 1 Own. `root`: whole number 0 to 63, default 0 (Own only). `seed` (rule 1) |
| Data | `pattern`: 4 lanes of up to 64 steps, with step attributes ([data kinds](panel-widgets.md#data-kinds)). `scale`: a one-lane gate pattern (Own only) |
| Panel | A step grid (rows layout) for `pattern`, with the attribute picker; radio buttons for `read`; knobs for `division`, `length`, and `steps1` to `steps4`. `type1` to `type4`, `scale`, `root`, and `seed` in the inspector. |

**Settled:** four fixed lanes, each with its own length, so lanes can run in polymeter. *Why four:* ports are static in a descriptor. *Why own lengths in P2:* a 5-step lane against a 16-step lane is one of the quickest ways to make a pattern that keeps changing; it moves forward from P3 (FR-LIB-14). Per-lane rate and direction stay in P3.

**Reading** (`read` is a setting, not chosen by a cable):

- **Phase:** `division` sets one step length for all lanes, as steps per phase cycle. The module counts phase cycles (a wrap is a fall of more than 0.5, as in [phase and cycles](../learn/phase-and-cycles.md#finding-the-end-of-a-cycle); a backward wrap counts down). The step count is `cycles × division + floor(phase × division)`, and lane `i` plays step `count mod steps_i`. *Why:* a true polymeter. Lanes share a pulse and drift against each other: 5 steps against 16 line up again every 80 steps. With nothing on `phase`, steps follow the bar and hold when the transport stops.
- **Clock:** each `clock` trigger adds one to the count. Lane `i` plays `count mod steps_i`.
- **Select:** lane `i` plays step `floor(select × steps_i)`, with `select` clamped to 0 to 1 and the result to `steps_i − 1`.
- `reset` makes the current frame step 0 in every lane, in every mode. The module starts at step 0.
- A lane **enters a step** when its step number changes, and on the first frame (rule 3). Edits to the pattern or the parameters apply when a step is entered, never mid-step.

**Lane types:**

- **Gate:** a gate cell that plays sets the output high for `length` of the step, then low. In Phase mode the time within a step comes from the phase. In Clock mode the step is the last interval between clocks, in samples; before the second clock, the gate stays high for the whole step. In Select mode a gate cell is high while it is selected.
- **Pitch:** each cell is a scale degree. Its value from 0 to 1 picks a degree over two octaves above the scale's root (rounded to the nearest of the scale's notes in those two octaves). The output is that note's pitch. A key change moves the notes with it. *Why degrees:* a pattern stays in key when the scale changes.
- **Value:** each cell is a value from 0 to 1, output as is. Scale it with modulation depth or a Scale module.

**Step attributes** (settled, included now):

- **Chance** (0 to 1, default 1): on entering a step, the lane draws a random number; the step plays only if it is below the chance. A step that does not play leaves a gate low and holds a pitch or value at its last value. Each lane has its own stream from the seed, and draws on every step it enters, whether or not it needs the draw. *Why:* changing one step's chance never changes what the other steps do. `reset` restarts the streams (rule 2).
- **Repeat** (whole number 1 to 4, default 1): a gate step is split into that many equal parts, each with a gate for `length` of its part. Ignored on pitch and value lanes.
- **Accent** (on or off, default off): `accent<i>` is high with the same timing as a gate on lane `i`, on accented steps that play. On pitch and value lanes it uses the timing a gate would have. *Why separate outputs:* gates stay plain gates, and accent can drive a level, a filter, or a second envelope.

## Spectrum (MT-VIEW-03)

`loom.display.spectrum`: shows the frequency content of a signal.

| | |
|---|---|
| Input | `in` (audio, default 0) |
| Output | `out` (passes `in` through, sample for sample) |
| Parameters | `tilt`: 0 Flat (default), 1 +4.5 dB per octave. `hold`: 0 Off (default), 1 On |
| Panel | A [spectrum](panel-widgets.md#spectrum) widget bound to `out`. `tilt` and `hold` in the inspector. Growable tile. |

- Mono, like the Level Meter. *Why a pass-through:* probes read outputs, and the module can sit inline or hang off an output without changing the sound, as the Level Meter and Scope do.
- The parameters change only the drawing, never the sound (panel widgets rule 6).

## Acceptance criteria

Random:

- [ ] The same seed gives the same values at block sizes 1, 77, and 512 (reference test).
- [ ] Over 100,000 triggers in step mode with Uniform, each tenth of the range gets 10% of the values, within 1 percentage point (test).
- [ ] Centred puts more values in the middle fifth than in either outer fifth (test).
- [ ] Walk mode never leaves the range, in one and two dimensions, including while the range is modulated (test).
- [ ] A reset replays the same values from the start, in step and walk mode (test).
- [ ] With `glide` 0.1 s, a new value is reached after 0.1 s in a straight line (test).

Chance:

- [ ] Chance 0 never sends to A, and 1 always does (test).
- [ ] Over 10,000 triggers at 0.3, between 28% and 32% go to A (test).
- [ ] A gate of 100 frames chosen for A gives 100 frames on A and none on B (test).
- [ ] One clock on lane 1 drives lanes 2 to 4 when they have no cable. With the same clock cabled to lane 3, lane 3's choices are the same whether or not lane 2 has its own cable (test).
- [ ] The same seed gives the same choices at block sizes 1, 77, and 512 (reference test).

Phase reset:

- [ ] A reset lands on the same sample at block sizes 1, 77, and 512 (test).
- [ ] After a reset, the output rises at the input's rate from 0 and wraps when it reaches 1 (test).
- [ ] A reset on the first frame counts (test).

Spectrum:

- [ ] The output equals the input, sample for sample (test). The widget checks are in [panel widgets](panel-widgets.md#acceptance-criteria).

Euclidean rhythm:

- [ ] 3 hits in 8 steps gives `x..x..x.`; 5 in 8 and 5 in 16 match the published patterns up to rotation, and rotation moves them later (unit tests).
- [ ] Driven by a phase, a hit lands on the same sample at block sizes 1, 77, and 512 (reference test).
- [ ] Changing `hits` mid-step takes effect at the next step (test).
- [ ] In Clock mode, a reset and a clock on the same frame give step 0 (test).

Selector:

- [ ] With 4 inputs in Select mode, 0.6 picks the third input; −1 and 2 clamp to the first and fourth (test).
- [ ] With a fade, no step in the output is larger than the step between the inputs at that moment (test).
- [ ] Random with Avoid never picks the current input twice in a row over 10,000 steps (test).
- [ ] Stepping and random picks are the same at block sizes 1, 77, and 512 (reference test).

Phase shift and reverse:

- [ ] Shift by −0.3, then Phase Trigger, fires when a slow phase passes 0.3, rising and falling (test).
- [ ] Reverse turns 0 into 0 and 0.25 into 0.75 (test).

Macro:

- [ ] The output equals the knob, and one Macro assigned to three parameters moves all three (test).

Pitch quantiser:

- [ ] In C major, every output is a scale note. A pitch rising from C moves to D only after it passes the C to D midpoint by 10% of the gap, and falling back moves to C only after passing the midpoint by 10% the other way (unit tests).
- [ ] A slow ramp across one octave in C major gives exactly 7 `changed` triggers (test).
- [ ] On trigger mode holds between rising edges (test).
- [ ] Output is the same at block sizes 1, 77, and 512 (reference test).

Multi-lane sequencer:

- [ ] Driven by the bar phase with division 16, each step starts on the same sample at block sizes 1, 77, and 512 (reference test).
- [ ] A 5-step lane and a 16-step lane line up again after 80 steps, in Phase and Clock modes (test).
- [ ] Editing a step while it plays does not allocate (`no_alloc` test) and applies at the next step (test).
- [ ] A pitch lane plays scale degrees, and a key change moves them (test).
- [ ] A step with chance 0 never plays and 1 always does; changing one step's chance leaves the other steps' choices unchanged (test).
- [ ] Repeat 3 gives three gates in the step; accent outputs follow accented steps only (test).
- [ ] The pattern with attributes saves, reloads unchanged, and each edit is one undo step (test, with [module data](module-data.md)).

## Out of scope

- The Smoothing trait itself (TR-11). Random uses a plain `glide` until it exists.
- Distributions other than Uniform and Centred.
- More than four lanes in Chance or the sequencer, or more than eight inputs in Selector.
- Per-lane rate and direction in the sequencer (P3, FR-LIB-14).
- The rest of Phase warp (bend, S-curve, skew, mirror, smoothing).

## Open points

None.
