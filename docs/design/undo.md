# Undo and Redo Specification

How a user takes back and replays edits on the canvas, the inspector, and the panels, so that experiments are cheap.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** none of its own yet. Builds on [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (all edits go through `Controller`). A short ADR is needed only if the `loom-core` change under [Engine support](#engine-support) turns out larger than a few small methods (see [Open points](#open-points)).
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
| **Gesture** | A user action that sends many events and ends at a known moment: a knob drag, a depth slider drag, a module move drag, a held arrow key. One gesture is one step. |
| **Hardware move** | A run of changes from mapped MIDI controls with no pause longer than the [pause time](#hardware-moves) between them. One hardware move is one step. |
| **History** | The list of steps, with a position. Steps before the position can be undone; steps after it can be redone. |

## Behaviour

### Commands

| Action | Result |
|---|---|
| **Ctrl+Z** (**Cmd+Z** on macOS) | Undo the last step. |
| **Ctrl+Shift+Z** (**Cmd+Shift+Z**), or **Ctrl+Y** on Windows and Linux | Redo the step that was undone last. |
| **Undo** and **Redo** buttons in the toolbar | The same. A button is disabled when there is nothing to do. Its hover text names the step (see [Step names](#step-names)). |
| Make any new edit after undoing | The steps that could be redone are dropped. |
| Press undo or redo while a gesture is in progress | Nothing happens. The gesture finishes first. |
| Press undo while a hardware move is in progress | The hardware move ends at once, and undo takes it back. |
| Open a patch, or start a new one | The history is cleared. |

The shortcuts work anywhere in the window, except while a text field is being edited. A text field has its own undo. *Why not only when the canvas has focus:* the app has no focus for the canvas or the inspector as a whole, and the user expects undo to work wherever they last clicked.

### After an undo or redo

- **Selection.** The modules the step touched and that still exist become the selection. A module that the step removed leaves the selection. A step that touches no module directly (a cable, a mapping) keeps the selection as it was. *Why:* the user sees at once what changed, and the inspector follows it.
- **View.** Pan and zoom do not change. *Why:* moving the view on every undo is disorienting, and the selection already shows what changed.
- **Status bar.** It names the step, for example **Undo: delete 3 modules**.

### Step names

A step is named by a verb and what it acted on, in the same words the user sees on screen. The name is shown in the toolbar hover text and the status bar.

| Step | Name |
|---|---|
| Add a module | **add Filter** |
| Delete modules | **delete Filter**, or **delete 3 modules** |
| Move modules | **move Filter**, or **move 2 modules** |
| Connect, disconnect | **connect cable**, **remove cable** |
| Change a parameter | **change Filter Cutoff** |
| Assignment add, remove, change | **add modulation**, **remove modulation**, **change modulation depth** |
| MIDI mapping | **learn CC 74**, **remove MIDI mapping**, **move MIDI mapping** |
| Hardware move | **MIDI change Filter Cutoff**, or **MIDI change 2 parameters** |

The exact wording can change while building. Each step must have a name.

### What is undoable

| Edit | One step is |
|---|---|
| Add a module | The add. Undo removes the module. |
| Delete modules | The whole delete, however many modules are selected. It includes the cables, modulation assignments, and MIDI mappings that went with them. |
| Move modules | One drag, for all selected modules together. A drag that ends where it started is not a step. |
| Connect | The cable. If it replaced a cable into the same input, undo brings the old cable back. |
| Disconnect | The removed cable. |
| One drag that removes a cable and adds another (moving a cable end to another input) | Both changes, as one step. |
| Change a parameter by a knob, slider, or radio control, on a tile or in the inspector | One gesture. A single click or key press on a control is a gesture of its own. Holding an arrow key down, so that it repeats, is one gesture from key down to key up, however many times it repeats. A gesture that ends at its starting value is not a step. |
| Change parameters by moving mapped MIDI controls | One [hardware move](#hardware-moves), however many controls and values it covers. |
| Double-click a control to reset it to its default | The reset, as one step. |
| Add or remove a modulation assignment | The add or the remove. |
| Change an assignment's depth, polarity, or on/off | One gesture, like a parameter change. |
| Map a MIDI control by learning | The new mapping, **and** the parameter value the control change set, together. If the mapping replaced another mapping on the same parameter or the same control, undo brings the old one back. |
| Move a mapping to another parameter | The move. |
| Remove a MIDI mapping (the small × button) | The removal. |
| "Add scope" on a cable | The new scope module and its cable together. |
| A button press whose saved value is toggle | The toggle. |

### What is not undoable

| Not in history | Why |
|---|---|
| Selection, pan, zoom, and which tile is open in the inspector | View state. It does not change the sound. |
| Probes and scopes the UI attaches for display | View state, not part of the patch. |
| A momentary button hold | A performance action, not an edit. Its value is never saved. |
| Transport: tempo, time signature, start and stop, position | Performance controls that the user changes while playing. Tempo and time signature are saved in the patch, but in the first version they stay out of the history. *Why:* the user changes them to play, often while the transport runs, and undoing a knob turn should never jump the tempo back. Revisit if users expect them in the history. |
| Saving | Writes the file; the patch does not change. |
| Learn mode on or off, and arming a parameter | A mode, not an edit. Undo while a parameter is armed disarms it first, then undoes. |

### Hardware moves

Moving a mapped MIDI control is an edit like turning a knob on screen. *Why:* the user changes the patch either way, and expects to take back a fader move the same way as a knob drag.

MIDI has no "touch" or "release" message, so the app cannot see where a fader move starts or ends. It groups by time instead:

- A hardware move starts with the first change from any mapped control.
- It ends when no mapped control has changed for the **pause time** (1 second), or when the user makes a screen edit, presses undo or redo, or opens a patch.
- Changes from several controls in one hardware move are one step. *Why:* a performer often moves two faders together, and it is one action to them.
- The step stores, for each parameter it touched, the value before its first change and after its last change. A hardware move that leaves every parameter at its starting value is not a step (rule 3).
- Like any new edit, a hardware move drops the redo steps.
- The control change that finishes MIDI learn belongs to the learn step. Later changes start a hardware move.

Undo sets the stored start values and redo the stored end values. A physical fader stays where it is, so the parameter jumps to the fader's position at its next move. *Why:* this is already how a mapped parameter behaves after a screen edit, and it keeps undo exact.

### Mapped parameters

A parameter with a MIDI mapping can change both from the screen and from the hardware. Each change is a step of its own: a screen edit is a gesture, and a fader move is a hardware move. Undo sets the stored value even if the other source moved the parameter since. *Why:* undo must give a known result. Keeping whatever value is there now would make undo depend on which source moved last.

### Gestures

The canvas reports one `SetControl` per pointer move, and one `Move` per pointer move during a module drag, so the app cannot tell where a drag starts or ends. Two new canvas events mark it: a **gesture start** and a **gesture end**. They carry the target of the events they bracket: module and widget for a control, module and assignment for a depth slider, or the moved modules for a module drag. The inspector sends the same pair for its sliders.

A control that changes without a drag (a click on a toggle, a radio choice, a key step on a focused control, a double-click reset) sends a start and an end around its single change. An arrow key held down repeats. It sends the start at key down and the end at key up, and every repeat falls inside, so the whole hold is one step. *Why:* holding a key for a second would otherwise make dozens of steps and push the user's real edits out of the history.

If a gesture is cut off (the pointer is released outside the window, the window loses focus, the patch is closed), the end is still sent. The existing rule for button holds ending applies here too.

### History limit

The history keeps the last **200 steps**. When a new step would make it longer, the oldest step is dropped. A step stores only what it needs to reverse and replay itself (ids, old and new values, a removed module's parameters), so 200 small steps cost little memory. The limit is a constant in the code.

## Rules

1. **One user action, one step.** What the user sees as one action is one step, even when it changes several things in the engine. *Why:* undo must match the user's picture of what they did, not our internal commands.
2. **A drag is one step.** Gestures are bracketed by start and end events and joined into one step. *Why:* a knob drag makes dozens of values; undoing them one by one is unusable.
3. **A gesture that ends where it began is not a step.** *Why:* a click that does not change a value should not take an undo press.
4. **Undo restores the patch, not the sound's history.** Parameters, cables, positions, assignments, and mappings come back exactly. Internal module state (delay lines, oscillator phase, envelope stage) does not, and a module that is removed and restored starts from its reset state. *Why:* the engine state is not part of the patch, and saving it would break the real-time rules.
5. **Ids come back.** A restored module or assignment gets its old id. Ids are never given out twice in a session, so the old id is always free. A restored MIDI mapping may get a new id, because mapping ids do not change the sound and no step refers to them. *Why:* other steps in the history refer to module and assignment ids, and assignments on one parameter are summed in id order, which decides the last bits of the output ([modulation undo](modulation.md#undo)).
6. **Deleting then undoing gives a bit-identical result.** Same patch, same input, same output, at any block size. *Why:* determinism is a hard rule; undo is no exception.
7. **Undo and redo are edits on the controller, never on the audio thread.** They call the same `Controller` methods an edit calls and then commit. They keep the real-time rules. *Why:* ADR-0001.
8. **One commit per step.** A step that changes several things is applied, then committed once, so the engine swaps one schedule. *Why:* the user hears one change, not a set of partial ones.
9. **Redo is dropped by a new edit.** The history is a line, not a tree. *Why:* a tree is hard to show and rarely needed.
10. **Undo never fails silently.** Before a step is applied, the app checks what it can: the module kinds are in the registry, and there are enough free module slots. A removed module's slot is free only after the next schedule swap, so the check counts the slots in use, not only the modules in the graph. If the check fails, nothing is applied, the step stays in the history, and the status bar says why. If a `Controller` call still fails while the step is applied (for example the command queue is full), the history is cleared and the status bar says why. *Why:* a half-applied step that stays in the history would make every later undo wrong.
11. **A hardware move is one step.** Changes from mapped MIDI controls are grouped by pause time, not stored one by one ([Hardware moves](#hardware-moves)). *Why:* a fader sends many values a second, and storing each would push every other edit out of the history.

## Engine support

Almost everything is already possible with the current `Controller`: `add_module_with_id` puts a module back with its old id, `connect` and `disconnect` return the cable they replaced or removed, and `map_midi`, `learn_midi`, and `unmap_midi` return the mappings they replaced.

`remove_module` returns nothing, and it drops the module's cables, assignments, and mappings with it. The app reads all of these from the graph before it removes the module, and stores them in the step together with the module's kind, parameter values, and position. No new method is needed for that.

What is missing:

- **Restore an assignment with its old id.** `Controller::add_assignment` always takes the next id. Undo needs a restore method that takes the id and the whole assignment, including polarity and on/off (rule 5). *Leaning:* one new method, no ADR.
- **Free module slots.** The check in rule 10 needs to know how many module slots are free. *Leaning:* a read-only method on `Controller`.
- **Set a value without changing a mapping's feedback state** if learning ends up needing it. To be checked when building.

The edit log itself lives in `loom-app` and knows nothing about the audio thread.

## Acceptance criteria

- [ ] Undo removes a module that was just added. Redo adds it back with the same id and position.
- [ ] Undo of a delete brings back the modules, their parameter values, their positions, their cables, their assignments, and their MIDI mappings, in one step. The restored modules are selected.
- [ ] After undoing the delete of a module with assignments, an offline render of the patch is bit-identical to a render before the delete, at block sizes 1, 77, and 512.
- [ ] Undoing a delete right after it, with the patch at the module limit, either works or changes nothing and says why in the status bar.
- [ ] Undo of a connect that replaced a cable brings the old cable back.
- [ ] A knob drag of many values is one step; undo returns the value from before the drag, redo returns the value at the end.
- [ ] A depth slider drag is one step.
- [ ] A click that does not change a value adds no step.
- [ ] A double-click reset is one step.
- [ ] Holding an arrow key on a focused control is one step.
- [ ] A drag that moves several selected modules is one step.
- [ ] A drag cut off by focus loss still ends its gesture and makes one step.
- [ ] One undo of a learned mapping restores both the mapping state and the parameter's old value.
- [ ] Removing a MIDI mapping with × can be undone.
- [ ] Moving a mapping to another parameter can be undone.
- [ ] A hardware MIDI fader move of many values is one step; undo returns the value from before the move, redo returns the value at the end.
- [ ] Moving two mapped controls together, without a pause, is one step.
- [ ] Two fader moves with a pause longer than the pause time between them are two steps.
- [ ] A hardware move drops the redo steps.
- [ ] Pressing undo during a hardware move ends it and takes it back.
- [ ] Undo of a screen edit on a mapped parameter sets the value from before the edit, even after the hardware moved it.
- [ ] A new edit after an undo drops the redo steps.
- [ ] The history keeps 200 steps; the 201st edit drops the oldest.
- [ ] Opening a patch clears the history, and the buttons are disabled.
- [ ] Undo and redo work from the keyboard and from the toolbar; the toolbar hover text and the status bar name the step.
- [ ] Undo and redo shortcuts do nothing while a text field is being edited.
- [ ] Undo and redo do not allocate or lock on the audio thread (`tests/no_alloc.rs` still passes).

## Out of scope

- **Copy, paste, and duplicate.** They are an edit that will join the history when they land ([#13](https://github.com/brylie/LoomDSP/issues/13)).
- **Module data** (patterns, samples). It joins when it exists (FR-LIB-19, OQ-17). Large data may need steps that share storage.
- **Undo of arrangement part swaps** (open question on swaps). A swap is one step when it exists.
- **Reset of a whole module.** There is no such command yet. When it exists, it is one step with every parameter it changed.
- **Tempo and time signature in the history** (see [What is not undoable](#what-is-not-undoable)).
- **Merging separate key presses** made close together into one step. One key press is one step until users ask for more. A held key is already one gesture. This answers the question left in [#12](https://github.com/brylie/LoomDSP/issues/12) from [#68](https://github.com/brylie/LoomDSP/issues/68).
- **A history panel** that lists steps, or branching history.
- **Keeping the history across sessions** or after a save and reopen.
- **Undo for edits made by several people at once.**

## Open points

1. **Unsaved-changes marker.** Should undo back to the saved state clear the "unsaved" mark? *Leaning:* yes, by remembering the history position at save. Not needed for the first version, because the app has no marker yet.
2. **History limit and pause time.** 200 steps and 1 second are guesses. *Leaning:* keep each a constant and revisit with real use. A fader moved slowly with short stops may split into several steps; that is acceptable for the first version.
3. **ADR.** Two small `Controller` methods do not need one. If the work grows into a general edit-command layer in `loom-core`, write an ADR first.
