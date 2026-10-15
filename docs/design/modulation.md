# Modulation Specification

How any output modulates any parameter without a cable, and how users make, see, and edit those assignments.

- **Kind:** living specification.
- **Status:** Draft. Review it before building. The P1 slice becomes Agreed first.
- **Decision behind it:** [ADR-0014](../adr/0014-modulation-without-cables.md) (modulation without cables). Builds on [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (panels) and [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signals).
- **Requirements:** FR-MOD-01 to FR-MOD-08, FR-LIB-12.
- **Issues:** [#19](https://github.com/brylie/LoomDSP/issues/19) (built-in modulation), [#20](https://github.com/brylie/LoomDSP/issues/20) (modulation range on controls).

## Goal

> Two LFOs, an envelope, and a macro knob shape the filter, the oscillator mix, and the delay time. The patch still shows the audio path clearly, and I can see at a glance what moves what.

## Terms

| Term | Meaning |
|---|---|
| **Assignment** | One link from an output (the source) to a parameter (the target), with a depth and a polarity. |
| **Base value** | The parameter's value set by its control, before modulation. |
| **Depth** | How far the source moves the parameter, from -1 to 1, as a share of the control's full range. |
| **Modulation matrix** | A table of every assignment in the current patch or sub-patch. |

## Behaviour

### Making an assignment

| Action | Result |
|---|---|
| Drag from an output port and drop on a control (on a tile or in the inspector) | Creates an assignment with depth 0.25. Dropping on a port still makes a cable. |
| Select a module, press **M** (modulate mode), then drag any control | Sets that module's first output as a source on the dragged control. The drag sets the depth. Press M or Escape to leave the mode. |
| Matrix: **Add**, then pick a source and a target | Creates an assignment from the table, by mouse or keyboard. |

### Seeing assignments

- **On the control:** the base value, an arc for each assignment's range, and a dot for the live value (FR-MOD-04). Unipolar assignments draw the arc on one side of the base. Bipolar assignments draw it on both sides.
- **On the source:** a small badge on the output port shows how many assignments it drives.
- **On hover:** pointing at a modulated control, or at a source badge, draws thin dashed lines between the source port and its targets, and highlights the tiles. They disappear when the pointer leaves. Selecting a module shows its lines too, following the same focus rules as cables.
- **In the inspector:** each parameter lists its assignments, with a depth slider and a remove button.
- **In the matrix:** every assignment, one per row.

### Editing and removing

- Drag an assignment's arc on the control to change its depth. Shift for fine steps.
- Right-click an arc, or press Delete in the matrix, to remove it.
- Deleting a module removes the assignments it is part of. Undo brings them back.

### The modulation matrix

A panel beside the inspector. Columns: on, source (module and output), target (module and parameter), depth, polarity. Sort and filter by module, or show only the selected module. Every cell can be edited by keyboard (NFR-UX-01).

*Why a matrix as well as arcs:* arcs are best for one control, the matrix for the whole patch. It is also the place to check for hidden routing, and the keyboard path for people who do not use a mouse.

## Rules

1. **Any output can be a source.** LFOs, envelopes, macros, sequencers, MIDI values, and audio oscillators alike. *Why:* "patch anything to anything" (ADR-0007). Audio-rate modulation is a sound, not a mistake.
2. **Modulation works in the control's range.** The engine maps the base value to the control's 0 to 1 range, adds `depth × source` for each assignment, clamps to 0 to 1, and maps back. *Why:* depth 0.1 means "a tenth of the knob" on every parameter. On a logarithmic parameter such as cutoff, that is a musical interval, not a few hertz. The arc on the knob shows exactly what the engine does.
3. **One mapping, shared.** Whether a parameter is linear or logarithmic is part of its `ParamSpec`, not guessed by the app. The inspector, panel controls, and engine all use the same mapping functions in `loom-core`. *Why:* if they disagreed, the arc would lie.
4. **Polarity comes from the source, unless the user overrides it.** Each output declares unipolar (0 to 1) or bipolar (-1 to 1). Defaults: envelopes, gates, and phases are unipolar; LFOs, audio, and pitch are bipolar. An assignment can force either. *Why:* an envelope should push a cutoff up from its base, an LFO should swing around it.
5. **Sum, then clamp.** Several assignments on one parameter add up before the clamp (FR-MOD-03). Whole-number parameters are rounded after the clamp. *Why:* predictable, and the clamp keeps filters stable.
6. **Per sample, only where needed.** A parameter with assignments, or one that is being smoothed, gets a per-sample buffer. Others stay one value per block. *Why:* modulation must be smooth at audio rate (FR-MOD-03), but most parameters are still most of the time.
7. **Smoothing applies to the base value only.** Changes from the UI and MIDI are smoothed (FR-MOD-05). The modulation itself is not. *Why:* a sharp envelope or audio-rate source must stay sharp. Smoothing it would change the sound.
8. **Assignments are graph edges.** The compiler orders the source before the target, and treats a loop through an assignment like any feedback loop: it inserts a one-sample delay where needed (ADR-0002). *Why:* the engine has one way to schedule, and feedback through modulation is a valid patch.
9. **Depth 0 sounds the same as no assignment.** Output is bit-identical. *Why:* users set depth to 0 to compare, and expect silence from it.
10. **Assignments stay inside their patch level.** Source and target must be in the same patch or sub-patch. To reach inside a sub-patch, expose the parameter (FR-ENCAP-03). *Why:* a sub-patch must work the same wherever it is used.
11. **Assignments are part of the patch.** They are saved, undone, copied with their modules (when both ends are copied), and exported. *Why:* they change the sound.
12. **Inputs and assignments add up.** Some modules have an input for the same quantity as a parameter, such as the filter's cutoff input for keyboard tracking. The input and the assignments both apply. *Why:* both are common, and neither should silently replace the other.

## Engine and API

- The graph holds a list of assignments: source `OutputRef`, target module and parameter index, depth, polarity, enabled.
- The compiler gives each modulated parameter a buffer of `max_block` samples, allocated with the schedule. No allocation on the audio thread.
- Before a target module runs, the engine fills its modulated parameter buffers from the base value (smoothed) and the source buffers.
- Modules read parameters through `PortIo`, as either a constant for the block or a per-sample slice. A helper gives the value at a frame index for code that does not care which.
- Patch format (RON):

```ron
modulation: [
    (source: "lfo1.out", target: "filter1.cutoff", depth: 0.25),
    (source: "env1.out", target: "filter1.cutoff", depth: 0.4, polarity: Unipolar),
]
```

## Example: a macro

A **Macro** module (FR-LIB-12) has one knob on its panel and one output equal to that knob. Assign its output to cutoff, resonance, and drive with different depths. One knob now opens the whole sound, and it can go on the front panel.

## P1 slice

### Acceptance criteria

- [ ] A constant source of 1.0 with depth 0.5 on a linear parameter with base 0 gives the middle of the range.
- [ ] Depth 0.1 on cutoff (20 Hz to 20 kHz, logarithmic) moves it by a tenth of the knob, about one octave.
- [ ] Two assignments sum before the clamp. The result never leaves the parameter range. Whole-number parameters stay whole.
- [ ] Depth 0 output is bit-identical to no assignment.
- [ ] Output is the same at any block size, with assignments present.
- [ ] Adding and removing assignments during playback allocates nothing on the audio thread (`no_alloc` test).
- [ ] A loop through an assignment gets a one-sample delay and plays.
- [ ] Dragging an output onto a control creates an assignment. Undo removes it.
- [ ] Modulated controls show base, range arc, and live value.
- [ ] Hovering a modulated control draws its source lines. The source port shows a badge.
- [ ] The matrix lists every assignment, and depth can be edited by keyboard.
- [ ] Assignments survive save and load.
- [ ] Deleting a source module removes its assignments. Undo restores them.

### Out of scope for P1

- Modulating an assignment's depth (FR-MOD-02, P2).
- Per-voice modulation (with polyphony, P2).
- Response curves on assignments (for example exponential depth).
- Assignments across sub-patch boundaries. Not planned: expose the parameter instead.
- MIDI learn. It maps a controller to a base value, a separate feature ([#24](https://github.com/brylie/LoomDSP/issues/24)).

## Open points

- The M key and the default depth of 0.25 are guesses. Try them with users.
- Should an assignment be able to target another assignment's depth directly, or through a "depth" parameter on a VCA-style module? Decide with FR-MOD-02.
- How the matrix shows assignments inside sub-patches: hidden, or listed read-only under the sub-patch.
