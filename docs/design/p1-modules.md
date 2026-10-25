# P1 Module Ports and Parameters Specification

The ports, parameters, ranges, defaults, and behaviour of the P1 modules that bring no new engine rules, so each can be built from its catalogue entry without guessing.

- **Kind:** living specification.
- **Status:** Agreed on merge. Each section is ready to build.
- **Decision behind it:** none. It follows [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signals), [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) (port and parameter text), and [ADR-0018](../adr/0018-clean-room-module-development.md) (clean room).
- **Use case and requirements:** UC1, FR-LIB-01, FR-LIB-03, FR-LIB-04, FR-LIB-05, FR-LIB-07, FR-MIDI-01, FR-SIG-04.
- **Background:** [Phase and cycles](../learn/phase-and-cycles.md).
- **Prior art:** none named. Every module is designed from its purpose and from published theory (textbooks and papers), cited where it matters.
- **Issues:** [#303](https://github.com/brylie/LoomDSP/issues/303) (Constant), [#173](https://github.com/brylie/LoomDSP/issues/173) (Phase wrap and fold), [#39](https://github.com/brylie/LoomDSP/issues/39) (Compare, Logic, Memory), [#38](https://github.com/brylie/LoomDSP/issues/38) (Mixer, Crossfade, Clamp, Scale), [#37](https://github.com/brylie/LoomDSP/issues/37) (Sample and hold, Slew), [#36](https://github.com/brylie/LoomDSP/issues/36) (LFO), [#35](https://github.com/brylie/LoomDSP/issues/35) (Envelope AD), [#34](https://github.com/brylie/LoomDSP/issues/34) (Ladder low-pass), [#33](https://github.com/brylie/LoomDSP/issues/33) (One-pole), [#30](https://github.com/brylie/LoomDSP/issues/30) (Controller input).
- **Code:** `crates/loom-modules/src`.

Modules in the clock family (Transport, Clock, Phase Scale) are in [clock.md](clock.md#modules).

## Goal

> I build the subtractive tutorial patch. I need a gate from a comparison, a slow wobble, a hold, and a filter that sings. Each module has the ports and knobs its name suggests, and the same patch sounds the same at any buffer size.

## Rules for all modules here

1. **Names follow the catalogue, IDs follow the category.** A module ID is `loom.<category>.<name>`.
2. **Every port and parameter has a `label` and a one-sentence `description`** ([port and parameter text](port-and-parameter-text.md)). The tables below give names only. Labels are the name in Title Case unless noted.
3. **Parameters that change a value over time are smoothed over 5 ms,** unless the table says otherwise. *Why:* a knob turn must not click (the existing `Constant`, `Add`, and `Multiply` do the same).
4. **A module added with non-default values starts at those values.** See [live patching, rule 7](live-patching.md#rules).
5. **Choices are whole-number parameters with choice names** ([panels](panels.md#choice-names)). The tables list the names in order, from 0.
6. **Each module follows the [module checklist](../../CONTRIBUTING.md#adding-a-module)** and has a reference test at block sizes 1, 77, and 512.

## Constant family (MT-LEVEL-01)

The catalogue type is built as three modules, one per unit. *Why not one module with a unit setting:* the knob's range, mapping, and unit all change with the unit, and parameters are static ([descriptor rules](panels.md#module-panels-on-tiles-p1)). Three small tiles are clearer on stage than one tile whose knob changes meaning.

| Module | Output | Parameter | Range | Default |
|---|---|---|---|---|
| `loom.math.constant` (exists) | The value, a plain number | `value` | Offers the [control ranges](control-ranges.md) | 0 |
| `loom.level.constant` Constant (level) | `out`: a linear level factor | `level`, in dB | -60 to 12 dB, decibel mapping ([#99](https://github.com/brylie/LoomDSP/issues/99)) | 0 dB |
| `loom.pitch.constant` Constant (pitch) | `out` (pitch signal): 0.1 per octave | `semitones` | -48 to 48 | 0 |

- Level output is `10^(dB/20)`. Pitch output is `semitones / 120`. *Why:* ADR-0007 gives the standard units of cables.
- Both smooth over 5 ms and ignore `value` modulation rules beyond the ordinary [parameter mapping](modulation.md#parameter-mapping).
- **Constant (level) stores its value in dB with the linear mapping,** so the knob is even in dB and the lowest setting is -60 dB (a factor of 0.001). The decibel mapping with -inf at the bottom ([#99](https://github.com/brylie/LoomDSP/issues/99)) is a later change to the knob, not to the module. *Why:* a value in dB needs no new mapping to sound right; -inf is a nicety of the control. The ranges for `loom.math.constant` come with [control ranges](control-ranges.md).
- **Ratio is dropped from MT-LEVEL-01.** Nothing says what a ratio outputs, and a ratio is `semitones / 12` through [Convert](https://github.com/brylie/LoomDSP/blob/main/crates/loom-modules/src/math.rs) when needed. The catalogue entry changes to match.
- Existing patches that use `loom.math.constant` load and sound the same.

## Phase wrap and fold (MT-PHASE-08)

One module, `loom.phase.wrap`, with a `mode`. *Why one:* both bring any number into 0 to 1, differ only in what happens past the edges, and share a tile ([merge rule](../adr/0017-primitives-traits-and-abstractions.md)).

| | |
|---|---|
| Input | `in` (any signal, default 0) |
| Output | `out` (phase, always in 0 to 1) |
| Parameter | `mode`: 0 Wrap (default), 1 Fold |
| Panel | Two radio buttons |

- **Wrap:** `out = x - floor(x)`. So 1.25 gives 0.25, -0.25 gives 0.75, and exactly 1 gives 0. If the `f32` result rounds to 1, it becomes 0. *Why:* a phase is always in [0, 1) ([clock.md](clock.md#signals-derived-from-the-transport)).
- **Fold:** `t = x - 2 × floor(x / 2)` (so `t` is in [0, 2)), then `out = t` if `t <= 1`, else `2 - t`. So 0 gives 0, 1 gives 1, 1.25 gives 0.75, 2 gives 0, and -0.25 gives 0.25. Fold's range is [0, 1], closed. *Why:* a fold has no wrap point, it turns around, so both edges are reachable values.
- NaN and infinity give 0. *Why:* a phase module must not pass NaN on to oscillators.
- No state, so the output is the same at any block size.

## Gate logic (MT-LOGIC-01 to 03)

Gates are high above 0.5 ([ADR-0007](../adr/0007-polymorphic-signal-model.md)). Outputs are exactly 0.0 or 1.0.

### Compare `loom.logic.compare`

| | |
|---|---|
| Inputs | `a` (default 0), `b` (default 0) |
| Output | `gate` |
| Parameters | `mode`: 0 Above (default), 1 Below. `hysteresis`: 0 to 1, default 0 |
| Panel | Two radio buttons, a knob |

- Above: `gate` is high while `a > b`. Below: high while `a < b`. Equal gives low in both modes.
- **Hysteresis** is a dead band around `b`: once high, the gate stays high until `a` falls `hysteresis` below `b` (Above), or rises that far above it (Below). *Why:* a noisy signal near the threshold would otherwise chatter.
- Other modes (window, equal, edges) come later ([catalogue](../product/module-catalogue.md)).

### Logic `loom.logic.logic`

| | |
|---|---|
| Inputs | `in1` to `in4` (gates, default 0) |
| Output | `out` |
| Parameters | `op`: 0 And (default), 1 Or, 2 Xor. `invert`: 0 or 1, default 0 |
| Panel | Radio buttons, a toggle |

- **Only connected inputs count.** An unconnected input is ignored, not low. *Why:* an And of two wires must not need four cables. `PortIo::is_connected` reports cables only, which is the point here.
- With no connected input, the result before `invert` is low.
- Xor of more than two inputs is true when an odd number are high.
- **Not** is one connected input with `invert` on, in Or. Nand, Nor, and Xnor are the three ops with `invert` on.

### Memory `loom.logic.memory`

| | |
|---|---|
| Inputs | `a`, `b` (default 0) |
| Output | `out` (gate) |
| Parameter | `mode`: 0 Set and reset (default), 1 Toggle, 2 Data |
| Display value | `state` (a light) |

What the inputs mean depends on the mode:

| Mode | `a` | `b` |
|---|---|---|
| Set and reset | Set: a rising edge turns `out` on | Reset: a rising edge turns `out` off |
| Toggle | Toggle: a rising edge flips `out` | Reset: a rising edge turns `out` off |
| Data | Data: a gate | Clock: on a rising edge, `out` takes the level of `a` |

- Rising edges use `signal::EdgeDetector`. `out` starts low.
- **Reset wins** when set (or toggle) and reset rise on the same frame. *Why:* a stop signal should never lose to a start signal.
- The state is the same at block sizes 1, 77, and 512, including edges on the first and last frame of a block.
- Switching mode keeps the state. *Why:* changing a setting should not look like an event.
- The latch that cycles through N states is [MT-LOGIC-04](https://github.com/brylie/LoomDSP/issues/175), a counter, in P3.

## Mixing and shaping (MT-MIX-01, MT-MIX-02, MT-LEVEL-03, MT-LEVEL-05)

### Mixer `loom.mix.mixer`

| | |
|---|---|
| Inputs | `in1` to `in4` (default 0) |
| Output | `out` |
| Parameters | `level1` to `level4`: 0 to 2, default 1. `mute1` to `mute4`: 0 or 1, default 0 |
| Panel | Four columns, each a knob over a toggle |

- `out = sum(in_n × level_n × (1 - mute_n))`. Level and mute are smoothed together over 5 ms, so a mute fades. *Why:* a hard mute clicks on stage.
- Four inputs, as the tile stays small. A bigger mix is two mixers in a row. Pan comes with stereo (the [catalogue](../product/module-catalogue.md) entry stays as it is).
- Inputs keep their channel count ([stereo](stereo.md)): each is summed per channel.

### Crossfade `loom.mix.crossfade`

| | |
|---|---|
| Inputs | `a`, `b` (default 0) |
| Output | `out` |
| Parameters | `mix`: 0 to 1, default 0.5. `law`: 0 Equal power (default), 1 Linear |
| Panel | A knob, two radio buttons |

- `mix` 0 is all `a`, 1 is all `b`. A [modulation assignment](modulation.md) can drive `mix`, so there is no `mix` input.
- **Equal power:** `out = a × cos(mix × π/2) + b × sin(mix × π/2)`. **Linear:** `a × (1 - mix) + b × mix`.
- *Why equal power by default:* audio from two sources adds in power, so a linear blend dips by 3 dB in the middle. Linear is exact for control signals, which is why it is offered.

### Clamp `loom.level.clamp`

| | |
|---|---|
| Input | `in` |
| Output | `out` |
| Parameters | `low`, `high`: both −10 to 10 and offer the [control ranges](control-ranges.md). Defaults −1 and 1 |

- `out` is `in` limited to the range between the two limits.
- **If `low` is above `high`, the limits swap.** *Why:* silence or a stuck value would be a bad surprise, and a modulated limit can cross the other.
- NaN becomes 0 before clamping. *Why:* Clamp is the tool for making a signal safe.

### Scale `loom.level.scale`

| | |
|---|---|
| Input | `in` |
| Output | `out` |
| Parameters | `factor`: −1 to 1, default 1. `offset`: −10 to 10 (offers the control ranges), default 0 |

- `out = in × factor + offset`. A factor of −1 inverts. Bipolar to unipolar is factor 0.5 with offset 0.5.
- **Range to range mapping is not built in P1.** It would be a separate module (`remap`), with four range ends, if the need appears. *Why:* factor and offset cover the P1 jobs, and a tile with four range knobs is a different tile.

## Sample and hold, and Slew (MT-RAND-02, MT-LEVEL-04)

### Sample and hold `loom.rand.sample-hold`

| | |
|---|---|
| Inputs | `in` (default 0), `trigger` (default 0) |
| Output | `out` |
| Parameter | `mode`: 0 Sample (default), 1 Track |
| Panel | Two radio buttons |

- **The mode is a setting,** not chosen by what is patched. *Why:* `is_connected` reports cables only, and a module that picks its behaviour from the cables would change when a cable is removed, which is hard to see.
- **Sample:** on a rising edge of `trigger`, `out` takes the value of `in` on the same frame, then holds. **Track:** while `trigger` is high, `out` follows `in`; when it goes low, `out` holds.
- `out` starts at 0. The held value is the same at block sizes 1, 77, and 512.
- With a noise source on `in`, this is the classic stepped random signal.

### Slew `loom.level.slew`

| | |
|---|---|
| Input | `in` |
| Output | `out` |
| Parameters | `rise` and `fall`: 0.0005 to 10 s, logarithmic, default 0.05 s |

- **The time is per unit of change:** `rise` is the time to go up by 1.0, `fall` the time to go down by 1.0. The output moves toward `in` at a constant rate of `1 / time` per second, and stops on arrival. *Why:* cables have no fixed range ([port ranges](port-ranges.md)), so "the full range" has no meaning. Per unit means the same on every signal, and a pitch glide of one octave (0.1) takes a tenth of `rise`.
- The first frame after the module is created or reset sets `out` to `in`, with no ramp. *Why:* a new slew must not sweep from 0.
- Per-sample, so block size changes nothing.

## LFO (MT-MOD-01)

`loom.mod.lfo`: a slow periodic signal, free-running or locked to the transport.

| | |
|---|---|
| Input | `phase` (phase, default **bar phase**) |
| Outputs | `out` (bipolar, −1 to 1), `out90` (the same wave a quarter cycle ahead), `phase` (the 0 to 1 phase being used) |
| Parameters | `sync`: 0 Free (default), 1 Synced. `rate`: 0.01 to 50 Hz, logarithmic, default 1. `cycles`: whole number 1 to 16, default 1. `shape`: 0 Sine (default), 1 Triangle, 2 Ramp up, 3 Ramp down, 4 Square |
| Panel | Two radio buttons (`sync`, `shape`), knobs for `rate` and `cycles` |

- **Free:** an internal phase moves at `rate` Hz. The `phase` input is ignored, even with a cable on it. **Synced:** the used phase is `fract(phase × cycles)`, so `cycles` is how many waves fit in one input cycle. `rate` is ignored. *Why a mode, not "a cable decides":* the input has a default, so `is_connected` cannot be the switch ([clock.md](clock.md#default-inputs)). A whole `cycles` keeps the wave continuous where the input wraps.
- With nothing connected in Synced mode, the output follows the transport's bar phase, and holds when the transport stops. Free mode keeps running.
- **Shapes** all cross 0 going up at phase 0, so a sine and a triangle line up: sine `sin(2πp)`; triangle `1 − 4·|fract(p + 0.25) − 0.5|`; ramp up `2·fract(p + 0.5) − 1`; ramp down is its negation; square is +1 for `p < 0.5` and −1 after.
- `out90` is the same shape at `fract(p + 0.25)`. A sine on `out` and `out90` into an XY scope draws a circle.
- Shapes are not band-limited. *Why:* an LFO is slow. For audio rate use the oscillators.
- A free LFO starts at phase 0. Its output is the same at any block size: the phase is advanced one sample at a time in `f64`.
- The shape source trait and retrigger from the catalogue are not built in P1.

## Envelope AD mode (MT-ENV-01)

The ADSR becomes the catalogue's Envelope: `loom.env.adsr` is renamed `loom.env.envelope` (early alpha allows it; every patch, test, and reference render is updated in the same pull request).

- **New parameters:** `mode`: 0 ADSR (default), 1 AD. `curve`: −1 to 1, default 0.
- **AD mode:** a rising gate starts the attack, the attack runs to 1, the decay then runs from 1 to 0, and the output stays at 0. The gate going low changes nothing. `sustain` and `release` are ignored. *Why:* AD is a one-shot, so it works from a trigger as well as a gate (a trigger is a one-sample gate).
- **Retrigger.** A rising gate during attack or decay (or during the release of ADSR) starts a new attack from the current level up to 1. The attack time is the full `attack` time. *Why:* no click, and the time stays the knob's value.
- **Curve.** Each stage has progress `x` from 0 to 1 over its time. The shaping power is `k = 2^(3 × curve)`, from 1/8 to 8. A rising stage is `1 − (1 − x)^k` and a falling stage is `(1 − x)^k`, between its start and end levels. At curve 0 every stage is linear. Positive curve is a fast start with a slow tail, as on analogue envelopes. Negative is a slow start. *Why:* one number gives the plucked and the swelling shapes.
- **This changes the sound of the ADSR** (today: linear attack, exponential decay). The reference renders and examples change in the same pull request, and the description lists it.
- Block sizes 1, 77, and 512 give identical output, with a retrigger in the middle of an attack and another in the middle of a decay.
- AR, delay and hold, looping, and the character trait are out of scope.

## Filters (MT-FILTER-02, MT-FILTER-03)

Both follow `loom.filter.svf`: an `in` port, a `cutoff` input in pitch units added to the `cutoff` parameter (so the cable and a knob combine), cutoff in Hz, logarithmic, smoothed over 10 ms. The ladder spans 20 to 20000 Hz. The one-pole goes down to 1 Hz, for smoothing.

### One-pole `loom.filter.onepole`

| | |
|---|---|
| Input | `in`, `cutoff` (pitch, default 0) |
| Outputs | `lowpass`, `highpass` |
| Parameter | `cutoff`: 1 to 20000 Hz, logarithmic, default 1000 |

- First order, 6 dB per octave. `highpass = in − lowpass`.
- **The cutoff is smoothed like the SVF, and the coefficient follows the smoothed value each sample.** *Why:* one rule for every filter. A cable into `cutoff` is applied per sample.
- At the cutoff frequency both outputs are 3 dB down (the −3 dB test, at 100 Hz, 1 kHz, and 10 kHz).

### Ladder low-pass `loom.filter.ladder`

| | |
|---|---|
| Input | `in`, `cutoff` (pitch, default 0) |
| Output | `out` |
| Parameters | `cutoff`: 20 to 20000 Hz, logarithmic, default 1000. `resonance`: 0 to 1, default 0.2. `drive`: 0 to 1, default 0. `slope`: a choice, 0 12 dB, 1 18 dB, 2 24 dB (default) |
| Panel | Radio buttons for `slope`, knobs for the rest |

- **One module with a slope setting, not several modules.** The slope picks how many one-pole stages sit in the loop: 2, 3, or 4. *Why:* a player who wants a gentler filter should not have to swap modules, and the stages are the same code. The 6 dB slope is the [one-pole filter](#one-pole-loomfilteronepole), which has no resonance.
- Cascaded one-pole low-pass stages with feedback from the last stage of the chosen length back to the input. `out` is that last stage.
- **Resonance scales the feedback gain `k`.** `k = resonance × k_max`, with `k_max` 4 at 24 dB, 8 at 18 dB, and 8 at 12 dB. *Why:* for N identical stages, the loop reaches self-oscillation when `(1 + s)^N = -k` has a root on the imaginary axis. That is `k = 4` for N = 4 (at the cutoff) and `k = 8` for N = 3 (at 1.7 times the cutoff). So at `resonance` 1, the 24 dB and 18 dB slopes self-oscillate. Two stages never reach it: at `k = 8` the 12 dB slope has a peak of about +4 dB, which is a strong resonance but not a whistle.
- **No gain compensation.** The low end thins as resonance rises, as on a classic ladder. *Why:* it is part of the character the catalogue entry describes. A player who wants the level kept raises `drive` or follows the filter with an amplifier.
- A saturating curve (tanh) sits in the feedback path. `drive` scales the input into the first stage: 0 is clean, 1 pushes the loop hard. *Why:* saturation limits the self-oscillation to a stable level and gives the sound its character.
- **Changing `slope` while playing** keeps the stage states and does not reset them. A stage that was not in the loop keeps running. The output of the old tap and the output of the new tap crossfade over 10 ms. During the crossfade the feedback gain is `resonance × min(old k_max, new k_max)`, so neither loop goes past its own self-oscillation limit. *Why:* a live switch must not click, restart the sound, or burst into oscillation. Moving the tap at once while `k_max` ramps would let the 4-stage loop run with `k` above 4 for part of the 10 ms, for example when going from 18 dB to 24 dB.
- Designed from published theory only (ADR-0018): Stilson and Smith, "Analyzing the Moog VCF with Considerations for Digital Implementation" (1996), and V. Zavalishin, "The Art of VA Filter Design" (stage and feedback topology, zero-delay form). Cite them in the code and the pull request.
- **Stability.** The cutoff is clamped to 0.45 of the sample rate. NaN and infinity in the input give 0 and clear the stage state. No allocation.
- No oversampling in P1, so heavy drive can alias. The catalogue's Quality trait is the place for it later.

## Controller input (MT-IO-04)

`loom.midi.controller`: one module for the type, with a `source` setting. *Why one module:* the ports are the same kind and the tile is similar ([merge rule](../adr/0017-primitives-traits-and-abstractions.md)).

| | |
|---|---|
| Output | `out` |
| Parameters | `source`: 0 Controller (default), 1 Pitch bend, 2 Aftertouch. `controller`: whole number 0 to 127, default 1. `channel`: whole number 0 to 16, default 0 (all). `smoothing`: 0 to 0.5 s, default 0.005 |
| Panel | Radio buttons for `source`, knobs |

| Source | Output |
|---|---|
| Controller | The value of controller number `controller`, from 0 to 1 (`value / 127`) |
| Pitch bend | The 14-bit bend, from −1 to 1, with the centre (8192) at 0 |
| Aftertouch | Channel pressure, from 0 to 1 |

- The output port is declared bipolar, as the widest case ([port ranges](port-ranges.md)). *Why:* a port's polarity is part of its static descriptor.
- **Bend is not scaled to semitones here.** The MIDI Note module already applies its bend range to pitch. This module gives the raw wheel.
- A value change takes effect at the event's sample offset; smoothing then runs from that sample, so the output is the same at any block size. Before any message arrives the output is 0 (bend centred).
- `channel` works as on MIDI Note: 0 listens to all 16, 1 to 16 to one.
- Per-note expression is [MT-IO-05](../product/module-catalogue.md), P2.

## Acceptance criteria

Every module also needs the checklist items in rule 6 above. The module-specific checks:

- [ ] Constant (level) at 0 dB outputs 1.0 and at −6 dB outputs 0.5012; Constant (pitch) at 12 semitones outputs 0.1. Existing `loom.math.constant` patches sound the same.
- [ ] Phase wrap and fold give the values in the rules for −0.25, 0, 0.25, 1, 1.25, and 2, and 0 for NaN and infinity.
- [ ] Compare hysteresis does not chatter on a signal with noise smaller than the band. Logic ignores unconnected inputs. Memory edges land on the same frame at block sizes 1, 77, and 512, and reset wins a tie.
- [ ] Mixer mute fades over 5 ms. Crossfade equal power gives 0.7071 of each side at mix 0.5, linear gives 0.5. Clamp swaps crossed limits and turns NaN into a value inside the limits. Scale inverts at factor −1.
- [ ] Sample and hold captures on the same frame at all three block sizes. Slew takes `time` seconds to rise by 1.0, and a new module starts at its input.
- [ ] A synced LFO with nothing connected wraps with the bar phase at block sizes 1, 77, and 512. A free LFO ignores a cable on `phase`.
- [ ] Envelope AD mode measures the attack and decay times, retriggers in attack and in decay without a click, and matches at all three block sizes.
- [ ] The one-pole outputs are 3 dB down at their cutoff. The ladder is about 12, 18, and 24 dB per octave steeper above the cutoff for each `slope`. At resonance 1, the 24 dB slope self-oscillates at the cutoff and the 18 dB slope at about 1.7 times it. The 18 dB check uses cutoffs below about 0.29 times the sample rate, because above that 1.7 times the cutoff is past Nyquist and the oscillation cannot appear. The 12 dB slope does not self-oscillate (its peak is about 4 dB above its own DC gain with feedback, not above unity). It stays finite at any input, and a `slope` change while playing is click-free and does not burst into oscillation.
- [ ] Controller input lands CC, bend, and aftertouch events on the sample of their offset at block sizes 1, 77, and 512.

## Out of scope

- Compare modes other than Above and Below; the Memory N-state latch; Mixer pan; Scale range-to-range; Constant ratio; LFO shape source, retrigger, and spread; AR and looping envelopes; ladder oversampling; per-note expression.

## Open points

- The Envelope `curve` and the new decay shape change how `adsr` sounds. Review the reference renders by ear in the pull request that builds it.
- Four inputs for Mixer and Logic is a tile-size choice. If four proves too few, add a second module rather than a variable port count (ports are static).
