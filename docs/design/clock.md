# Clock and Transport Specification

How musical time reaches every module: the patch transport, inputs that follow it without cables, and the modules that put time on cables and divide it.

- **Kind:** living specification.
- **Status:** Sections have their own status. **Implemented** for the engine slice ([#72](https://github.com/brylie/LoomDSP/issues/72)): the transport fields, signals derived from the transport, default inputs, rules 1 to 6 (except saving run state, which needs the Transport module), and engine and API. Also **Implemented**: the Clock Divide ([#48](https://github.com/brylie/LoomDSP/issues/48)) and Phase Trigger modules. The Transport ([#73](https://github.com/brylie/LoomDSP/issues/73)) and Clock ([#74](https://github.com/brylie/LoomDSP/issues/74)) modules, and rules 10 and 11, are **Agreed**. Phase Scale ([#31](https://github.com/brylie/LoomDSP/issues/31)) is **Implemented**; its `phase` input defaults to 0, like Phase Trigger. The Button's hold moves to a transient parameter (see Button details). The clock mark on ports is Agreed in [panels](panels.md#clock-mark-on-transport-inputs). The rest is Draft.
- **Decision behind it:** [ADR-0015](../adr/0015-one-transport-per-patch.md) (one transport per patch). Builds on [ADR-0007](../adr/0007-polymorphic-signal-model.md) (phase and gate signals) and [ADR-0012](../adr/0012-arrangement-from-phase-driven-primitives.md) (arrangement from phase).
- **Requirements:** FR-LIB-07, FR-LIB-16, FR-LIB-22, FR-LIB-25 to FR-LIB-27, FR-MIDI-07.
- **Background:** [Phase and cycles](../learn/phase-and-cycles.md) explains phase signals and how the end of a cycle is found.
- **Prior art:** VCV Rack clock modules (run and reset on the panel and as inputs), Bitwig's Button modulator.
- **Issues:** [#41](https://github.com/brylie/LoomDSP/issues/41) (transport and clock), [#72](https://github.com/brylie/LoomDSP/issues/72) (engine transport and default inputs), [#73](https://github.com/brylie/LoomDSP/issues/73) (Transport module), [#48](https://github.com/brylie/LoomDSP/issues/48) (Clock Divide), [#36](https://github.com/brylie/LoomDSP/issues/36) (LFO), [#47](https://github.com/brylie/LoomDSP/issues/47) (triggers).

## Goal

> I set the tempo to 96. My LFOs, sequencers, and loops all follow it, and I never patched a clock cable. Then I patch a half-speed phase into one sequencer, and it plays against the others.

## The transport

**Status:** the fields table is Implemented. "Controls live on the Transport module" and "Starting and stopping" are Agreed ([#73](https://github.com/brylie/LoomDSP/issues/73)).

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

There are no transport controls in the app toolbar. The Space key toggles Run when no panel control is focused, as a shortcut to the same control. When a panel control has focus, Space and Enter act on that control instead ([panels](panels.md#interaction-68)).

*Why on a module:* it follows ADR-0011 and ADR-0013. Time is controlled where you see it, it can go on a front panel, and it can be patched. Starting the clock from a pad, a sequencer, or a logic module is ordinary patching.

### Starting and stopping

| How | What happens |
|---|---|
| Click Run on the Transport panel, or press Space when no panel control is focused | Toggles running. |
| A gate into the run input | High runs, low stops. While the input is connected, it decides, and the Run toggle shows its state. |
| A trigger into reset, or the Reset button | Returns to bar 1 and fires the jump trigger. While stopped, the next start begins at bar 1. |

A **Button** module (FR-LIB-12) gives a performance control for this. Its panel has one large button, and its output is a gate. In toggle mode, each press flips the gate, like Bitwig's Button modulator; in momentary mode, the gate is high while held. Patch a toggle Button into the run input, put the Button on the front panel, and the performer starts and stops the piece from there.

*Why the run input is a gate, not a trigger:* a level cannot drift out of step. After a patch reload or a missed pulse, a toggle trigger could leave the clock stopped when the pad shows "on". With a gate, the input is always the truth.

## Signals derived from the transport

**Status:** Implemented. The rate phase row belongs to the Clock module ([#74](https://github.com/brylie/LoomDSP/issues/74)) and is Agreed.

| Signal | Value |
|---|---|
| Beat phase | `fract(position)` |
| Bar phase | `fract(position / beats_per_bar)`, with `beats_per_bar = numerator × 4 / denominator` |
| Rate phase | `fract(position / rate_in_beats)`, for a Clock module's rate |
| Playing | gate: 1.0 while playing, 0.0 while stopped |

Every phase is computed from the absolute position, never by adding up steps. *Why:* phases stay aligned with bar 1 after a stop, a tempo change, or a host jump, and two modules that compute the same rate always agree.

Phases are computed in `f64` and converted to `f32` last. A phase that rounds to 1.0 in `f32` becomes 0.0. *Why:* a phase is always in [0, 1). The wrap then comes less than one `f32` step early, far less than one sample.

## Default inputs

**Status:** Implemented, except the clock mark, which is Agreed and not built ([#103](https://github.com/brylie/LoomDSP/issues/103)).

A module can declare a **default** for any input. The default is used while nothing is connected.

| Default | Meaning |
|---|---|
| Value | A fixed number. Most inputs use 0. |
| Bar phase, beat phase | The transport's phase. |
| Playing | The transport's play gate. |

- Connecting a cable replaces the default for that module. Removing the cable brings it back.
- An input that is following the transport shows a small clock mark next to the port ([the mark](panels.md#clock-mark-on-transport-inputs)). *Why:* behaviour without a cable must still be visible.
- The engine computes each transport signal once per block into a shared buffer, and points unconnected inputs at it. *Why:* hundreds of synced modules cost one buffer, not hundreds.
- The default is part of the module's static descriptor. It does not change with the module's parameters. A module that uses an input only in some modes ignores it in the others. *Why:* the compiler binds inputs when the patch changes, not when a parameter moves, so binding stays off the audio thread.
- `PortIo::is_connected` still reports cables only. An input that follows its default reports false. *Why:* a module can still tell whether the user patched something, for example to show it or to pick a different behaviour.

Examples: the LFO's phase input in sync mode defaults to bar phase, with a rate parameter that scales it. Note Cycle and Audio Cycle default their bar phase input to the transport (FR-LIB-20).

## Modules

**Status:** Clock Divide ([#48](https://github.com/brylie/LoomDSP/issues/48)), Phase Trigger, and Button ([#75](https://github.com/brylie/LoomDSP/issues/75)) are Implemented; see their details below the table. Transport, Clock, and Phase Scale are Agreed, with details below.

| Module | Inputs | Outputs | Parameters | Behaviour |
|---|---|---|---|---|
| **Transport** `loom.clock.transport` | run (gate), reset (trigger) | bar phase, beat phase, playing (gate), bar (trigger), beat (trigger), jump (trigger) | tempo, numerator, denominator, run, reset (transient) | Exactly one per patch, and cannot be deleted. Its panel controls the transport (see above). Jump fires when the position moves by anything other than normal playback (a reset, a host loop). |
| **Clock** `loom.clock.clock` | none | phase, trigger, gate | rate, gate width | A phase at a musical rate: 4 bars, 2 bars, 1 bar, 1/2, 1/4, 1/8, 1/16, 1/32, each straight, dotted, or triplet. Trigger fires on each wrap. Gate is high for the first part of each cycle (gate width, 1 to 99 percent). |
| **Button** `loom.ui.button` | none | gate | mode (toggle, momentary), on, hold (transient) | A large button. Toggle flips `on` on each press and the gate is `on`. Momentary holds `hold` high while the button is held and the gate is `hold`. |
| **Clock Divide** `loom.logic.divide` | trigger, reset | trigger | N (1 to 64), offset (0 to N-1) | Counts rising edges. Fires on the triggers numbered N, 2N, 3N after a reset; offset k moves each firing k triggers earlier. With offset N-1 it fires on the first trigger. |
| **Phase Scale** `loom.phase.scale` | phase, reset | phase | multiply (1 to 16), divide (1 to 16) | Scales a phase by multiply ÷ divide, and stays locked to it. Divide counts the input's wraps, so a divide of 2 wraps once every two input cycles, counted from the module's first cycle or its last reset. |
| **Phase Trigger** `loom.phase.trigger` | phase | trigger | none | Fires a trigger each time the input phase wraps: forward from near 1 to near 0, or backward from near 0 to near 1. Turns any phase into triggers: a scaled phase, an LFO's phase, or the transport's bar phase. |

*Why both Clock Divide and Phase Scale:* triggers are events, for counting and arrangement logic (ADR-0012). Phases are continuous, for anything that should move smoothly and in sync: LFOs, sequencers, cycles. Each domain needs its own divider. Phase Scale also gives polyrhythm: multiply 3, divide 2 plays three against two.

*Why Phase Trigger:* it is how to multiply a clock. Phase Scale multiplies a phase, and Phase Trigger turns the result into triggers, for example to drive a Clock Divide or an envelope. A trigger-based multiplier, as in many VCV Rack modules, would have to measure the time between input triggers and guess when the next one comes. It would stumble on the first trigger and whenever the tempo changes. A multiplied phase is computed from the input, so it never guesses and stays locked. This follows Bitwig Grid, where timing is built from phase signals.

Trigger outputs are high for one sample, and trigger inputs count rising edges ([ADR-0007](../adr/0007-polymorphic-signal-model.md#triggers)).

### Clock Divide details

These details were settled while building the module.

- **Parameter names** are `n` (default 2) and `offset` (default 0). Both are whole numbers.
- **Offset above N-1.** The `offset` parameter ranges from 0 to 63, because a parameter's range is fixed in the descriptor. An offset of N or more counts as N-1. *Why:* offset N-1 means "fire on the first trigger", and lowering N keeps that meaning.
- **Reset and trigger on the same frame.** The reset comes first, so that trigger counts as the first one after the reset. *Why:* a loop's end trigger and a section reset often arrive together, and the new section must start counting at once.
- **Changing N.** The module counts every trigger since the reset, not the count modulo N. So after changing N, it fires as if N had been set at the reset.
- **No latency.** The output fires on the same frame as the input trigger.

### Phase Trigger details

- **What counts as a wrap.** The phase jumps by more than 0.5 from one frame to the next, in either direction. *Why:* a phase moving at any rate below half the sample rate changes by less than 0.5 per frame, so only a wrap can jump that far. A small wobble does not fire.
- **Both directions.** A phase running forward wraps from near 1 to near 0. One running backward wraps from near 0 to near 1. Both start a new cycle, so both fire.
- **No latency.** The trigger fires on the first frame of the new cycle. That frame is also the end of the old cycle, so this is the same frame an end-of-cycle trigger marks.
- **The first frame** fires only when the phase is exactly 0. *Why:* a phase that starts on a bar line marks bar 1, but a module added halfway through a cycle must not fire.
- **Jumps.** A reset to 0 from above 0.5, or any other jump of more than half a cycle, looks the same on the cable as a wrap, and fires once. A smaller jump does not fire.
- **Crossings and turnarounds.** To fire when a phase passes another position *p*, shift the phase by *-p* first, then use Phase Trigger. A ping-pong phase never wraps; take its turnarounds from the phase that drives it. [Phase and cycles](../learn/phase-and-cycles.md#phases-that-never-wrap) explains both with examples.
- **Shared rule.** The rule lives in `loom_core::signal::WrapDetector`. Every trigger that marks the end of a cycle should use it: Phase Trigger now, and later the Clock trigger, the Transport's bar and beat triggers, and the end trigger of cycles (FR-LIB-20). *Why:* they must all agree on where a cycle ends.

### Button details

**Status:** Agreed ([#75](https://github.com/brylie/LoomDSP/issues/75)). Code: `loom.ui.button` in `loom-modules/src/ui.rs`, and `WidgetKind::Button` and `ButtonMode` in `loom-core`.

- **Parameters.** `mode` is a choice: 0 Toggle (the default), 1 Momentary. `on` is a whole number, 0 or 1, default 0, and is saved: it is the toggle's latch. `hold` is a whole number, 0 or 1, default 0, and is [transient](transient-params.md): it is never saved. The output `gate` is `on` in Toggle mode and `hold` in Momentary mode. `process` reads `mode`. *Why:* the gesture decides which parameter changes, and the module decides which one counts. Keeping the hold out of the file means a patch saved while a button is held loads with the gate low. The registry requires `mode` to be a choice from 0 to 1, and `on` and `hold` to be different parameters from it and from each other.
- **No extra threshold.** A whole-number parameter is rounded after the clamp ([modulation](modulation.md), rule 5), so the module only sees 0 or 1. The gate is that value, with no smoothing.
- **Panel.** One button, 6 by 3 grid units, bound to `on` and `hold` (the widget writes the one that its `mode` says). Its fill shows the gate. Its caption is "On". `mode` stays in the inspector. *Why:* mode is a setup choice. A control for it on the tile would be easy to hit by mistake on a front panel.
- **The canvas reports the gesture.** A press, a release, or an arrow. It does not read `mode`. `ButtonMode` turns that report into a value. *Why:* the canvas reports input (ADR-0010). The same button widget can serve any module, including a later Transport reset.
- **Toggle.** A press flips `on`. Space or Enter presses it. Arrow keys set 0 or 1, as for a toggle. A release changes nothing. A double-click is two presses, not a reset. *Why press, not release:* the gate should land on the beat. Waiting for release would add the length of the press as delay.
- **Momentary.** A press sets `hold` to 1. A release sets it to 0, including after the pointer has left the button, when another mouse button is pressed, or when the window loses focus. Releasing a mouse button other than the left one does not end a hold the left button is still keeping. Space and Enter are separate holds: each keeps the button high while that key is down, releasing one leaves it high while the other is still down, and pressing either on a different button releases the button that key was holding. The pointer counts as a hold too, so lifting it leaves the gate high while a key is still down, and releasing a key leaves the gate high while the pointer is still down. Arrow keys do nothing. *Why the extra releases:* otherwise the gate stays high with nobody holding it.
- **The button does not drag the tile.** In both modes, pointer down on the button leaves the tile where it is. Drag the tile from its header.
- **Key repeat is ignored.** A held key does not flip the gate again.
- **Switching mode needs no clearing.** The two modes use different parameters, so a latched `on` does not show as a hold, and a hold does not change the latch. The gate follows the mode at once. *Why:* the old rule (clear `on` when the mode becomes Momentary) existed only because both modes shared one saved parameter ([#161](https://github.com/brylie/LoomDSP/issues/161)).
- **Space.** When a panel control has keyboard focus, Space and Enter act on that control and do not toggle Run. Space toggles Run only when no panel control is focused: the canvas reports `ToggleRun` and the app calls `Controller::set_running`. A repeated Space does not toggle again. Enter with nothing focused does nothing. *Why:* if both ran, a focused Button patched into run would be asked to change twice. The focused control is the one being played. The canvas consumes the key, so Run's handler, when the Transport module is built ([#73](https://github.com/brylie/LoomDSP/issues/73)), must not also see it.
- **A hold is never saved.** `hold` is transient, so a patch saved while a button is held loads with the gate low, and a hold does not mark the patch changed. A press and a release that land in the same buffer are applied in order, so a very short tap can be missed by the module; this is the commands queue's order and is out of scope.

## Rules

**Status:** rules 1 to 6 are Implemented in the engine; saving run state in the patch waits for the Transport module ([#73](https://github.com/brylie/LoomDSP/issues/73)). Rules 7 to 9 are Draft. Rules 10 and 11 are Agreed.

1. **One transport, owned by the engine.** Modules read it; only the transport source changes it. *Why:* every synced module must agree on where the bar starts.
2. **Position is double precision.** Phases are converted to `f32` only at the end. *Why:* in `f32`, position loses sample accuracy after a few minutes. An hour-long set must stay in step.
3. **Tempo changes keep the position.** A new tempo changes the speed from the next block on; phases never jump. *Why:* changing tempo while playing must not glitch loops or sequencers.
4. **The transport updates at block starts.** Inside a block, position advances by a fixed amount per sample. *Why:* blocks are at most `max_block` frames (2.7 ms at 48 kHz with 128), well under what anyone hears, and modules can compute phases in a tight loop.
5. **Nothing depends on block size.** The derived signals at a given sample are the same, bit for bit, for any block size. *Why:* block-size invariance (AGENTS.md hard rule 2). To make this hold, position is computed from an anchor, not by adding a step per block (see Engine and API).
6. **Run state is part of the patch.** A new patch starts running. Stopping holds the position, and synced modules hold their phase. Running again continues from there; Reset returns to bar 1. *Why:* a patch opens the way it was saved. Holding when stopped matches hardware clocks in VCV Rack and Eurorack: no clock, no movement, so stopping really stops the music.
7. **A host or MIDI clock source replaces the internal one completely.** Tempo, play state, and position come from it. *Why:* in a DAW, the patch must follow the song.
8. **Position jumps fire no skipped triggers.** When the position jumps, bar and beat triggers do not fire for the bars that were skipped, and the jump trigger fires once. *Why:* a host looping four bars must not burst out dozens of triggers.
9. **Sub-patches share the patch transport.** A sub-patch can take a different phase through an input. *Why:* one time base, but time can still be bent by patching.

10. **A time signature change is a jump, applied at once.** The engine applies it at the start of the next buffer, like any transport change. The position does not move; the bar grid does, so bar phase changes. The change counts as a jump: `jumped` is true for one call, the `jump` trigger fires once, and rule 8 applies, so no skipped bar triggers burst out. A stopped transport takes the change the same way. *Why:* it is the simplest rule that keeps every module honest. A module watching bar phase is told, instead of seeing a silent step. Applying it at the next bar line, as DAWs do, needs the engine to change the grid in the middle of a buffer, which breaks rule 4. A "wait for the bar" option can come later, from the control thread.
11. **Run and reset inputs are read on a fixed grid.** When the Transport's `run` or `reset` input has a cable, the engine splits every block at multiples of 32 frames, counted from engine start, and handles the transport at the start of each segment of at most 32 frames. The module reads the inputs during a segment, and an edge it finds takes effect at the start of the next segment. So an edge takes effect 1 to 32 frames later (0.7 ms at 48 kHz), at any block size, and every phase is bit-identical at block sizes 1, 32, and 1024. With neither input connected, the engine does not split. *Why not split at the edge:* the edge is not known until the modules that make it have run, and those modules may read the transport. *Why a fixed grid:* the delay depends on the frame count, not on the buffer, so rule 5 holds. *Why not a longer grid:* a stop must feel immediate on stage.

## Engine and API

**Status:** Implemented ([#72](https://github.com/brylie/LoomDSP/issues/72)), except the patch format, which belongs to [#73](https://github.com/brylie/LoomDSP/issues/73).

### Transport state

- The engine owns the transport. It is not part of the schedule, so editing the patch never moves it. *Why:* adding a module while playing must not restart the bar.
- A new engine starts running, at 120 BPM, in 4/4, at position 0.
- The control thread changes the transport through the `Controller`: set tempo, set time signature, set running, and reset. Each change goes through the command queue, like a parameter change, and takes effect at the start of the next buffer. *Why:* no locks, and one path for every change. The Transport module ([#73](https://github.com/brylie/LoomDSP/issues/73)) and the app will use the same calls.
- Tempo is clamped to 20 to 300 BPM, as a parameter is. A time signature out of range is refused with an error, and the transport does not change. *Why:* a clamped tempo is still a sensible tempo. There is no sensible nearest value for 7/3. `TransportInfo::new`, the constructor for testing modules alone, panics on an invalid time signature for the same reason; a NaN bar phase would hide the mistake.

### Position

- `beats_per_sample = tempo / 60 / sample_rate`, in `f64`.
- Position at a sample is `anchor_position + (sample - anchor_sample) × beats_per_sample`. `sample` counts frames since the engine started, as an integer. *Why:* adding a step per block rounds differently at different block sizes. With an anchor, a given sample gets the same position for any block size, bit for bit (rule 5), and rounding does not build up over an hour (rule 2).
- The anchor moves to the current sample and position whenever the tempo changes, the transport starts or stops, or the position is reset. So a tempo change keeps the position (rule 3).
- While stopped, the position does not move (rule 6).

### Jumps

- In P1, the only jump is a reset. Starting, stopping, and tempo changes are not jumps.
- A reset sets the position to 0. It counts as a jump at the first frame of the next buffer, whether the transport is running or stopped. A stopped transport stays stopped, and starts from bar 1 later.

### `ProcessContext`

Every call to `Module::process` gets the transport:

| Field | Type | Meaning |
|---|---|---|
| playing | `bool` | True while the transport runs. |
| tempo | `f64` | Beats per minute. |
| position | `f64` | Position, in beats, at the first frame of this call. |
| beats per sample | `f64` | How far the position moves per frame in this call. 0 while stopped. |
| beats per bar | `f64` | `numerator × 4 / denominator`. |
| jumped | `bool` | True when the position jumped at the first frame of this call. |

- Inside a feedback loop, each call is one frame. Position and jumped then describe that frame. Only the call for the first frame of the block can see `jumped`. *Why:* a module then sees the same values for any grouping of frames (rule 5).
- For the position at frame `i` of a call, a module uses `TransportInfo::position_at(i)` (or `bar_phase_at`, `beat_phase_at`). These compute from the anchor, exactly as the engine's shared buffers do. *Why:* adding `i × beats_per_sample` to the position rounds differently at different block sizes, which would break rule 5 inside the module.
- *Why beats per sample is 0 while stopped:* any position computed from it then holds still with no special case, as rule 6 asks.
- `ProcessContext::new`, used to test modules alone, gives a stopped transport at position 0, 120 BPM, 4/4. Tests can set another transport.

### `PortSpec` default

- A `PortSpec` gains a default: a value (`f32`), bar phase, beat phase, or playing.
- The default default is the value 0. *Why:* every existing module keeps its behaviour.

### Schedule

- The compiler lists the transport signals that at least one unconnected input uses, and gives each one buffer. The engine fills these buffers at the start of each block, before any module runs. *Why:* a signal that nothing uses costs nothing.
- An input with a value default other than 0 reads a constant buffer, one per distinct value, filled once when the schedule is compiled. Inputs with default 0 keep sharing the zero buffer.
- Filling the buffers is real-time safe: no allocation, no locks.

### Patch format

The Transport module's parameters (tempo, time signature, run) are saved like any module's ([#73](https://github.com/brylie/LoomDSP/issues/73)). Until then, a patch file does not store the transport.

## P1 slice

### Acceptance criteria: engine ([#72](https://github.com/brylie/LoomDSP/issues/72))

"Block size" below means both the buffer length passed to `Engine::process` and `max_block`.

- [x] After one hour at 120 BPM and 48 kHz, bar phase at the last sample matches the exact value within one sample. The exact value comes from integer arithmetic on the sample count.
- [x] Changing tempo while playing keeps the position continuous: no jump in any phase.
- [x] The bar phase, beat phase, and playing buffers are bit-identical at block sizes 1, 32, and 1024, over a run that includes a tempo change, a stop, a start, and a reset at the same samples.
- [x] An unconnected input with a bar phase default reads bar phase. A cable overrides it; removing the cable returns to the transport. `is_connected` is false while it follows the default.
- [x] An unconnected input with a value default reads that value.
- [x] Stopping holds the position and every phase. Starting continues from there.
- [x] Reset sets the position to 0, and `jumped` is true for exactly one call per module: the one that covers the first frame after the reset.
- [x] Out-of-range tempo is clamped. An out-of-range time signature is refused and changes nothing.
- [x] Many inputs that follow the same transport signal read one shared buffer.
- [x] The allocation tests (`loom-core/tests/transport_no_alloc.rs`, `loom-modules/tests/no_alloc.rs`) cover a patch with transport defaults and transport changes.

### Acceptance criteria: modules and UI (Draft)

- [ ] Clock at 1/4 in 4/4 fires four triggers per bar, in line with the bar phase wrap, at block sizes 1, 32, and 1024.
- [x] Clock Divide with N = 2, offset 0, fires on triggers 2, 4, 6. Offset 1 fires on 1, 3, 5. Reset starts the count again.
- [ ] Phase Scale with divide 2 wraps once per two input wraps, aligned with the input's bar. Multiply 3, divide 2 wraps three times per two input wraps.
- [x] Phase Trigger fires one sample on each wrap of its input phase, forward or backward, and on the first frame only when the phase is exactly 0. The output is the same for any block size.
- [ ] Phase Scale ([#31](https://github.com/brylie/LoomDSP/issues/31)) with multiply 4 into Phase Trigger fires four triggers per input cycle. The first is on the input's wrap. The output is the same at block sizes 1, 32, and 1024.
- [ ] An LFO in sync mode with nothing connected follows bar phase. A cable overrides it; removing the cable returns to the transport.
- [ ] Ports that follow the transport show the clock mark.
- [ ] Every patch has one Transport module. It cannot be deleted, and a second one cannot be added.
- [ ] Run on the Transport panel toggles running. Space does the same when no panel control is focused. A focused control takes Space for itself. Stopping holds synced phases; running continues from there.
- [x] Every Button gate sample is 0 or 1, and a toggle press flips `on`.
- [ ] The gate is `on` in Toggle mode and `hold` in Momentary mode. A momentary press sets `hold` to 1, and release sets it to 0, including when the pointer has left the button, when another mouse button is pressed, or when the window loses focus. Switching `mode` changes neither `on` nor `hold`. A patch saved while a momentary button is held loads with the gate low (needs [transient parameters](transient-params.md)).
- [ ] Reset returns to bar 1 and fires the jump trigger.
- [ ] A toggle Button patched into run starts and stops the transport. While connected, the Run toggle follows the input.
- [ ] Run state, tempo, and time signature survive save and load. `reset` is not saved.
- [ ] A patch without a Transport gets one on load, with a report. A patch with two fails with an error that names both. A second cannot be added, and the only one cannot be deleted.
- [ ] With a cable on `run` or `reset`, bar phase, beat phase, and playing are bit-identical at block sizes 1, 32, and 1024, and an edge takes effect 1 to 32 frames later (rule 11).
- [ ] A time signature change while playing sets `jumped` for one call, fires `jump` once, and fires no skipped bar triggers (rule 10), at block sizes 1, 32, and 1024.
- [ ] Removing a cable from `run` leaves the transport as it was, and the toggle works again.

### Out of scope for P1

- Host transport (with plugin export, later; FR-LIB-25 covers it once export exists).
- MIDI clock in and out (FR-MIDI-07, P2).
- Swing and shuffle.
- Tempo changes over time, such as modulation curves over bars (FR-LIB-16, P3).
- Tap tempo.

## Open points

- How tempo goes on a cable, if at all (a scale for BPM in the signal range).
- Swing: a Clock parameter, a separate module that bends a phase, or both.
- A "change at the next bar" option for the time signature, set from the control thread (rule 10).
