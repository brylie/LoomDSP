# Undo and Redo Specification

How a user takes back and replays edits on the canvas, the inspector, and the panels, so that experiments are cheap.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** none of its own yet. Builds on [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (all edits go through `Controller`). A short ADR is needed only if the `loom-core` change under [Engine support](#engine-support) turns out larger than one method (see [Open points](#open-points)).
- **Use case and requirements:** FR-CANVAS-04. Related: FR-LIB-19 (module data is covered by undo, built later).
- **Background:** none.
- **Prior art:** every editor has undo. The only conventions we use are the common shortcuts and "one drag is one step".
- **Issues:** [#12](https://github.com/brylie/LoomDSP/issues/12). Rules this spec collects from elsewhere: [modulation undo](modulation.md#undo), [MIDI learn](midi-learn.md) (out of scope list), [removing an item](panels.md#removing-an-item).
- **Code:** not built. Expected: an edit log in `crates/loom-app`, a restore method on `Controller` in `loom-core`, and gesture events in `loom-canvas`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

> I drag a cable into the wrong input, delete three modules by mistake, and turn the cutoff knob too far. I press undo three times and the patch is as it was. I press redo and my last change comes back. A long knob drag is one step, not fifty.

## Terms

| Term | Meaning |
|---|---|
| **Edit** | One change to the patch that the user makes: add a module, turn a knob, and so on. |
| **Step** | One entry in the history. Undo takes back one step; redo replays one step. A step can hold several changes that the user sees as one action. |
| **Gesture** | A user action that sends many events and ends at a known moment: a knob drag, a depth slider drag, a module move drag. One gesture is one step. |
| **History** | The list of steps, with a position. Steps before the position can be undone; steps after it can be redone. |

## Behaviour

### Commands

| Action | Result |
|---|---|
| **Ctrl+Z** (**Cmd+Z** on macOS) | Undo the last step. |
| **Ctrl+Shift+Z** (**Cmd+Shift+Z**), or **Ctrl+Y** on Windows and Linux | Redo the step that was undone last. |
| **Undo** and **Redo** buttons in the toolbar | The same. A button is disabled when there is nothing to do. Its hover text names the step, for example **Undo: move 2 modules**. |
| Make any new edit after undoing | The steps that could be redone are dropped. |
| Press undo or redo while a gesture is in progress | Nothing happens. The gesture finishes first. |
| Open a patch, or start a new one | The history is cleared. |

The shortcuts work when the canvas or the inspector has focus, and do nothing while a text field is being edited (the field has its own undo).

### What is undoable

| Edit | One step is |
|---|---|
| Add a module | The add. Undo removes the module. |
| Delete modules | The whole delete, however many modules are selected. It includes the cables, modulation assignments, and MIDI mappings that went with them. |
| Move modules | One drag, for all selected modules together. A drag that ends where it started is not a step. |
| Connect | The cable. If it replaced a cable into the same input, undo brings the old cable back. |
| Disconnect | The removed cable. |
| Change a parameter by a knob, slider, or radio control, on a tile or in the inspector | One gesture. A single click or key step on a control is a gesture of its own. A gesture that ends at its starting value is not a step. |
| Add or remove a modulation assignment | The add or the remove. |
| Change an assignment's depth, polarity, or on/off | One gesture, like a parameter change. |
| Map a MIDI control by learning | The new mapping, **and** the parameter value the control change set, together. If the mapping replaced another mapping on the same parameter or the same control, undo brings the old one back. |
| Move a mapping to another parameter | The move. |
| Remove a MIDI mapping (the small × button) | The removal. |
| Reset a module | The reset, with every parameter value it changed. |
| "Add scope" on a cable | The new scope module and its cable together. |
| A button press whose saved value is toggle | The toggle. |

### What is not undoable

| Not in history | Why |
|---|---|
| Selection, pan, zoom, and which tile is open in the inspector | View state. It does not change the sound. |
| Probes and scopes the UI attaches for display | View state, not part of the patch. |
| A momentary button hold | A performance action, not an edit. Its value is never saved. |
| Parameter changes that come from a hardware MIDI control | Performance. Hundreds a second would bury the user's own edits. The next undo takes back the user's last edit and does not touch the mapped value. |
| Transport: tempo, time signature, start and stop, position | Performance controls that the user changes while playing. See [Open points](#open-points) for tempo. |
| Saving | Writes the file; the patch does not change. |
| Learn mode on or off, and arming a parameter | A mode, not an edit. Undo while a parameter is armed disarms it first, then undoes. |

### Gestures

The canvas reports one `SetControl` per mouse move, so the app cannot tell where a drag starts or ends. Two new canvas events mark it: a **gesture start** and a **gesture end**. They carry the same target as the controls they bracket (module and widget, or module and assignment). The inspector sends the same pair for its sliders.

A control that changes without a drag (a click on a toggle, a radio choice, a key step on a focused control) sends a start and an end around its single change.

If a gesture is cut off (the pointer is released outside the window, the window loses focus, the patch is closed), the end is still sent. The existing rule for button holds ending applies here too.

### History limit

The history keeps the last **200 steps**. When a new step would make it longer, the oldest step is dropped. A step stores only what it needs to reverse and replay itself (ids, old and new values, a removed module's parameters), so 200 small steps cost little memory.

## Rules

1. **One user action, one step.** What the user sees as one action is one step, even when it changes several things in the engine. *Why:* undo must match the user's picture of what they did, not our internal commands.
2. **A drag is one step.** Gestures are bracketed by start and end events and joined into one step. *Why:* a knob drag makes dozens of values; undoing them one by one is unusable.
3. **A gesture that ends where it began is not a step.** *Why:* a click that does not change a value should not take an undo press.
4. **Undo restores the patch, not the sound's history.** Parameters, cables, positions, assignments, and mappings come back exactly. Internal module state (delay lines, oscillator phase, envelope stage) does not, and a module that is removed and restored starts from its reset state. *Why:* the engine state is not part of the patch, and saving it would break the real-time rules.
5. **Ids come back.** A restored module or assignment gets its old id. *Why:* other steps in the history refer to ids, and assignments on one parameter are summed in id order, which decides the last bits of the output ([modulation undo](modulation.md#undo)).
6. **Deleting then undoing gives a bit-identical result.** Same patch, same input, same output, at any block size. *Why:* determinism is a hard rule; undo is no exception.
7. **Undo and redo are edits on the controller, never on the audio thread.** They call the same `Controller` methods an edit calls and then commit. They keep the real-time rules. *Why:* ADR-0001.
8. **One commit per step.** A step that changes several things is applied, then committed once, so the engine swaps one schedule. *Why:* the user hears one change, not a set of partial ones.
9. **Redo is dropped by a new edit.** The history is a line, not a tree. *Why:* a tree is hard to show and rarely needed.
10. **Undo refuses nothing silently.** If a step cannot be replayed (for example a module kind is no longer in the registry), the history is cleared and the status bar says why. This should not happen in normal use. *Why:* a half-applied undo would be worse than none.
11. **Hardware controls are not history.** Parameter values set by MIDI mappings never become steps ([What is not undoable](#what-is-not-undoable)). *Why:* see above; and a performer must not lose their last real edit to a fader wiggle.

## Engine support

Almost everything is already possible with the current `Controller`: `add_module_with_id` puts a module back with its old id, and `map_midi`, `learn_midi`, and `unmap_midi` return what they replaced.

What is missing:

- **Restore an assignment with its old id.** `Controller::add_assignment` always takes the next id. Undo needs a restore method that takes the id (rule 5). *Leaning:* one new method, no ADR.
- **Set a value without changing a mapping's feedback state** if learning ends up needing it. To be checked when building.

The edit log itself lives in `loom-app` and knows nothing about the audio thread.

## Acceptance criteria

- [ ] Undo removes a module that was just added. Redo adds it back with the same id and position.
- [ ] Undo of a delete brings back the modules, their parameter values, their cables, their assignments, and their MIDI mappings, in one step.
- [ ] After undoing the delete of a module with assignments, rendering the patch gives a bit-identical result to rendering before the delete, at block sizes 1, 77, and 512.
- [ ] Undo of a connect that replaced a cable brings the old cable back.
- [ ] A knob drag of many values is one step; undo returns the value from before the drag, redo returns the value at the end.
- [ ] A depth slider drag is one step.
- [ ] A click that does not change a value adds no step.
- [ ] A drag that moves several selected modules is one step.
- [ ] A drag cut off by focus loss still ends its gesture and makes one step.
- [ ] One undo of a learned mapping restores both the mapping state and the parameter's old value.
- [ ] Removing a MIDI mapping with × can be undone.
- [ ] Moving a mapping to another parameter can be undone.
- [ ] A hardware MIDI knob movement adds no step.
- [ ] A new edit after an undo drops the redo steps.
- [ ] The history keeps 200 steps; the 201st edit drops the oldest.
- [ ] Opening a patch clears the history, and the buttons are disabled.
- [ ] Undo and redo work from the keyboard and from the toolbar; the toolbar hover text names the step.
- [ ] Undo and redo do not allocate or lock on the audio thread (`tests/no_alloc.rs` still passes).

## Out of scope

- **Copy, paste, and duplicate.** They are an edit that will join the history when they land ([#14](https://github.com/brylie/LoomDSP/issues/13)).
- **Module data** (patterns, samples). It joins when it exists (FR-LIB-19, OQ-17). Large data may need steps that share storage.
- **Undo of arrangement part swaps** (open question on swaps). A swap is one step when it exists.
- **A history panel** that lists steps, or branching history.
- **Keeping the history across sessions** or after a save and reopen.
- **Undo for edits made by several people at once.**

## Open points

1. **Tempo and time signature.** Are they edits (in the history) or performance controls (not)? *Leaning:* not undoable while the transport runs, since the user plays with them; revisit if users expect it.
2. **Unsaved-changes marker.** Should undo back to the saved state clear the "unsaved" mark? *Leaning:* yes, by remembering the history position at save. Not needed for the first version if the app has no marker yet.
3. **History limit.** 200 is a guess. *Leaning:* keep it a constant and revisit with real use.
4. **ADR.** One extra `Controller` method does not need one. If the work grows into a general edit-command layer in `loom-core`, write an ADR first.
5. **Reset module.** Is "reset" one step with all its parameters, or is it excluded because it is cheap to redo? *Leaning:* one step.
6. **Merging fast key steps.** Should several key steps on a focused control within a short time merge into one step? *Leaning:* no, one key step is one step, until users ask. This also answers the question left in [#12](https://github.com/brylie/LoomDSP/issues/12) from #68.
