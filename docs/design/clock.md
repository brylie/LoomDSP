# Clock and Transport Specification

How musical time reaches every module: the patch transport, inputs that follow it without cables, and the modules that put time on cables and divide it.

- **Kind:** living specification.
- **Status:** Draft. Review it before building. The P1 slice becomes Agreed first.
- **Decision behind it:** [ADR-0015](../adr/0015-one-transport-per-patch.md) (one transport per patch). Builds on [ADR-0007](../adr/0007-polymorphic-signal-model.md) (phase and gate signals) and [ADR-0012](../adr/0012-arrangement-from-phase-driven-primitives.md) (arrangement from phase).
- **Requirements:** FR-LIB-07, FR-LIB-16, FR-LIB-22, FR-LIB-25 to FR-LIB-27, FR-MIDI-07.
- **Prior art:** VCV Rack clock modules (run and reset on the panel and as inputs), Bitwig's Button modulator.
- **Issues:** [#41](https://github.com/brylie/LoomDSP/issues/41) (transport and clock), [#48](https://github.com/brylie/LoomDSP/issues/48) (Clock Divide), [#36](https://github.com/brylie/LoomDSP/issues/36) (LFO), [#47](https://github.com/brylie/LoomDSP/issues/47) (triggers).

## Goal

> I set the tempo to 96. My LFOs, sequencers, and loops all follow it, and I never patched a clock cable. Then I patch a half-speed phase into one sequencer, and it plays against the others.

## The transport

The engine keeps one transport per patch.

| Field | Range | Default | Notes |
|---|---|---|---|
| Tempo | 20 to 300 BPM | 120 | Quarter-note beats per minute. |
| Time signature | 1 to 32 over 1, 2, 4, 8, 16 | 4/4 | Sets the length of a bar. |
| Running | on, off | on | Saved in the patch. See rule 6. |
| Position | beats, double precision | 0 | Quarter-note beats since bar 1. |
| Source | internal, host, MIDI clock | internal | Host in plugins; MIDI clock in P2. |

### Controls live on the Transport module

Every patch has exactly one **Transport** module, as it has one audio output. Its panel is the transport's control surface, as a clock module is in VCV Rack:

- a **Run** toggle, a **Reset** button, a **tempo** knob with a typed value, and the time signature;
- a **run** input (gate) and a **reset** input (trigger), so anything in the patch can start and stop time.

There are no transport controls in the app toolbar. The Space key toggles Run, as a shortcut to the same control.

*Why on a module:* it follows ADR-0011 and ADR-0013. Time is controlled where you see it, it can go on a front panel, and it can be patched. Starting the clock from a pad, a sequencer, or a logic module is ordinary patching.

### Starting and stopping

| How | What happens |
|---|---|
| Click Run on the Transport panel, or press Space | Toggles running. |
| A gate into the run input | High runs, low stops. While the input is connected, it decides, and the Run toggle shows its state. |
| A trigger into reset, or the Reset button | Returns to bar 1 and fires the jump trigger. While stopped, the next start begins at bar 1. |

A **Button** module (FR-LIB-12) gives a performance control for this. Its panel has one large button, and its output is a gate. In toggle mode, each press flips the gate, like Bitwig's Button modulator; in momentary mode, the gate is high while held. Patch a toggle Button into the run input, put the Button on the front panel, and the performer starts and stops the piece from there.

*Why the run input is a gate, not a trigger:* a level cannot drift out of step. After a patch reload or a missed pulse, a toggle trigger could leave the clock stopped when the pad shows "on". With a gate, the input is always the truth.

## Signals derived from the transport

| Signal | Value |
|---|---|
| Beat phase | `fract(position)` |
| Bar phase | `fract(position / beats_per_bar)`, with `beats_per_bar = numerator × 4 / denominator` |
| Rate phase | `fract(position / rate_in_beats)`, for a Clock module's rate |
| Playing | gate: high while playing |

Every phase is computed from the absolute position, never by adding up steps. *Why:* phases stay aligned with bar 1 after a stop, a tempo change, or a host jump, and two modules that compute the same rate always agree.

## Default inputs

A module can declare a **default** for any input. The default is used while nothing is connected.

| Default | Meaning |
|---|---|
| Value | A fixed number. Most inputs use 0. |
| Bar phase, beat phase | The transport's phase. |
| Playing | The transport's play gate. |

- Connecting a cable replaces the default for that module. Removing the cable brings it back.
- An input that is following the transport shows a small clock mark next to the port. *Why:* behaviour without a cable must still be visible.
- The engine computes each transport signal once per block into a shared buffer, and points unconnected inputs at it. *Why:* hundreds of synced modules cost one buffer, not hundreds.

Examples: the LFO's phase input in sync mode defaults to bar phase, with a rate parameter that scales it. Note Cycle and Audio Cycle default their bar phase input to the transport (FR-LIB-20).

## Modules

| Module | Inputs | Outputs | Parameters | Behaviour |
|---|---|---|---|---|
| **Transport** `loom.clock.transport` | run (gate), reset (trigger) | bar phase, beat phase, playing (gate), bar (trigger), beat (trigger), jump (trigger) | tempo, time signature, run | Exactly one per patch, and cannot be deleted. Its panel controls the transport (see above). Jump fires when the position moves by anything other than normal playback (a reset, a host loop). |
| **Clock** `loom.clock.clock` | none | phase, trigger, gate | rate, gate width | A phase at a musical rate: 4 bars, 2 bars, 1 bar, 1/2, 1/4, 1/8, 1/16, 1/32, each straight, dotted, or triplet. Trigger fires on each wrap. Gate is high for the first part of each cycle (gate width, 1 to 99 percent). |
| **Button** `loom.ui.button` | none | gate | mode (toggle, momentary) | A large button on its panel. Toggle flips the gate on each press; momentary is high while held. |
| **Clock Divide** `loom.logic.divide` | trigger, reset | trigger | N (1 to 64), offset (0 to N-1) | Counts rising edges. Fires on the triggers numbered N, 2N, 3N after a reset; offset k moves each firing k triggers earlier. With offset N-1 it fires on the first trigger. |
| **Phase Scale** `loom.phase.scale` | phase, reset | phase | multiply (1 to 16), divide (1 to 16) | Scales a phase by multiply ÷ divide, and stays locked to it. Divide counts the input's wraps, so a divide of 2 wraps once every two input cycles, starting on the same bar. |

*Why both Clock Divide and Phase Scale:* triggers are events, for counting and arrangement logic (ADR-0012). Phases are continuous, for anything that should move smoothly and in sync: LFOs, sequencers, cycles. Each domain needs its own divider. Phase Scale also gives polyrhythm: multiply 3, divide 2 plays three against two.

What a trigger output sends follows OQ-19 ([#47](https://github.com/brylie/LoomDSP/issues/47)).

## Rules

1. **One transport, owned by the engine.** Modules read it; only the transport source changes it. *Why:* every synced module must agree on where the bar starts.
2. **Position is double precision.** Phases are converted to `f32` only at the end. *Why:* in `f32`, position loses sample accuracy after a few minutes. An hour-long set must stay in step.
3. **Tempo changes keep the position.** A new tempo changes the speed from the next block on; phases never jump. *Why:* changing tempo while playing must not glitch loops or sequencers.
4. **The transport updates at block starts.** Inside a block, position advances by a fixed amount per sample. *Why:* blocks are at most `max_block` frames (2.7 ms at 48 kHz with 128), well under what anyone hears, and modules can compute phases in a tight loop.
5. **Nothing depends on block size.** The derived signals at a given sample are the same for any block size. *Why:* block-size invariance (AGENTS.md hard rule 2).
6. **Run state is part of the patch.** A new patch starts running. Stopping holds the position, and synced modules hold their phase. Running again continues from there; Reset returns to bar 1. *Why:* a patch opens the way it was saved. Holding when stopped matches hardware clocks in VCV Rack and Eurorack: no clock, no movement, so stopping really stops the music.
7. **A host or MIDI clock source replaces the internal one completely.** Tempo, play state, and position come from it. *Why:* in a DAW, the patch must follow the song.
8. **Position jumps fire no skipped triggers.** When the position jumps, bar and beat triggers do not fire for the bars that were skipped, and the jump trigger fires once. *Why:* a host looping four bars must not burst out dozens of triggers.
9. **Sub-patches share the patch transport.** A sub-patch can take a different phase through an input. *Why:* one time base, but time can still be bent by patching.

## Engine and API

- `ProcessContext` gains the transport for the block: playing, tempo, position at the first frame, beats per sample, beats per bar, and whether the position jumped.
- `PortSpec` gains a default: a value, or a transport signal.
- The schedule holds one buffer per transport signal in use, filled at the start of each block, and binds unconnected inputs with a transport default to it.
- Patch format: the Transport module's parameters (tempo, time signature, run) are saved like any module's.

## P1 slice

### Acceptance criteria

- [ ] After one hour at 120 BPM and 48 kHz, bar phase at the last sample matches the exact value within one sample.
- [ ] Changing tempo while playing keeps the position continuous: no jump in any phase.
- [ ] Clock at 1/4 in 4/4 fires four triggers per bar, in line with the bar phase wrap, at block sizes 1, 32, and 1024.
- [ ] Clock Divide with N = 2, offset 0, fires on triggers 2, 4, 6. Offset 1 fires on 1, 3, 5. Reset starts the count again.
- [ ] Phase Scale with divide 2 wraps once per two input wraps, aligned with the input's bar. Multiply 3, divide 2 wraps three times per two input wraps.
- [ ] An LFO in sync mode with nothing connected follows bar phase. A cable overrides it; removing the cable returns to the transport.
- [ ] Ports that follow the transport show the clock mark.
- [ ] Every patch has one Transport module. It cannot be deleted, and a second one cannot be added.
- [ ] Run on the Transport panel and Space both toggle running. Stopping holds synced phases; running continues from there.
- [ ] Reset returns to bar 1 and fires the jump trigger.
- [ ] A toggle Button patched into run starts and stops the transport. While connected, the Run toggle follows the input.
- [ ] Run state, tempo, and time signature survive save and load.

### Out of scope for P1

- Host transport (with plugin export, later; FR-LIB-25 covers it once export exists).
- MIDI clock in and out (FR-MIDI-07, P2).
- Swing and shuffle.
- Tempo changes over time, such as modulation curves over bars (FR-LIB-16, P3).
- Tap tempo.

## Open points

- How tempo goes on a cable, if at all (a scale for BPM in the signal range).
- Swing: a Clock parameter, a separate module that bends a phase, or both.
- Should Clock have a phase input of its own, so it can follow a bent phase instead of the transport?
