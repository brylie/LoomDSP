# P2 Module Ports and Parameters Specification

The ports, parameters, ranges, defaults, and behaviour of the P2 modules that bring no new engine rules, so each can be built from its catalogue entry without guessing.

- **Kind:** living specification.
- **Status:** Agreed for Random, Chance, and Phase reset (2026-11-01). Each section is ready to build. Other P2 modules are added here as they are agreed.
- **Decision behind it:** none. It follows [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signals), [ADR-0014](../adr/0014-modulation-without-cables.md) (modulation without cables), [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) (port and parameter text), and [ADR-0018](../adr/0018-clean-room-module-development.md) (clean room).
- **Use case and requirements:** FR-LIB-01, FR-LIB-12, FR-LIB-22, NFR-SND-04.
- **Background:** [Phase and cycles](../learn/phase-and-cycles.md).
- **Prior art:** none named. Every module is designed from its purpose.
- **Issues:** [#304](https://github.com/brylie/LoomDSP/issues/304) (Random), [#305](https://github.com/brylie/LoomDSP/issues/305) (Chance), [#174](https://github.com/brylie/LoomDSP/issues/174) (Phase reset).
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
- [ ] One clock on lane 1 drives lanes 2 to 4 when they have no cable; lane 3's choices are the same whether or not lane 2 has its own cable (test).
- [ ] The same seed gives the same choices at block sizes 1, 77, and 512 (reference test).

Phase reset:

- [ ] A reset lands on the same sample at block sizes 1, 77, and 512 (test).
- [ ] After a reset, the output rises at the input's rate from 0 and wraps when it reaches 1 (test).
- [ ] A reset on the first frame counts (test).

## Out of scope

- The Smoothing trait itself (TR-11). Random uses a plain `glide` until it exists.
- Distributions other than Uniform and Centred.
- More than four lanes in Chance.

## Open points

None.
