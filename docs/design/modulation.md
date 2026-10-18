# Modulation Specification

How any output modulates any parameter without a cable, and how users make, see, and edit those assignments.

- **Kind:** living specification.
- **Status:** Editing and display [slice 1](#slice-1-make-see-and-edit) (make, see, and edit) and its [undo rules](#undo) are Agreed ([#70](https://github.com/brylie/LoomDSP/issues/70), [#20](https://github.com/brylie/LoomDSP/issues/20)). Slices 2 and 3 (live value, hover lines, matrix) are Draft ([#172](https://github.com/brylie/LoomDSP/issues/172), [#71](https://github.com/brylie/LoomDSP/issues/71)). The [engine rules](#rules), [engine and API](#engine-and-api), and [engine acceptance criteria](#acceptance-criteria-engine) are Agreed and built ([#19](https://github.com/brylie/LoomDSP/issues/19)). [Parameter mapping](#parameter-mapping) is Agreed.
- **Decision behind it:** [ADR-0014](../adr/0014-modulation-without-cables.md) (modulation without cables). Builds on [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (panels) and [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signals).
- **Requirements:** FR-MOD-01 to FR-MOD-08, FR-LIB-12.
- **Issues:** [#19](https://github.com/brylie/LoomDSP/issues/19) (built-in modulation), [#20](https://github.com/brylie/LoomDSP/issues/20) (range arcs and dots), [#66](https://github.com/brylie/LoomDSP/issues/66) (parameter mapping), [#70](https://github.com/brylie/LoomDSP/issues/70) (making and editing assignments), [#71](https://github.com/brylie/LoomDSP/issues/71) (matrix), [#151](https://github.com/brylie/LoomDSP/issues/151) and [#172](https://github.com/brylie/LoomDSP/issues/172) (slice 2), [#12](https://github.com/brylie/LoomDSP/issues/12) (undo).

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

The editing and display work ships in three slices, so each one can be built and tried on its own. Slice 1 is Agreed. Slices 2 and 3 are Draft.

| Slice | What it adds | Status | Issues |
|---|---|---|---|
| 1. Make, see, and edit | Drag an output onto a tile control, add from the inspector, edit in the inspector, see what each output drives in the inspector, a range arc on tile knobs, a badge on the source | **Agreed** | [#70](https://github.com/brylie/LoomDSP/issues/70) (making and editing), [#20](https://github.com/brylie/LoomDSP/issues/20) (arcs and dots) |
| 2. Live value and lines | The live value dot, lines on hover and pinned from the badge, dragging an arc to change depth | Draft | [#172](https://github.com/brylie/LoomDSP/issues/172), decided in [#151](https://github.com/brylie/LoomDSP/issues/151) |
| 3. Matrix | The modulation matrix, with the keyboard path | Draft | [#71](https://github.com/brylie/LoomDSP/issues/71) |

*Why slices:* slice 1 needs no engine change and no new kind of canvas drawing. Its range arcs, modulation dots (on toggles, radio buttons, and knobs whose arc has zero length or whose assignments are all off), and count badges use the arcs, round markers, and labels the canvas already draws. The live dot needs the engine to publish modulated values, and the matrix is a new panel. Neither should hold back the first way to make a patch with modulation.

### Slice 1: make, see, and edit

**Status:** Agreed.

#### Making an assignment

| Action | Result |
|---|---|
| Drag from an output port and drop on a control on a tile | Creates an assignment with the default depth (below), polarity following the source, and on. The new assignment is added after the others on that parameter. Dropping on an input port still makes a cable. Dropping anywhere else does nothing. |
| While that drag is over a control | The control is highlighted the way an input port is highlighted for a cable, so the user can see where the drop will go. |
| Inspector: **Add modulation** under a parameter, then pick a source | Creates the same assignment as a drop. The list shows every output at the current patch level as "module name #id: output label", in module id order, then port order. Outputs that already drive this parameter are greyed out and cannot be picked, by the same rule as a drop on a control. |

- Any control bound to a parameter accepts a drop: knobs, toggles, and radio buttons. Whole-number parameters round after the clamp (rule 5). A control bound to a display value (a light or meter) does not.
- The source can be on the same module as the target. That makes a self-loop, which is valid (rule 8).
- A drop that matches an assignment that already exists (same output, same parameter) adds nothing. The status bar says the assignment exists. *Why:* a second link from the same output only adds its depth to the first, so it is never needed, and two rows that look the same are confusing. The engine and the patch format still accept duplicates, so files written by hand keep loading.
- **Default depth:** 0.25, a quarter of the knob, or one step of a whole-number parameter if that is larger. One step is `1 / (max - min)` in the control's 0 to 1 range, so a toggle gets depth 1, a radio button with 3 options gets 0.5, and one with 8 options keeps 0.25. *Why:* 0.25 is large enough to hear on a continuous parameter and small enough not to slam to a limit. On a whole-number parameter it is not enough: the value rounds after the clamp (rule 5), so a toggle moved by 0.25 never changes, and the drop would seem to do nothing. One step lets a source at its peak move the value one step, with the change at half way. The 0.25 is one constant in the app, so it is easy to change after trying it with users.
- A unipolar assignment with a positive depth, on a control already at its maximum, only pushes into the clamp, so nothing moves. This holds for a toggle that is on. Set a negative depth to move it down. A bipolar assignment swings both ways, so it still moves the control. The spec does not flip the sign for the user, because the same drop must give the same assignment whatever the knob position.

#### Seeing assignments

- **On a tile knob:** one extra arc, in the modulation colour, shows the furthest the assignments can reach from the base. The knob's value arc and pointer still show the base value. The range runs from `base + low` to `base + high`, clamped to 0 to 1, where each enabled assignment adds to `low` and `high`:

  | Assignment uses | Adds to `low` | Adds to `high` |
  |---|---|---|
  | unipolar, depth `d` | `min(0, d)` | `max(0, d)` |
  | bipolar, depth `d` | `-abs(d)` | `abs(d)` |

  This is the most the assignments can reach, for sources inside their declared range (rules 2, 4, and 5). Two sources reach the ends together only when they peak at the same time. A source outside its range, such as a hot audio signal, can reach further before the clamp. The arc does not show that. On a whole-number parameter, each end is rounded the way the engine rounds the value: map the end back to the parameter with `ParamSpec::denormalize`, then to the control's range with `ParamSpec::normalize`. For a parameter from 0 to 10 with base 5 and one unipolar assignment at depth 0.25, the end is 8 (position 0.8), not position 0.75. *Why:* the engine rounds after the clamp (rule 5), so an unrounded arc would stop short of where the knob can go. The arc shows the parameter before any module input adds to it (rule 12).
- *Why one arc and not one per assignment:* a tile knob is small, and several arcs on it cannot be told apart. The combined range answers the question on stage: where can this knob go? The inspector shows each assignment.
- **On toggles and radio buttons:** no arc. A small dot in the modulation colour, next to the control, shows that it has at least one enabled assignment.
- **When a knob's arc has zero length:** the knob shows the dot instead, not dimmed. This happens when its enabled assignments cannot move it: depth 0, a depth too small to reach the next value of a whole-number knob (0 to 10 at base 5 with depth 0.04 still rounds to 5), or a unipolar assignment pushing into the clamp. *Why:* users set depth 0 to compare (rule 9), and the knob must not then look unmodulated.
- **When every assignment on a control is off:** a knob shows no range arc, and any control (knob, toggle, or radio button) shows the dot, dimmed. *Why:* turning assignments off is how users compare with and without modulation. The control must still say it has assignments, or they are easy to forget and hard to find again.
- **On the source:** an output port that drives at least one assignment shows a small label with the number of assignments, next to the port, counting those that are off. It does not cover the port or its cable. *Why:* the badge says where routing starts, so it must not hide routing that is only switched off.
- **In the inspector:** under each parameter's slider, one row per assignment, in the order the engine applies them. Each row shows the source ("module name #id: output label"), a depth slider from -1 to 1 with 0 in the middle and the depth as a signed percentage, a polarity choice with three options: **Follow source (bipolar)**, with the source's own polarity in brackets, **Unipolar**, and **Bipolar**, an on/off switch, and a remove button.
- **What an output drives:** when the source module is selected, the inspector shows **Drives** under its parameters, one group per output that drives at least one assignment. Each line names the target and its depth, as "module name #id: parameter label, +25%", in the order the assignments were added. A line for an assignment that is off is dimmed. Clicking a line selects the target module. Depth and the other settings are edited on the target's rows, not here. *Why:* the badge says how many targets an output has, but not which. Without this list, slice 1 has no way to find them from the source side. Editing stays in one place, so the two views cannot disagree.

#### Editing and removing

- Depth, polarity, and on/off are edited in the inspector row. They take effect at the next buffer and do not recompile (engine rule, see [Engine and API](#engine-and-api)).
- The remove button removes that assignment. It is the inspector's small **×** button ([removing an item](panels.md#removing-an-item)).
- Deleting a module removes the assignments it is part of. Its badge counts and arcs update.
- Saving writes assignments into the patch, as the engine already does.

#### Undo

Undo ([#12](https://github.com/brylie/LoomDSP/issues/12), [undo.md](undo.md)) is built. The app has no screen to make or change assignments yet, so today undo restores assignments with the module that held them, with their old ids. When that screen comes (slice 1), these rules apply:

- Adding, removing, and changing depth, polarity, or on/off are each an undo step. One depth slider drag is one step, not one per value, like a knob drag.
- Undoing the removal of an assignment, or of a module with assignments, brings each assignment back in its old place in the order. *Why:* several assignments on one parameter are added in id order, and float sums depend on that order (see [Engine and API](#engine-and-api)). Restoring at the end of the list could change the sound in the last bits. The controller needs a way to put an assignment back with its old id.

*Why not wait:* undo covers every edit, and building it was its own piece of work. Modulation did not wait for it.

### Slice 2: live value and lines

**Status:** Draft. The [proposal](#slice-2-proposal) below answers the open questions from [#151](https://github.com/brylie/LoomDSP/issues/151). It is not Agreed yet.

- **Live value:** a dot on the knob shows the modulated value (FR-MOD-04). The engine computes it per sample, so it must publish one value per block for the UI, much like display values. See [the proposal](#live-value).
- **On hover:** pointing at an output port that drives assignments, at its badge, or at a modulated control draws thin dashed lines between the source port and its targets, and highlights the tiles. They disappear when the pointer leaves. Hovering a port still shows its hover text. Selecting a module shows its lines too, following the same focus rules as cables.
- **Pinned lines:** clicking a badge keeps that output's lines visible after the pointer leaves. Clicking the badge again, or pressing Escape, hides them. The badge shows that it is pinned. *Why the badge and not the port:* pressing an output port starts dragging a cable or an assignment, so a click there must not also mean something else.
- **Drag an arc** on the control to change its depth. Shift for fine steps. With one combined arc, a drag must pick one assignment. See [the proposal](#arc-drag).
- **Drop on an inspector control.** The inspector is not part of the canvas, so a drag that starts on the canvas and ends there needs its own design. See [the proposal](#drop-on-the-inspector).

#### Slice 2 proposal

**Status:** Proposal for [#151](https://github.com/brylie/LoomDSP/issues/151). Each question lists the options and a recommendation. Once #151 accepts or changes them, this section replaces the open bullets above and slice 2 becomes Agreed.

##### Live value

The question: how does the engine publish one modulated value per parameter per block, without allocating or waiting on the audio thread?

| Option | How it works | Cost |
|---|---|---|
| A. A latest-value table, like display values | A table of atomics, allocated once with the engine. After each block, the engine stores each modulated parameter's value in its cell. `Controller::modulated(module, param)` loads it when the UI draws. | Small. Same pattern as `DisplayTable` (ADR-0013). Only the latest value is kept, which is all a dot needs. |
| B. A telemetry queue | Push `(module, param, value)` records into a bounded queue after each block, like meter readings. | More moving parts. Drops records when the UI is slow, and the UI has to keep its own latest-value map. A queue suits streams, such as scope chunks, not one latest number. |
| C. Compute it in the app | The app probes the sources and applies rules 2, 4, 5, and 7 itself. | Duplicates engine maths. It is wrong inside feedback loops, during smoothing, and where the compiler delays an assignment. The dot would lie. |

**Recommendation: A.**

- **Size.** The table has a fixed number of cells, set in the engine config next to `max_modules` (proposed default: 256). The compiler gives each modulated parameter a cell when it builds the schedule. If the patch has more modulated parameters than cells, the extra ones still sound right but show no dot, and the compiler reports it once as a warning. *Why a fixed size:* the table is allocated once, so no schedule swap ever allocates for it. 256 is far more than a stage patch uses.
- **Owner check.** Each cell stores the module ID and parameter index that own it, checked before and after reading, as display values do. *Why:* after a recompile, a cell may move to another parameter, and the old knob must not show the new value.
- **Which frame.** The last frame of the block. *Why:* it is the value the parameter has when the block ends, so it matches what display values do. An average would hide fast sources, and a peak would show values the base never sits at. At audio rate the dot jumps from frame to frame. That is honest, and the range arc already shows the span.
- **Which value.** The value after step 1 of rule 12, mapped to the control's 0 to 1 position with `ParamSpec::normalize`. So the dot is the engine's modulated value before any module input adds to it, the same thing the range arc shows.
- **How often the UI reads.** Once per drawn frame, for the visible knobs with assignments. The dot positions go into the canvas's `PanelValues` buffer, so a moving dot never rebuilds tiles, ports, or cables (ADR-0010).
- **No dot** when every assignment on the knob is off, or when the knob has no cell. Toggles and radio buttons show no live value in slice 2: their dot already says they are modulated, and a flickering state on a small control is hard to read.
- **ADR.** None needed if this stays a copy of the display table pattern. If review wants the table inside the schedule instead (no fixed size), that changes `loom-core` beyond the display pattern and needs an ADR.

##### Arc drag

The question: a knob shows one combined arc. With several assignments, which one does dragging the arc change?

| Option | Behaviour | Problem |
|---|---|---|
| A. Only with one assignment | With one enabled assignment, the drag changes it. With several, the drag does nothing, and the status bar says to use the inspector. | Clear, but useless on the knobs that need it most. |
| B. The last added | The drag changes the newest assignment. | Invisible: nothing on the knob says which one is newest, so the user changes the wrong one. |
| C. All, scaled together | The drag scales every depth by the same factor. | Mixed signs and polarities make the result hard to predict, and a depth at 0 never moves. |
| D. The focused source | If one source has its lines pinned (badge click) or its module selected, and it drives this knob, the drag changes that assignment. | Needs a source in focus, so it must fall back to something. |

**Recommendation: D, falling back to A.**

- The drag changes the assignment from the **pinned** source. If no source is pinned, it uses the **selected** module's output, when exactly one of its outputs drives this knob. If neither gives one, and the knob has exactly one enabled assignment, the drag changes that one. Otherwise the drag does nothing, and the status bar says "Several sources: pin one with its badge, or use the inspector". *Why:* pinning already means "show me this source's targets", so it is the natural way to say which source the drag is about. The user can see the choice, because the pinned lines run to this knob.
- **Where to grab.** The ring around the knob where the range arc is drawn, on any knob with at least one enabled assignment, including one whose arc has zero length (it shows a dot). Pressing the knob's body still sets the base value. *Why:* depth 0 is a common setting (rule 9), and the user must be able to drag back out of it.
- **The gesture.** Vertical drag, like the knob itself: up raises the depth, down lowers it, through 0 to negative. Shift for fine steps, at the same ratio as the knob's fine drag. Depth is clamped to -1 to 1. The status bar shows the source and the depth as a signed percentage while dragging. While dragging, the arc shows only the assignment being changed, so the user sees what moves.
- **Undo.** One drag is one undo step, like a depth slider drag in the inspector ([Undo](#undo)).
- **Toggles and radio buttons** have no arc, so there is nothing to drag. Their depth is set in the inspector.

##### Drop on the inspector

The question: can a drag that starts on an output port on the canvas end on an inspector parameter, and how is that shown?

| Option | How it works | Cost |
|---|---|---|
| A. An app-level drag | The canvas reports the assignment drag to the app while it is in progress (a new `CanvasEvent` with the source and the pointer position). Each inspector parameter row is wrapped in a `mouse_area`, so the app knows which row the pointer is over. On release over a row, the app creates the assignment. The row is highlighted like a tile control. | Needs a spike: Iced must keep sending pointer events to the canvas after the pointer leaves it, and `mouse_area` must report enter and exit while a button is held. Not checked yet. |
| B. Leave it out of P1 | The inspector already has **Add modulation** (slice 1), which reaches every parameter, including ones not on the tile. | No drag path to parameters that are not on the panel. |

**Recommendation: B for slice 2, and a spike for A.** *Why:* slice 1 already gives a way to modulate any parameter from the inspector, and the drop is the riskiest part of slice 2 for the least gain. If the spike shows Iced supports A cleanly, a follow-up adds it with these rules: same default depth and duplicate rule as a drop on a tile control, rows highlight while the drag is over them, and dropping elsewhere in the inspector does nothing.

##### Modulate mode

The question: bring back a mode where the user picks a source, then drags knobs to set depth?

| Option | Behaviour |
|---|---|
| A. No mode in P1 | Keep the plan. Depth is set by arc drag and the inspector. |
| B. A full mode | A toolbar or keyboard mode: pick a source, then any knob drag sets depth from that source, creating the assignment if needed. Leave the mode to go back to normal drags. |
| C. No mode, but pinning as focus | The arc drag recommendation above already uses the pinned source. A later step could let an arc drag on a knob that this source does not drive yet create the assignment. |

**Recommendation: A for slice 2, and decide C after trying it.** *Why:* a mode changes what every knob drag means, which is risky on stage, where a wrong drag is heard at once. Pinning gives most of the benefit without a mode, because the pinned lines show what the drag will affect. Try slice 2 with users first. If they keep pinning a source and then reaching for the inspector to add assignments, C is the next step.

##### Notes for building

- The lines are drawn by the canvas as cable-like strips, rebuilt only when the hovered or pinned source changes, not every frame (ADR-0010).
- Escape hides pinned lines first. Only when nothing is pinned does it do what it does today.

### Slice 3: the modulation matrix

**Status:** Draft.

A panel beside the inspector. Columns: on, source (module and output), target (module and parameter), depth, polarity. Sort and filter by module, or show only the selected module. Every cell can be edited by keyboard (NFR-UX-01). **Add**, then pick a source and a target, creates an assignment. Delete removes the selected row.

The matrix shows the assignments of the current patch level only. Assignments inside a sub-patch are not listed. *Why:* an assignment never crosses a patch level (rule 10), so the ones inside a sub-patch belong to it. Open the sub-patch to see them, as with its cables.

*Why a matrix as well as arcs:* arcs are best for one control, the matrix for the whole patch. It is also the place to check for hidden routing, and the keyboard path for people who do not use a mouse.

### Not planned for P1

- **Modulate mode (select a module, press M, drag controls).** Drag and drop and the inspector already cover the mouse path, and the matrix covers the keyboard path. It can come back if trying slice 1 shows that making many assignments is slow.

## Rules

1. **Any output can be a source.** LFOs, envelopes, macros, sequencers, MIDI values, and audio oscillators alike. *Why:* "patch anything to anything" (ADR-0007). Audio-rate modulation is a sound, not a mistake.
2. **Modulation works in the control's range.** The engine maps the base value to the control's 0 to 1 range, adds `depth × source` for each assignment, clamps to 0 to 1, and maps back. *Why:* depth 0.1 means "a tenth of the knob" on every parameter. On a logarithmic parameter such as cutoff, that is a musical interval, not a few hertz. The arc on the knob shows the range the engine can reach.
3. **One mapping, shared.** Whether a parameter is linear or logarithmic is part of its `ParamSpec`, not guessed by the app. The inspector, panel controls, and engine all use the same mapping functions in `loom-core`. *Why:* if they disagreed, the arc would lie.
4. **Polarity comes from the source's range, unless the user overrides it.** An output is unipolar when its nominal range has `min >= 0`, and bipolar otherwise ([port ranges](port-ranges.md), Tier 1). Envelopes, gates, and phases are unipolar; LFOs, audio, and pitch are bipolar. Only the polarity is used, not the span: a 0 to 5 source is treated like a 0 to 1 source. An assignment can force either, and the engine then remaps the source sample `s` before applying depth:

   | Source declares | Assignment uses | Value used |
   |---|---|---|
   | same as the assignment | (no override) | `s` |
   | bipolar | unipolar | `(s + 1) / 2` |
   | unipolar | bipolar | `2s - 1` |

   The range arc on the knob follows the value used: a unipolar assignment reaches from the base to `base + depth`, a bipolar one from `base - depth` to `base + depth`, and the arc combines them ([Seeing assignments](#seeing-assignments)). Sources are never clamped; only the final parameter value is (rule 5). *Why:* an envelope should push a cutoff up from its base, an LFO should swing around it. A fixed, simple remap keeps the arc honest.
5. **Sum, then clamp.** Several assignments on one parameter add up before the clamp (FR-MOD-03). Whole-number parameters are rounded after the clamp. *Why:* predictable, and the clamp keeps filters stable.
6. **Per sample, only where needed.** A parameter with at least one assignment gets a per-sample buffer. Others stay one value per block. *Why:* modulation must be smooth at audio rate (FR-MOD-03), but most parameters are still most of the time.
7. **Smoothing applies to the base value only.** When a parameter has assignments and its `ParamSpec` sets a smooth time, the engine smooths the base in the control's 0 to 1 position, then adds the modulation unsmoothed (FR-MOD-05). The module reads that buffer and does not smooth it again. A parameter with no assignments keeps the smoothing it already had inside the module, so a knob that is not modulated sounds the same as before. The time lives on `ParamOptions`, set with `ParamSpec::smooth`. Built-in times: the filter cutoff and resonance are 10 ms; the add offset, multiply gain, and constant value are 5 ms. Every other parameter is unsmoothed. *Why:* a sharp envelope or audio-rate source must stay sharp. Smoothing it would change the sound. Keeping the old path for unmodulated knobs means existing patches do not change.
8. **Assignments are graph edges.** The compiler orders the source before the target, and treats a loop through an assignment like any feedback loop: it inserts a one-sample delay where needed (ADR-0002). When a cycle must be broken, it delays the assignment before it delays a cable, so a modulation loop does not slip the audio path. A cycle of cables only still needs a cable delay, so the compiler breaks those first. Then it does not delay an assignment that the cable delay has already taken out of the loop. A module assigned to its own parameter is a self-loop and runs one sample at a time. *Why:* the engine has one way to schedule, and feedback through modulation is a valid patch.
9. **Depth 0 adds nothing.** On a patch where the assignment does not insert a delay on a cable, the output is bit-identical to the same patch without that assignment, as long as the knob is not gliding. While a smoothed knob glides, the engine glides it in the control's 0 to 1 position (rule 7), and the module glides an unmodulated knob in its own units. For the built-in modules both follow the same curve, but rounding can differ in the last bits until the glide ends. When the first assignment on a parameter is added during a glide, the engine starts its glide from the value the module reports through `Module::param_glide`, so the parameter does not step. A module that does not report it steps to the knob's new value. *Why:* users set depth to 0 to compare, and expect silence from it.
10. **Assignments stay inside their patch level.** Source and target must be in the same patch or sub-patch. To reach inside a sub-patch, expose the parameter (FR-ENCAP-03). *Why:* a sub-patch must work the same wherever it is used.
11. **Assignments are part of the patch.** They are saved, undone, copied with their modules (when both ends are copied), and exported. *Why:* they change the sound.
12. **Inputs and assignments add up, in a fixed order.** Some modules have an input for the same quantity as a parameter, such as the filter's cutoff input for keyboard tracking. The order is:
    1. The engine computes the modulated value: base, plus assignments in the 0 to 1 range, clamped, mapped back, rounded for whole numbers (rules 2 and 5).
    2. The module applies its input to that value in the input's own units (for the filter, pitch at 0.1 per octave), then clamps to the parameter range again.

    The module's help page states how its inputs apply. *Why:* both are common, and neither should silently replace the other. A fixed order means the same base, cable, and assignments give the same value in every module.

## Parameter mapping

**Status:** Agreed ([#66](https://github.com/brylie/LoomDSP/issues/66)).

Every parameter maps between its value and its control's position, from 0 to 1. Rules 2 and 3 use this mapping.

| Mapping | Position `t` to value | Use |
|---|---|---|
| Linear (the default) | `min + t × (max - min)` | Most parameters. Whole-number parameters are always linear. |
| Logarithmic | `min × (max / min)^t` | Frequencies and times. Equal steps of the control give equal ratios, so a tenth of a 20 Hz to 20 kHz knob is about one octave anywhere on it. |

- **The mapping is part of `ParamSpec`.** A `ParamSpec` with `ParamOptions::DEFAULT` is linear; `.logarithmic()` makes it logarithmic. Nothing is guessed from the unit. *Why:* the module author knows how the parameter should feel. A guess can be wrong, and then the arc lies.
- **`loom-core` has the two functions:** `ParamSpec::normalize(value)` gives the position, and `ParamSpec::denormalize(position)` gives the value. The inspector, panel controls, and the engine's modulation use only these. *Why:* rule 3.
- **Every result is valid.** `normalize` clamps the value into range and returns a position from 0 to 1. `denormalize` clamps the position to 0 to 1 and returns the value as `ParamSpec::clamp` gives it: in range, and rounded for whole numbers. NaN in either becomes the default value. *Why:* callers never need to clamp again.
- **The registry refuses impossible mappings:** a logarithmic parameter that is a whole number, or whose minimum is 0 or less. *Why:* there is no logarithm of 0, and whole numbers must step evenly, one value per step.
- **Built-in parameters in Hz or seconds are logarithmic:** the filter cutoff and the envelope's attack, decay, and release times. All others are linear.
- **The mapping is not saved in patch files.** Patches store values, not positions. *Why:* changing a parameter's mapping later never changes how a saved patch sounds.

### Acceptance criteria: parameter mapping

- [x] `denormalize(normalize(v))` returns `v` (within 1e-4, relative) at the minimum, default, and maximum of linear and logarithmic parameters.
- [x] For cutoff (20 Hz to 20 kHz), position 0.5 is about 632 Hz, and each step of 0.1 multiplies the value by about 2.
- [x] Whole-number parameters round, and step one value at a time across the whole range.
- [x] Out-of-range values, positions, and NaN give valid results.
- [x] The registry refuses a logarithmic whole-number parameter and a logarithmic parameter with a minimum of 0 or less.
- [x] Every built-in parameter in Hz or seconds is logarithmic.
- [x] The inspector uses `normalize` and `denormalize`. The app no longer guesses a mapping.

### Out of scope

- Decibel and stepped mappings (FR-PANEL-04, P2).
- Skewed or custom curves.
- The engine's modulation itself ([#19](https://github.com/brylie/LoomDSP/issues/19)). It will use these functions.

## Engine and API

**Status:** Agreed ([#19](https://github.com/brylie/LoomDSP/issues/19)).

- The graph holds a list of assignments: source `OutputRef`, target module and parameter index, depth (-1 to 1), optional polarity override, enabled. Each assignment has a stable id. Several assignments on one parameter are applied in id order. Patch files keep assignments in that order, and loading gives them ids in file order, so a reloaded patch adds them the same way. Float sums depend on order.
- Each output's polarity comes from its nominal `range` on its `PortSpec` ([port ranges](port-ranges.md)): unipolar when `min >= 0`, bipolar otherwise. Gate and phase conventions are 0 to 1. Audio, pitch, and modulation conventions are bipolar. An envelope output and a MIDI velocity output declare 0 to 1. Until port-ranges Tier 1 is built, outputs declare `polarity` directly on `PortSpec`, with the same results. The engine remaps a source sample only when the assignment overrides that declaration (rule 4).
- The compiler gives each modulated parameter a buffer of `max_block` samples, allocated with the schedule. No allocation on the audio thread.
- Before a target module runs, the engine fills its modulated parameter buffers from the base value (smoothed in control space when the parameter has a smooth time) and the source buffers. Inside a feedback loop the fill is one frame; otherwise it is the whole block. A delayed assignment reads the one-sample tap, not the live output.
- Modules read a modulated parameter through `PortIo` as a per-sample slice. `PortIo::param_at` gives the value at a frame. A parameter with no assignments stays the value from `set_param`, one per block.
- A module that smooths a parameter reports the value it is gliding through with `Module::param_glide`, or `None` once it has settled. The engine reads it on the audio thread when a schedule swap gives that parameter its first assignment, and starts its own glide there (rule 9).
- Adding or removing an assignment recompiles the graph. Changing depth, polarity, or enabled takes effect at the next buffer, like a parameter change, and does not allocate on the audio thread.
- Deleting a module removes the assignments it is part of.
- Patch format (RON), using the same `(module id, port or parameter name)` pairs as cables. Old files omit `modulation` and still load. `polarity` is omitted when the assignment follows the source. `enabled: false` is written only when the assignment is off. The format stays at version 1. *Why:* LoomDSP is a pre-release alpha, so no released build needs to refuse the new field. The cost: an older build loads a modulated patch without its assignments and plays it unmodulated. If that build then saves the file, the assignments are gone for good, because it writes only the fields it knows. Open modulated patches only in builds that support modulation. Bump the version for format changes once LoomDSP has a release.

```ron
modulation: [
    (source: (1, "out"), target: (2, "cutoff"), depth: 0.25),
    (source: (3, "out"), target: (2, "cutoff"), depth: 0.4, polarity: Unipolar),
]
```

## Example: a macro

A **Macro** module (FR-LIB-12) has one knob on its panel and one output equal to that knob. Assign its output to cutoff, resonance, and drive with different depths. One knob now opens the whole sound, and it can go on the front panel.

## P1 slice

### Acceptance criteria: engine

**Status:** Agreed ([#19](https://github.com/brylie/LoomDSP/issues/19)).

- [x] A constant source of 1.0 with depth 0.5 on a linear parameter with base 0 gives the middle of the range.
- [x] Depth 0.1 on cutoff (20 Hz to 20 kHz, logarithmic) moves it by a tenth of the knob, about one octave.
- [x] Two assignments sum before the clamp. The result never leaves the parameter range. Whole-number parameters stay whole.
- [x] Depth 0 output is bit-identical to no assignment, on a patch where the assignment does not delay a cable.
- [x] Output is the same at any block size, with assignments present.
- [x] Adding and removing assignments during playback allocates nothing on the audio thread (`no_alloc` test).
- [x] A loop through an assignment gets a one-sample delay and plays.
- [x] Assignments survive save and load. A patch saved before modulation still loads.
- [x] Deleting a source or target module removes its assignments.

### Acceptance criteria: slice 1

**Status:** Agreed. [#70](https://github.com/brylie/LoomDSP/issues/70), and [#20](https://github.com/brylie/LoomDSP/issues/20) for the arc and dot criteria.

- [ ] Dragging an output onto a tile control creates an assignment that follows the source's polarity, with depth 0.25 on a continuous parameter. Dropping on an input port still makes a cable.
- [ ] A drop on a toggle gives depth 1, and on a radio button with 3 options depth 0.5. With the toggle off, a unipolar source at 1 turns it on (test).
- [ ] Dropping the same output on the same parameter again adds nothing, and the status bar says why.
- [ ] **Add modulation** in the inspector creates the same assignment as a drop. Its list holds only outputs at the current patch level, and outputs that already drive the parameter are greyed out.
- [ ] The inspector lists each parameter's assignments in engine order. Depth, polarity, and on/off can be changed there, and the remove button removes the assignment.
- [ ] A modulated tile knob shows one range arc. For a given base and set of assignments, the arc's ends match the table in [Seeing assignments](#seeing-assignments) (unit test on the arc geometry), including the clamp at 0 and 1, leaving out disabled assignments, and rounding the ends of a whole-number parameter (0 to 10, base 5, unipolar depth 0.25 ends at position 0.8).
- [ ] A modulated toggle or radio button shows the modulation dot.
- [ ] A knob with enabled assignments whose arc has zero length (depth 0, a whole-number knob from 0 to 10 at base 5 with depth 0.04, or a unipolar assignment at the maximum) shows the dot, not dimmed, and no arc.
- [ ] With every assignment on a control turned off, the control shows the dimmed dot and no arc.
- [ ] Selecting a source module lists, under **Drives**, every assignment its outputs drive, with target and depth, dimming those that are off. Clicking a line selects the target module.
- [ ] An output that drives assignments shows their number, counting those that are off. The number updates when an assignment is added or removed, or a module is deleted.
- [ ] Changing depth, polarity, or on/off does not rebuild tiles, ports, or cables on the GPU (ADR-0010). Adding or removing an assignment may.
- [ ] Save, then load: the same assignments appear in the inspector, in the same order.

### Acceptance criteria: slices 2 and 3

**Status:** Draft. [#172](https://github.com/brylie/LoomDSP/issues/172), [#71](https://github.com/brylie/LoomDSP/issues/71).

- [ ] Modulated knobs show the live value.
- [ ] Hovering an output port, its badge, or a modulated control draws the lines. Clicking the badge pins them until it is clicked again or Escape is pressed.
- [ ] Proposed: the live value is the last frame of the block, before module inputs, and reading it does not allocate or wait on the audio thread (`tests/no_alloc.rs`).
- [ ] Proposed: a patch with more modulated parameters than table cells sounds the same, shows no dot on the extra knobs, and reports one warning.
- [ ] Proposed: dragging the arc changes the pinned source's assignment, or the only enabled one. With several and none in focus, it changes nothing and the status bar says why.
- [ ] Proposed: one arc drag is one undo step.
- [ ] The matrix lists every assignment in the current patch level, and every cell can be edited by keyboard.

### Acceptance criteria: undo

**Status:** Agreed. Checked when [#12](https://github.com/brylie/LoomDSP/issues/12) was built: the third is tested in `loom-app/src/undo/tests.rs`. The first two wait for a UI that makes assignments and changes their depth; undo covers them through the same snapshot difference.

- [ ] Undo removes an assignment that was just made. Redo brings it back.
- [ ] One depth slider drag is one undo step.
- [x] Undo restores the assignments removed by deleting a module, in their old order, and the output is bit-identical to before the delete.

### Out of scope for P1

- Modulating an assignment's depth (FR-MOD-02, P2).
- Per-voice modulation (with polyphony, P2).
- Response curves on assignments (for example exponential depth).
- Assignments across sub-patch boundaries. Not planned: expose the parameter instead.
- MIDI learn. It maps a controller to a base value, a separate feature ([MIDI learn](midi-learn.md), [#24](https://github.com/brylie/LoomDSP/issues/24)).

## Open points

- Should an assignment be able to target another assignment's depth directly, or through a "depth" parameter on a gain module? Not needed for P1. Decide with FR-MOD-02 (P2).
- Slice 2: how the engine publishes live modulated values, which assignment an arc drag changes, how a drag reaches the inspector, and modulate mode. Proposed answers are in [Slice 2 proposal](#slice-2-proposal), waiting for [#151](https://github.com/brylie/LoomDSP/issues/151).
- Try the default depth of 0.25 with users once slice 1 is built.
