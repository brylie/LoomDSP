# Modulation Specification

How any output modulates any parameter without a cable, and how users make, see, and edit those assignments.

- **Kind:** living specification.
- **Status:** Editing and display [slice 1](#slice-1-make-see-and-edit) (make, see, and edit) and its [undo rules](#undo) are Agreed ([#70](https://github.com/brylie/LoomDSP/issues/70), [#20](https://github.com/brylie/LoomDSP/issues/20)). [Slice 2](#slice-2-live-value-and-lines) (live value, lines, arc drag) is Agreed and built ([#151](https://github.com/brylie/LoomDSP/issues/151), [#172](https://github.com/brylie/LoomDSP/issues/172)). The [inspector drop](#inspector-drop) is Agreed and not built yet ([#188](https://github.com/brylie/LoomDSP/issues/188)). Slice 3 (the matrix) is Draft ([#71](https://github.com/brylie/LoomDSP/issues/71)). The [engine rules](#rules), [engine and API](#engine-and-api), and [engine acceptance criteria](#acceptance-criteria-engine) are Agreed and built ([#19](https://github.com/brylie/LoomDSP/issues/19)). [Parameter mapping](#parameter-mapping) is Agreed.
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

The editing and display work ships in three slices, so each one can be built and tried on its own. Slices 1 and 2 are Agreed. Slice 3 is Draft.

| Slice | What it adds | Status | Issues |
|---|---|---|---|
| 1. Make, see, and edit | Drag an output onto a tile control, add from the inspector, edit in the inspector, see what each output drives in the inspector, a range arc on tile knobs, a badge on the source | **Agreed** | [#70](https://github.com/brylie/LoomDSP/issues/70) (making and editing), [#20](https://github.com/brylie/LoomDSP/issues/20) (arcs and dots) |
| 2. Live value and lines | The live value dot, lines on hover and pinned from the badge, dragging an arc to change depth | **Agreed** | [#172](https://github.com/brylie/LoomDSP/issues/172), decided in [#151](https://github.com/brylie/LoomDSP/issues/151) |
| 3. Matrix | The modulation matrix, with the keyboard path | Draft | [#71](https://github.com/brylie/LoomDSP/issues/71) |

*Why slices:* slice 1 needs no engine change and no new kind of canvas drawing. Its range arcs, modulation dots (on toggles, radio buttons, and knobs whose arc has zero length or whose assignments are all off), and count badges use the arcs, round markers, and labels the canvas already draws. The live dot needs the engine to publish modulated values, and the matrix is a new panel. Neither should hold back the first way to make a patch with modulation.

### Slice 1: make, see, and edit

**Status:** Agreed and built ([#70](https://github.com/brylie/LoomDSP/issues/70), [#20](https://github.com/brylie/LoomDSP/issues/20)).

#### Making an assignment

| Action | Result |
|---|---|
| Drag from an output port and drop on a control on a tile | Creates an assignment with the default depth (below), polarity following the source, and on. The new assignment is added after the others on that parameter. Dropping on an input port still makes a cable. Dropping anywhere else does nothing. |
| While that drag is over a control | The control is highlighted the way an input port is highlighted for a cable, so the user can see where the drop will go. |
| Inspector: **Add modulation** under a parameter, then pick a source | Creates the same assignment as a drop. The list shows every output at the current patch level as "module name #id: output label", in module id order, then port order. Outputs that already drive this parameter are greyed out and cannot be picked, by the same rule as a drop on a control. |

- Any control bound to a parameter accepts a drop: knobs, toggles, and radio buttons. Whole-number parameters round after the clamp (rule 5). A control bound to a display value (a light or meter) does not.
- The source can be on the same module as the target. That makes a self-loop, which is valid (rule 8).
- In MIDI learn mode, a drop on a control makes no assignment ([Inspector drop](#inspector-drop)).
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
- The keyboard focus ring and the MIDI learn ring go outside the range arc, never over it. *Why:* a keyboard user adjusting a modulated knob must still see where it can go.
- *Why one arc and not one per assignment:* a tile knob is small, and several arcs on it cannot be told apart. The combined range answers the question on stage: where can this knob go? The inspector shows each assignment.
- **On toggles and radio buttons:** no arc. A small dot in the modulation colour, next to the control, shows that it has at least one enabled assignment. A button bound to a parameter shows the same dot, because a patch file can assign to it even though a drop cannot.
- **When a knob's arc has zero length:** the knob shows the filled dot instead, not the ring. This happens when its enabled assignments cannot move it: depth 0, a depth too small to reach the next value of a whole-number knob (0 to 10 at base 5 with depth 0.04 still rounds to 5), or a unipolar assignment pushing into the clamp. *Why:* users set depth 0 to compare (rule 9), and the knob must not then look unmodulated.
- **When every assignment on a control is off:** a knob shows no range arc, and any control (knob, toggle, or radio button) shows a hollow ring in place of the dot: the same size and the same modulation colour, but not filled. *Why:* turning assignments off is how users compare with and without modulation. The control must still say it has assignments, or they are easy to forget and hard to find again. *Why a ring and not a dimmed dot:* a dot faded far enough to look different falls below 3:1 contrast with the tile (WCAG 2.1, 1.4.11), so it is easy to miss. The ring keeps the full contrast and differs by shape, which also works for colour-blind users. Filled for on and hollow for off is a common convention.
- **On the source:** an output port that drives at least one assignment shows a small label with the number of assignments, next to the port, counting those that are off. It does not cover the port or its cable. *Why:* the badge says where routing starts, so it must not hide routing that is only switched off.
- **In the inspector:** under each parameter's slider, one row per assignment, in the order the engine applies them. Each row shows the source ("module name #id: output label"), a depth slider from -1 to 1 with 0 in the middle and the depth as a signed percentage, a polarity choice with three options: **Follow source (bipolar)**, with the source's own polarity in brackets, **Unipolar**, and **Bipolar**, an on/off switch, and a remove button.
- **What an output drives:** when the source module is selected, the inspector shows **Drives** under its parameters, one group per output that drives at least one assignment. Each line names the target and its depth, as "module name #id: parameter label, +25%", in the order the assignments were added. A line for an assignment that is off is dimmed and ends with "(off)". *Why the word:* dimmed text alone can fall below the contrast small text needs (WCAG 1.4.3), and the state must not rest on colour alone. Clicking a line selects the target module. Depth and the other settings are edited on the target's rows, not here. *Why:* the badge says how many targets an output has, but not which. Without this list, slice 1 has no way to find them from the source side. Editing stays in one place, so the two views cannot disagree.

#### Editing and removing

- Depth, polarity, and on/off are edited in the inspector row. They take effect at the next buffer and do not recompile (engine rule, see [Engine and API](#engine-and-api)).
- The remove button removes that assignment. It is the inspector's small **×** button ([removing an item](panels.md#removing-an-item)).
- Deleting a module removes the assignments it is part of. Its badge counts and arcs update.
- Saving writes assignments into the patch, as the engine already does.

#### Undo

Undo ([#12](https://github.com/brylie/LoomDSP/issues/12), [undo.md](undo.md)) is built. Undo restores assignments with the module that held them, with their old ids. For the slice 1 edits, these rules apply:

- Adding, removing, and changing depth, polarity, or on/off are each an undo step. One depth slider drag is one step, not one per value, like a knob drag.
- Undoing the removal of an assignment, or of a module with assignments, brings each assignment back in its old place in the order. *Why:* several assignments on one parameter are added in id order, and float sums depend on that order (see [Engine and API](#engine-and-api)). Restoring at the end of the list could change the sound in the last bits. The controller needs a way to put an assignment back with its old id.

*Why not wait:* undo covers every edit, and building it was its own piece of work. Modulation did not wait for it.

### Slice 2: live value and lines

**Status:** Agreed ([#151](https://github.com/brylie/LoomDSP/issues/151)) and built ([#172](https://github.com/brylie/LoomDSP/issues/172)).

#### Live value

A dot on a modulated knob shows the value the engine gives the parameter (FR-MOD-04).

- **The engine publishes one value per modulated parameter per block** into a latest-value table, the same pattern as display values (ADR-0013). The table is a set of atomics, allocated once with the engine. After each block the engine stores each value in its cell. The UI loads it with `Controller::modulated(module, param)` when it draws. Nothing waits and nothing allocates on the audio thread. *Why not a telemetry queue:* a queue suits streams such as scope chunks. A dot needs only the latest number, and a queue would drop records when the UI is slow and make the UI keep its own map. *Why not compute it in the app:* the app would copy the engine's maths, and get it wrong inside feedback loops, during smoothing, and where the compiler delays an assignment.
- **Fixed size.** The table has a fixed number of cells, set in the engine config next to `max_modules`. The default is 256. The compiler gives each modulated parameter a cell when it builds the schedule. If a patch has more modulated parameters than cells, the extra ones still sound right but show no dot, and the compiler reports it once as a warning. *Why:* the table is allocated once, so no schedule swap allocates for it. 256 is far more than a stage patch uses. This is why no ADR is needed: it stays a copy of the display table pattern.
- **Owner check.** Each cell stores the module ID and parameter index that own it, checked before and after reading, as display values do. *Why:* after a recompile a cell may move to another parameter, and the old knob must not show the new value.
- **Which frame.** The last frame of the block. *Why:* it is the value the parameter has when the block ends, which matches display values. An average would hide fast sources, and a peak would show values the parameter only touches. At audio rate the dot jumps from frame to frame. That is honest, and the range arc already shows the span.
- **Which value.** The value after step 1 of rule 12, mapped to the control's 0 to 1 position with `ParamSpec::normalize`. So the dot is the engine's modulated value before any module input adds to it, the same thing the range arc shows.
- **Drawing.** The UI reads the values once per drawn frame, for knobs with enabled assignments, while the patch has one. The dot positions go into the canvas's `PanelValues` buffer, so a moving dot never rebuilds tiles, ports, or cables (ADR-0010).
- **No dot** when every assignment on the knob is off, or when the parameter has no cell. Toggles and radio buttons show no live value: their modulation dot already says they are modulated, and a flickering state on a small control is hard to read.

#### Lines

- **On hover:** pointing at an output port that drives assignments, at its badge, or at a modulated control draws thin dashed lines between the source port and its targets, and highlights the tiles. They disappear when the pointer leaves. Hovering a port still shows its hover text. Selecting a module shows its lines too, following the same focus rules as cables.
- **Pinned lines:** clicking a badge keeps that output's lines visible after the pointer leaves. Clicking the badge again, or pressing Escape, hides them. The badge shows that it is pinned. Only one output is pinned at a time: clicking another badge moves the pin. Escape hides pinned lines before anything else, except a drag in progress ([Inspector drop](#inspector-drop)). Only when nothing is pinned does it do what it does today. *Why the badge and not the port:* pressing an output port starts dragging a cable or an assignment, so a click there must not also mean something else. *Why one pin:* the pinned source also picks which assignment an arc drag changes (below), so it must be unambiguous.
- **Drawing.** The canvas draws the lines as cable-like strips. It rebuilds them when the hovered or pinned source, the selection, the assignments, or a module position changes, and never every frame. Pan and zoom only update the camera, like cables (ADR-0010).

#### Arc drag

Dragging the range arc on a tile knob changes the depth of one assignment.

- **Which assignment.** In this order:
  1. The pinned source's assignment on this knob, if it has one.
  2. Otherwise, if exactly one output of the selected module drives this knob, that assignment.
  3. Otherwise, if the knob has exactly one enabled assignment, that one.
  4. Otherwise the drag changes nothing, and the status bar says "Several sources: pin one with its badge, or use the inspector".

  If step 1 or 2 finds the source but it has more than one assignment on this knob, the drag changes nothing, and the status bar says to use the inspector. This only happens in patch files written by hand, because a drop never makes a duplicate (slice 1).

  *Why:* pinning already means "show me this source's targets", so it is the natural way to say which source a drag is about, and the pinned lines run to this knob, so the user can see the choice. *Why not the last added:* nothing on the knob says which one that is. *Why not scale them all:* with mixed signs and polarities the result is hard to predict, and a depth at 0 never moves.
- **Where to grab.** The ring around the knob where the range arc is drawn, on any knob with at least one enabled assignment. This includes a knob whose arc has zero length and shows a dot. Pressing the knob's body still sets the base value. *Why:* depth 0 is a common setting (rule 9), and the user must be able to drag back out of it.
- **The gesture.** Vertical drag, like the knob: up raises the depth, down lowers it, through 0 to negative. Shift gives fine steps, at the same ratio as the knob's fine drag. Depth is clamped to -1 to 1. While dragging, the status bar shows the source and the depth as a signed percentage, and the arc shows only the assignment being changed, so the user sees what moves.
- **Undo.** One drag is one undo step, like a depth slider drag in the inspector ([Undo](#undo)).
- **Toggles and radio buttons** have no arc, so there is nothing to drag. Their depth is set in the inspector.

#### Not in slice 2

- **Dropping an output on an inspector control.** Agreed, but built after slice 2, in [#188](https://github.com/brylie/LoomDSP/issues/188). See [Inspector drop](#inspector-drop).
- **Modulate mode.** See [Not planned for P1](#not-planned-for-p1).

### Inspector drop

**Status:** Agreed. Not built yet: [#188](https://github.com/brylie/LoomDSP/issues/188) builds it, after slice 2. The [spike in #186](https://github.com/brylie/LoomDSP/issues/186#issuecomment-6252045730) showed that Iced can carry the drag.

Dropping an output on a parameter row in the inspector makes an assignment, the same as a drop on a tile control. *Why:* the inspector shows every parameter, including ones that are not on the tile, and dragging is faster than picking from the **Add modulation** list.

- **What a drop makes.** The same as a drop on a tile control ([Making an assignment](#making-an-assignment)): the default depth, polarity following the source, on, and added after the others on that parameter. A drop that matches an assignment that already exists adds nothing, and the status bar says so. The drop is one undo step.
- **Which rows accept a drop.** The rows of parameters that a tile drop accepts: knobs, toggles, and radio buttons. Other rows do not highlight and ignore the drop. *Why:* the two drop targets must agree, or a parameter could be modulated from one place and not the other.
- **While dragging.** The row under the pointer is highlighted, the way a tile control is highlighted. Only one row is highlighted at a time. A drop anywhere else in the inspector does nothing. A drop on an input port still makes a cable, and a drop on a tile control follows slice 1.
- **The drag line.** The dragged cable is drawn across the whole window with the same shader as on the canvas, so it looks the same over the inspector. A window-wide layer on top of the app draws it with a second canvas primitive. It reports no mouse interaction, so the layers below keep their cursor and events. *Why a second primitive:* iced_wgpu clips a custom primitive to its widget, so the canvas cannot draw outside itself. *Why the same shader:* a line that changes look at the canvas edge reads as two different gestures. The layer's GPU data changes only while a drag is in progress, never every frame when nothing is dragged (ADR-0010).
- **Escape cancels any drag from an output**, a cable or an assignment, and so does the window losing focus. Nothing is made. A drag in progress takes Escape before anything else, such as pinned lines. *Why:* today Escape does not cancel a cable drag, so the only way out is to drop somewhere harmless. A drag the user can always back out of is safer on stage.
- **Release outside the window** makes nothing.
- **In MIDI learn mode**, dropping an output on a tile control or an inspector row makes no assignment, and nothing highlights. A drag from an output can still make a cable. *Why:* in learn mode, touching a control or row arms it for mapping ([MIDI learn](midi-learn.md)). A drop that also makes an assignment would give one gesture two meanings.

### Slice 3: the modulation matrix

**Status:** Draft.

A panel beside the inspector. Columns: on, source (module and output), target (module and parameter), depth, polarity. Sort and filter by module, or show only the selected module. Every cell can be edited by keyboard (NFR-UX-01). **Add**, then pick a source and a target, creates an assignment. Delete removes the selected row.

The matrix shows the assignments of the current patch level only. Assignments inside a sub-patch are not listed. *Why:* an assignment never crosses a patch level (rule 10), so the ones inside a sub-patch belong to it. Open the sub-patch to see them, as with its cables.

*Why a matrix as well as arcs:* arcs are best for one control, the matrix for the whole patch. It is also the place to check for hidden routing, and the keyboard path for people who do not use a mouse.

### Not planned for P1

- **Modulate mode (select a module, press M, drag controls).** Drag and drop and the inspector already cover the mouse path, and the matrix covers the keyboard path. A mode also changes what every knob drag means, which is risky on stage, where a wrong drag is heard at once. Decided in [#151](https://github.com/brylie/LoomDSP/issues/151). Pinning a source gives most of the benefit without a mode. After trying slice 2, decide whether an arc drag on a knob that the pinned source does not drive yet should create that assignment.

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

Text contrast, including off **Drives** lines, is checked by
[`text_colours_meet_wcag_minimum`](../../crates/loom-app/src/theme.rs).
Run `cargo test -p loom-app theme::tests` instead of repeating screenshot
contrast measurements in future manual checklists. Keep the layout, zoom
and interaction checks below.

- [x] Dragging an output onto a tile control creates an assignment that follows the source's polarity, with depth 0.25 on a continuous parameter. Dropping on an input port still makes a cable.
- [x] A drop on a toggle gives depth 1, and on a radio button with 3 options depth 0.5. With the toggle off, a unipolar source at 1 turns it on (test).
- [x] Dropping the same output on the same parameter again adds nothing, and the status bar says why.
- [x] **Add modulation** in the inspector creates the same assignment as a drop. Its list holds only outputs at the current patch level, and outputs that already drive the parameter are greyed out.
- [x] The inspector lists each parameter's assignments in engine order. Depth, polarity, and on/off can be changed there, and the remove button removes the assignment.
- [x] A modulated tile knob shows one range arc. For a given base and set of assignments, the arc's ends match the table in [Seeing assignments](#seeing-assignments) (unit test on the arc geometry), including the clamp at 0 and 1, leaving out disabled assignments, and rounding the ends of a whole-number parameter (0 to 10, base 5, unipolar depth 0.25 ends at position 0.8).
- [x] A modulated toggle or radio button shows the modulation dot.
- [x] A knob with enabled assignments whose arc has zero length (depth 0, a whole-number knob from 0 to 10 at base 5 with depth 0.04, or a unipolar assignment at the maximum) shows the filled dot, not the ring, and no arc.
- [x] With every assignment on a control turned off, the control shows the hollow ring and no arc.
- [x] Selecting a source module lists, under **Drives**, every assignment its outputs drive, with target and depth, dimming those that are off. Clicking a line selects the target module.
- [x] An output that drives assignments shows their number, counting those that are off. The number updates when an assignment is added or removed, or a module is deleted.
- [x] Changing depth, polarity, or on/off does not rebuild tiles, ports, or cables on the GPU (ADR-0010). Adding or removing an assignment may.
- [x] Save, then load: the same assignments appear in the inspector, in the same order.

### Acceptance criteria: slice 2

<a id="acceptance-criteria-slices-2-and-3"></a>

**Status:** Agreed ([#151](https://github.com/brylie/LoomDSP/issues/151)). Checked in [#172](https://github.com/brylie/LoomDSP/issues/172): engine tests in `loom-modules/tests/modulation.rs`, canvas tests in `loom-canvas/src/widget.rs` and `controls.rs`, app tests in `loom-app/src/main.rs` and `scene.rs`.

- [x] Modulated knobs show the live value: the last frame of the block, before module inputs.
- [x] Publishing and reading live values does not allocate or wait on the audio thread (`tests/no_alloc.rs`).
- [x] A patch with more modulated parameters than table cells sounds the same, shows no dot on the extra knobs, and reports one warning.
- [x] After a recompile moves a cell to another parameter, the old knob never shows the new parameter's value.
- [x] Hovering an output port, its badge, or a modulated control draws the lines. Clicking the badge pins them until it is clicked again, another badge is clicked, or Escape is pressed.
- [x] Dragging the arc changes the pinned source's assignment, then the selected module's, then the only enabled one. With several and none in focus, it changes nothing and the status bar says why.
- [x] An arc drag works on a knob whose arc has zero length.
- [x] One arc drag is one undo step.

### Acceptance criteria: inspector drop

**Status:** Agreed. [#188](https://github.com/brylie/LoomDSP/issues/188).

- [ ] Dropping an output on a continuous parameter row makes an assignment with depth 0.25 that follows the source's polarity (test).
- [ ] Dropping on a toggle parameter row gives depth 1 (test).
- [ ] Dropping the same output on the same row again adds nothing, and the status bar says why (test).
- [ ] Rows of parameters that a tile drop refuses do not highlight and ignore a drop (test).
- [ ] Only the row under the pointer is highlighted during the drag (manual check).
- [ ] The dragged cable looks the same over the canvas and over the inspector, and nothing is rebuilt on the GPU every frame when nothing is dragged (manual check, frame meter).
- [ ] Escape and window focus loss cancel a cable drag and an assignment drag, and make nothing (test).
- [ ] A release over empty canvas or outside the window makes nothing (test; manual check outside the window on macOS, Windows, and Linux).
- [ ] In MIDI learn mode, a drop on a tile control or a row makes no assignment (test).
- [ ] The drop is one undo step (test).

### Acceptance criteria: slice 3

**Status:** Draft. [#71](https://github.com/brylie/LoomDSP/issues/71).

- [ ] The matrix lists every assignment in the current patch level, and every cell can be edited by keyboard.

### Acceptance criteria: undo

**Status:** Agreed. The third was tested when [#12](https://github.com/brylie/LoomDSP/issues/12) was built, in `loom-app/src/undo/tests.rs`. The first two are tested in `loom-app/src/main.rs` since [#70](https://github.com/brylie/LoomDSP/issues/70).

- [x] Undo removes an assignment that was just made. Redo brings it back.
- [x] One depth slider drag is one undo step.
- [x] Undo restores the assignments removed by deleting a module, in their old order, and the output is bit-identical to before the delete.

### Out of scope for P1

- Modulating an assignment's depth (FR-MOD-02, P2).
- Per-voice modulation (with polyphony, P2).
- Response curves on assignments (for example exponential depth).
- Assignments across sub-patch boundaries. Not planned: expose the parameter instead.
- MIDI learn. It maps a controller to a base value, a separate feature ([MIDI learn](midi-learn.md), [#24](https://github.com/brylie/LoomDSP/issues/24)).

## Open points

- Should an assignment be able to target another assignment's depth directly, or through a "depth" parameter on a gain module? Not needed for P1. Decide with FR-MOD-02 (P2).
- Try the default depth of 0.25 with users once slice 1 is built.
