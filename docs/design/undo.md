# Undo and Redo Specification

How a user takes back and replays edits on the canvas, the inspector, and the panels, so that experiments are cheap.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** none of its own yet. Builds on [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (all edits go through `Controller`). A short ADR is needed only if the `loom-core` change under [Engine support](#engine-support) turns out larger than a few small methods (see [Open points](#open-points)).
- **Use case and requirements:** FR-CANVAS-04. Related: FR-LIB-19 (module data is covered by undo, built later).
- **Background:** none.
- **Prior art:** every editor has undo. The only conventions we use are the common shortcuts and "one drag is one step".
- **Issues:** [#12](https://github.com/brylie/LoomDSP/issues/12). Rules this spec collects from elsewhere: [modulation undo](modulation.md#undo), [MIDI learn](midi-learn.md) (out of scope list), [removing an item](panels.md#removing-an-item).
- **Code:** `crates/loom-app/src/undo.rs` (snapshots, steps, history; tests in `undo/tests.rs`), `edits.rs` (the edit and gesture being recorded, and recording off after a failed commit; tests in `edits/tests.rs`), and the edit wiring in `editing.rs`. `Controller::restore_assignment` and `Controller::free_module_slots` in `loom-core`. `CanvasEvent::GestureStart` and `GestureEnd` in `loom-canvas`.

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
| Cut, paste, duplicate | **cut Filter**, **paste 3 modules**, **duplicate Filter** ([copy, paste, and duplicate](clipboard.md)) |

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
| Double-click a control to reset it to its default | The reset, as one step. |
| Pick a parameter's control range in the inspector ([control ranges](control-ranges.md)) | The new range, and the value it clamped, together. |
| Pick a choice name in the inspector ([choice names](panels.md#choice-names)) | The pick, as one step. |
| Add or remove a modulation assignment | The add or the remove. |
| Change an assignment's depth, polarity, or on/off | One gesture, like a parameter change. |
| Map a MIDI control by learning | The new mapping, **and** the parameter value the control change set, together. If the mapping replaced another mapping on the same parameter or the same control, undo brings the old one back. |
| Move a mapping to another parameter | The move. |
| Remove a MIDI mapping (the small × button) | The removal. |
| "Add scope" on a cable | The new scope module and its cable together. |
| Cut, paste, or duplicate | The whole group: its modules, cables, and assignments ([copy, paste, and duplicate](clipboard.md)). Copy is not a step. |
| A button press whose saved value is toggle | The toggle. |

### What is not undoable

| Not in history | Why |
|---|---|
| Selection, pan, zoom, and which tile is open in the inspector | View state. It does not change the sound. |
| Probes and scopes the UI attaches for display | View state, not part of the patch. |
| A momentary button hold | A performance action, not an edit. Its value is never saved. |
| Transport: tempo, time signature, start and stop, position | Performance controls that the user changes while playing. Tempo and time signature are saved in the patch, but in the first version they stay out of the history. *Why:* the user changes them to play, often while the transport runs, and undoing a knob turn should never jump the tempo back. Revisit if users expect them in the history. |
| Parameter changes that come from a hardware MIDI control | The hardware control is its own undo: move it back. Undo cannot move a physical knob, so taking back a fader move would leave the sound and the fader apart, and the next touch would jump. It would also bury the user's own edits and drop their redo steps. See [Mapped parameters](#mapped-parameters). Revisit when pickup modes (FR-MIDI-09) remove the jump. |
| Saving | Writes the file; the patch does not change. |
| Learn mode on or off, and arming a parameter | A mode, not an edit. Undo while a parameter is armed disarms it first, then undoes. |

### Mapped parameters

A parameter with a MIDI mapping can change both from the screen and from the hardware.

- A screen edit on it is a step like any other. The step stores the value at the gesture start and at the gesture end.
- Undo sets the stored start value, and redo the stored end value, even if the hardware moved the parameter since. The hardware takes over again at its next move, and the value can jump, as after any screen edit on a mapped parameter ([MIDI learn](midi-learn.md#playing)). *Why:* undo must give a known result. Skipping the step, or keeping the hardware value, would make undo depend on a fader the user may not remember touching.
- Hardware moves never become steps, and they never drop the redo steps.

### Gestures

The canvas reports one `SetControl` per pointer move, and one `Move` per pointer move during a module drag, so the app cannot tell where a drag starts or ends. Two new canvas events mark it: a **gesture start** and a **gesture end**. They carry no target. The app compares the patch at the start and at the end, so the step holds whatever changed in between, and the name comes from that change. *Why:* one rule covers every gesture, including a cable pulled out of one input and dropped on another. Gestures start at pointer down on a knob, a tile, or a port, and end at pointer up.

An inspector slider sends no start, and letting go of it sends a slider-release message. The slider's first change opens the gesture. Arrow keys on a hovered slider change it without any release, so the next edit, or an undo or redo, closes a gesture that began on a slider. *Why:* the slider widget gives no other signal, and a gesture left open would block undo.

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
11. **Hardware controls are not history.** Parameter values set by MIDI mappings never become steps ([Mapped parameters](#mapped-parameters)). *Why:* the hardware control is its own undo, undo cannot move it back, and a performer must not lose their last real edit to a fader wiggle.

## How it works

The app keeps the patch (modules with parameters and positions, cables, assignments, MIDI mappings) as a snapshot when an edit or gesture starts, and again when it ends. The difference is the step: only what changed, as (before, after) pairs. Undo applies the before side, redo the after side, through the same `Controller` methods an edit uses, and commits once.

*Why compare states and not log inverse commands:* one action can touch many things, and a difference holds all of them with nothing to keep in step with the controller. A change on a mapped parameter counts only if the app set it, which is how hardware moves stay out of steps.

## Engine support

Almost everything is already possible with the current `Controller`: `add_module_with_id` puts a module back with its old id, `connect` and `disconnect` return the cable they replaced or removed, and `map_midi`, `learn_midi`, and `unmap_midi` return the mappings they replaced.

`remove_module` returns nothing, and it drops the module's cables, assignments, and mappings with it. The app reads all of these from the graph before it removes the module, and stores them in the step together with the module's kind, parameter values, and position. No new method is needed for that.

What was missing, and is now built:

- **Restore an assignment with its old id.** `Controller::add_assignment` always takes the next id. `Controller::restore_assignment` takes the id and the whole assignment, including polarity and on/off (rule 5). Built.
- **Free module slots.** The check in rule 10 uses `Controller::free_module_slots`. Built.
- Learning needed no further change: undo sets the stored value with `set_params`.

The edit log itself lives in `loom-app` and knows nothing about the audio thread.

## Acceptance criteria

- [x] Undo removes a module that was just added. Redo adds it back with the same id and position.
- [x] Undo of a delete brings back the modules, their parameter values, their positions, their cables, their assignments, and their MIDI mappings, in one step. The restored modules are selected.
- [x] After undoing the delete of a module with assignments, an offline render of the patch is bit-identical to a render before the delete, at block sizes 1, 77, and 512.
- [x] Undoing a delete right after it, with the patch at the module limit, either works or changes nothing and says why in the status bar.
- [x] Undo of a connect that replaced a cable brings the old cable back.
- [x] A knob drag of many values is one step; undo returns the value from before the drag, redo returns the value at the end.
- [ ] A depth slider drag is one step.
- [x] A click that does not change a value adds no step.
- [ ] A double-click reset is one step.
- [x] Holding an arrow key on a focused control is one step.
- [ ] A drag that moves several selected modules is one step.
- [x] A drag cut off by focus loss still ends its gesture and makes one step.
- [x] One undo of a learned mapping restores both the mapping state and the parameter's old value.
- [x] Removing a MIDI mapping with × can be undone.
- [x] Moving a mapping to another parameter can be undone.
- [ ] A hardware MIDI knob movement adds no step and does not drop the redo steps.
- [x] Undo of a screen edit on a mapped parameter sets the value from before the edit, even after the hardware moved it.
- [x] A new edit after an undo drops the redo steps.
- [x] The history keeps 200 steps; the 201st edit drops the oldest.
- [ ] Opening a patch clears the history, and the buttons are disabled.
- [ ] Undo and redo work from the keyboard and from the toolbar; the toolbar hover text and the status bar name the step.
- [ ] Undo and redo shortcuts do nothing while a text field is being edited.
- [ ] Undo and redo do not allocate or lock on the audio thread (`tests/no_alloc.rs` still passes).

## Out of scope

- **Module data** (patterns, samples). It joins when it exists (FR-LIB-19, OQ-17). Large data may need steps that share storage.
- **Undo of arrangement part swaps** (open question on swaps). A swap is one step when it exists.
- **Reset of a whole module.** There is no such command yet. When it exists, it is one step with every parameter it changed.
- **Undo of hardware MIDI moves.** Revisit when pickup modes (FR-MIDI-09) land (see [What is not undoable](#what-is-not-undoable)).
- **Tempo and time signature in the history** (see [What is not undoable](#what-is-not-undoable)).
- **Merging separate key presses** made close together into one step. One key press is one step until users ask for more. A held key is already one gesture. This answers the question left in [#12](https://github.com/brylie/LoomDSP/issues/12) from [#68](https://github.com/brylie/LoomDSP/issues/68).
- **A history panel** that lists steps, or branching history.
- **Keeping the history across sessions** or after a save and reopen.
- **Undo for edits made by several people at once.**

## Open points

1. **Unsaved-changes marker.** Should undo back to the saved state clear the "unsaved" mark? Done: the history remembers its position at save ([files](files.md#what-counts-as-an-unsaved-change)).
2. **History limit.** 200 is a guess. *Leaning:* keep it a constant and revisit with real use.
3. **ADR.** The two small `Controller` methods do not need one. If the work grows into a general edit-command layer in `loom-core`, write an ADR first.
