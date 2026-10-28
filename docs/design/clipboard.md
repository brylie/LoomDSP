# Copy, Paste, and Duplicate Specification

How a user copies modules with their cables and modulation, pastes them into the same patch or another one, and duplicates them in place.

- **Kind:** living specification.
- **Status:** Implemented. The transport rule (rule 6) waits for the Transport module.
- **Decision behind it:** none of its own. Builds on [ADR-0008](../adr/0008-patch-format-ron.md) (the clipboard holds patch text) and [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (all edits go through `Controller`).
- **Use case and requirements:** FR-CANVAS-05. Related: FR-FILE-05 (unknown modules, [#18](https://github.com/brylie/LoomDSP/issues/18)), NFR-UX-01 (keyboard, [#101](https://github.com/brylie/LoomDSP/issues/101)).
- **Background:** none.
- **Prior art:** every editor has copy, paste, and duplicate. The only conventions we use are the common shortcuts, and "paste lands at the pointer".
- **Issues:** [#13](https://github.com/brylie/LoomDSP/issues/13).
- **Code:** `crates/loom-app/src/clipboard.rs` (`Group`; tests in `clipboard/tests.rs`), the commands in `main.rs`, and `CanvasEvent::Paste` in `loom-canvas`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

> I built a filter and envelope pair I like. I select both, press Cmd+D, and get a second pair next to the first, played by the same keyboard. I copy the pair, open my other patch, and paste it where the pointer is. I can also paste it into a text file, change a value, and paste it back.

## Terms

| Term | Meaning |
|---|---|
| **Selection** | The selected modules on the canvas. |
| **Group** | The modules being copied or duplicated, with what goes with them: the cables and assignments the rules below keep. |
| **Inside** | A cable or assignment whose two ends are both on selected modules. |
| **Incoming** | A cable or assignment that starts outside the selection and ends on a selected module. |
| **Outgoing** | A cable or assignment that starts on a selected module and ends outside the selection. |

## Behaviour

### Commands

| Action | Result |
|---|---|
| **Ctrl+C** (**Cmd+C** on macOS) | Copy the selection to the system clipboard as patch text. The patch does not change. |
| **Ctrl+X** (**Cmd+X**) | Cut: copy, then delete the selection. One undo step. |
| **Ctrl+V** (**Cmd+V**) | Paste the clipboard into the patch. One undo step. |
| **Ctrl+D** (**Cmd+D**) | Duplicate the selection in place. One undo step. The clipboard does not change. |

- With nothing selected, copy, cut, and duplicate do nothing. The clipboard keeps what it had.
- The status bar says what happened: **Copied 3 modules**, **Pasted Filter**, **Duplicated 2 modules**, or why nothing happened.
- Copy is not an edit. It adds no undo step and does not mark the patch as changed.
- While a drag or a held arrow key is in progress, cut, paste, and duplicate do nothing, as undo does. *Why:* the edit would join the gesture's step and get its name.

### What goes with the modules

| | Copy, cut, and paste | Duplicate |
|---|---|---|
| Modules, with their parameter values | Kept | Kept |
| Inside cables | Kept | Kept |
| Incoming cables | Dropped | Kept: the copy is fed by the same output |
| Outgoing cables | Dropped | Dropped |
| Inside modulation assignments | Kept | Kept |
| Incoming assignments (outside source, selected parameter) | Dropped | Kept: the source also moves the copy's parameter |
| Outgoing assignments (selected source, outside parameter) | Dropped | Dropped |
| MIDI mappings | Dropped | Dropped |

*Why duplicate keeps incoming cables:* an output can feed any number of inputs, so the copy can share the original's source without changing the original. A duplicated voice is still played by the same keyboard. *Why it drops outgoing cables:* an input takes one cable. Keeping an outgoing cable would replace the original's cable into that input, and the original would go silent.

*Why paste drops incoming cables:* the clipboard can go to another patch, where the outside module does not exist, or exists with another id. One rule for every paste is easier to predict than one that depends on where the paste lands.

*Why assignments follow the same rules as cables:* a source can drive any number of parameters, as an output can feed any number of inputs. An outgoing assignment is dropped because it would add a second, equal modulation to the outside parameter, and its sound would change.

*Why MIDI mappings are dropped:* a hardware control maps to one parameter only ([MIDI learn](midi-learn.md)). A copied mapping would take the control away from the original.

### Where a paste lands

| Where the pointer is | The group goes |
|---|---|
| Over the canvas | With the top-left corner of the group at the pointer, snapped to the grid |
| Anywhere else in the window | With the top-left corner of the group at the centre of the visible canvas, snapped to the grid |

The modules keep their places relative to each other. *Why the centre:* the same as the toolbar Add button ([module browser](module-browser.md)). The pasted modules are always in view.

**Duplicate** puts the copy 2 grid units (24 pixels) right of and below the original, at any zoom. *Why:* the copy is visibly separate but stays next to the original. Duplicate again, and the next copy is offset from the last one, because the new copy is selected.

A pasted module with no position (text written by hand) goes in a row starting at the paste point, below the modules that have one. *Why not the patch auto-layout:* it places new modules below the whole patch, far from the pointer.

### After a paste or duplicate

- The new modules are selected, and nothing else is. *Why:* the user can drag them away at once, and the inspector follows.
- Pan and zoom do not change.
- The new modules start from their reset state, like a module added from the browser. Only parameter values are copied, not delay lines or envelope stages.

### The clipboard text

Copy writes a whole patch in the [patch format](../../crates/loom-patch/src/lib.rs), with `version` and an empty `name`. It holds the selected modules with their positions and non-default parameters, the inside cables, and the inside assignments in their order. It holds no MIDI mappings.

```ron
Patch(
    version: 1,
    name: "",
    modules: [
        (id: 4, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}, position: (240.0, 96.0)),
        (id: 7, kind: "loom.env.envelope@1", position: (240.0, 0.0)),
    ],
    cables: [],
    modulation: [
        (source: (7, "out"), target: (4, "cutoff"), depth: 0.5),
    ],
)
```

*Why a whole patch, not a fragment:* paste reads it with the same parser and version check as Open, so there is one format to keep correct. A saved patch file can be opened in a text editor, copied, and pasted into another patch.

Paste reads any text that is a valid patch, including text edited by hand and a whole patch file. MIDI mappings in it are ignored. The ids in it only link modules, cables, and assignments inside the text; they are never kept (rule 2).

Duplicate does not use the system clipboard. It takes the group straight from the graph, so it also has the incoming cables and assignments, which the clipboard text cannot hold.

### When a paste cannot be done

Nothing is added, no undo step is made, and the status bar says why, in these cases:

| Case | Status bar |
|---|---|
| The clipboard holds no text, or text that is not a patch | **The clipboard holds no LoomDSP modules** |
| The patch format version is newer than this build | The same message as Open gives |
| A module type is not registered | **Cannot paste: unknown module `kind`** |
| A parameter, port, or module id that the text refers to does not exist | **Cannot paste:** and the reason |
| Not enough free module slots | **Cannot paste: the patch has room for N more modules** |

*Why the whole paste or nothing:* half a group, with some cables missing, is worse than none, and it would make a step that undo cannot describe. This is the same rule as undo ([undo](undo.md#rules), rule 10).

## Rules

1. **Cut, paste, and duplicate are one undo step each.** They run inside one edit, so undo removes the whole group and redo brings it back with the same ids. Copy is not a step. Step names: **cut Filter** or **cut 3 modules**, **paste Filter** or **paste 3 modules**, **duplicate Filter** or **duplicate 3 modules**. *Why:* the user sees one action. The undo step is the difference before and after the edit, so no extra undo code is needed ([undo](undo.md#how-it-works)).
2. **New modules get new ids.** Each pasted or duplicated module gets the next free id from the controller, in the order of the old ids. Cables and assignments are remapped from old to new ids. *Why:* ids are never given out twice in a session ([undo](undo.md#rules), rule 5), and the old ids may be in use.
3. **Assignment order is kept.** Assignments are added in the order the clipboard text lists them, or, for duplicate, the order of the originals. *Why:* assignments on one parameter are summed in id order, which decides the last bits of the output. A duplicate must sound the same as the original (determinism).
4. **One commit.** All the new modules, cables, and assignments are added, then committed once. *Why:* the engine swaps one schedule, and the user hears the whole group start at once ([undo](undo.md#rules), rule 8).
5. **Check before adding.** Before anything is added, the app checks that every module type is registered, that there are enough free module slots, and that every name in the text exists. *Why:* see [When a paste cannot be done](#when-a-paste-cannot-be-done).
6. **A patch keeps exactly one transport.** The Transport module ([clock](clock.md)) is never added by a paste or duplicate. Copy still writes it to the clipboard, so its cables and assignments have an end. On paste, cables and assignments from the copied transport connect to the patch's own transport, and the copied tempo and time signature are ignored. Duplicate skips a selected transport and treats it as outside the selection. If only the transport is selected, duplicate does nothing and the status bar says why. *Why:* a patch has exactly one transport, and a group copied with Select All should keep its clock cables in the other patch. This rule applies once the Transport module exists.
7. **The keys do not reach the canvas while a text field is being edited.** In the module browser's search field and in other text fields, Ctrl/Cmd+C, X, V, and D act on the text. *Why:* the same rule as undo ([undo](undo.md#commands)). The text field has its own clipboard.
8. **The keys do not clash with panel focus.** Panel focus uses Tab, the arrows, Space, Enter, and Escape without modifiers ([panels](panels.md#interaction-68)). Copy, cut, paste, and duplicate use Ctrl or Cmd, so they work the same with or without a focused control. *Why:* [#101](https://github.com/brylie/LoomDSP/issues/101) orders the keys; these keys need no place in that order.

## Acceptance criteria

- [x] Copy writes patch text with `version`, the selected modules, their positions and parameters, the inside cables, and the inside assignments, and no MIDI mappings.
- [x] Copy with nothing selected leaves the clipboard unchanged.
- [x] Paste of copied text adds the modules with new ids, their parameter values, the inside cables, and the inside assignments in the same order.
- [x] Paste with the pointer over the canvas puts the group's top-left corner at the pointer, snapped to the grid. With the pointer elsewhere, at the centre of the visible canvas.
- [x] Duplicate puts the copy 2 grid units right and down, keeps incoming cables and incoming assignments, and drops outgoing ones.
- [x] Duplicate leaves the original's cables, assignments, and MIDI mappings unchanged.
- [x] An offline render of a patch after duplicating a module with inside assignments matches a render of the same patch built by hand, bit for bit, at block sizes 1, 77, and 512.
- [x] After a paste or duplicate, the new modules are selected and nothing else is.
- [x] Undo removes the whole paste, redo brings it back with the same ids. The same for duplicate and cut.
- [x] Cut copies, then deletes, as one undo step.
- [x] Paste of a whole patch file adds its modules and cables and ignores its MIDI mappings.
- [x] Paste of hand-edited text with a changed parameter value uses the new value.
- [x] Paste of text that is not a patch, of an unknown module type, of an unknown parameter, or of too many modules adds nothing, adds no undo step, and says why in the status bar.
- [ ] Ctrl/Cmd+C, X, V, and D act on the text, not the patch, while the module browser's search field is open. (manual)
- [ ] Copy, paste, and duplicate work while a panel control has focus. (manual)
- [x] Paste and duplicate do not allocate or lock on the audio thread (`tests/no_alloc.rs` still passes).

## Out of scope

- **Placeholders for unknown modules** ([#18](https://github.com/brylie/LoomDSP/issues/18)). Until then, a paste with an unknown type is refused. When placeholders exist, a placeholder copies and pastes like any module, and keeps its data.
- **Paste from the context menu or an Edit menu.** The app has neither yet. They call the same commands when they exist.
- **Paste with incoming cables** ("paste with connections"). Only duplicate keeps them. Revisit if users ask.
- **Copying parameter values alone**, from one module to another of the same type.
- **Other clipboard formats**, such as an image of the selection.
- **Module labels.** The patch format has them, but the app does not show them yet. When it does, they are copied with the module.

## Open points

1. **Duplicate offset.** 2 grid units is a guess. *Leaning:* keep it a constant and revisit with real use.
