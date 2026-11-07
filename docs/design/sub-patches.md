# Sub-patches Specification

How a player collapses part of a patch into one module with its own ports and panel, opens it to edit, saves it to a library, and builds abstractions with swappable slots and modes.

- **Kind:** living specification.
- **Status:** Agreed, M2. The choices marked "Settled" were made with the maintainer on 2026-11-01. The [patch file](#patch-file) and [flattening](#flattening) are Implemented (slice 1 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-06), without `library`, `roles`, `panel`, and the mode fields, which their slices add. Collapse, dissolve, open, the breadcrumb, and undo across levels are Implemented (slice 2 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-07); see [Editor model](#editor-model).
- **Decision behind it:** [ADR-0031](../adr/0031-sub-patches-stored-as-definitions-and-flattened.md) (definitions, flattening, library items as patch files). [ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md) (the editor keeps the hierarchy as an overlay on the flat graph). [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md) for roles and slots. [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) and [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) for the sub-patch's panel.
- **Use case and requirements:** FR-ENCAP-01 to FR-ENCAP-10, FR-SDK-08, FR-PANEL-10, FR-PANEL-11, FR-LIB-24, FR-MOD-06. Settles OQ-20 and OQ-21.
- **Builds on:** [Panels: exposed controls in a sub-patch](panels.md#exposed-controls-in-a-sub-patch) (what the surface shows), [Module data](module-data.md) (data in slots and copies), [Undo](undo.md), [Copy, paste, and duplicate](clipboard.md), [Changing a running patch](live-patching.md).
- **Background:** [Flattening](../learn/flattening.md): how a sub-patch becomes plain modules, why a hierarchical engine was rejected, and why flat ids come from a table keyed by path. An explainer on "abstraction" belongs with roles and slots.
- **Prior art:** Max has both private sub-patchers and shared file abstractions. Reaktor and Bitwig Grid nest containers. Design tools have "instances" of a shared component. Named only to explain the trade-offs; the rules below are our own.
- **Issues:** [#51](https://github.com/brylie/LoomDSP/issues/51) (umbrella), [#76](https://github.com/brylie/LoomDSP/issues/76) (panel editor).
- **Code:** `crates/loom-patch/src/subpatch.rs` (the definition types), `flatten.rs` (flattening and the id table), `level.rs` (what the top patch and a definition share), `hierarchy.rs` (the editor's overlay: paths, definitions, ports, instances, boundary cables, `unflatten`, `collapse`, `dissolve`), and `crates/loom-modules/src/structure.rs` (the Patch input and output modules). Flattening lives in `crates/loom-patch`, next to the patch file types and the code that applies a patch to a `Controller`. `loom-core`'s `Graph` stays the flat engine graph with no file types (ADR-0002, ADR-0034). ADR-0031 left the crate open; settled with the maintainer on 2026-11-06 ([#651](https://github.com/brylie/LoomDSP/issues/651)).

## Goal

> My patch has 40 tiles. I select the oscillator, filter, and two envelopes, press Ctrl+G, and they become one tile called "Voice" with the same cables going in and out. I double-click it to change the filter, and the breadcrumb takes me back. I expose cutoff and resonance, so the Voice tile has two knobs. I save it to my library and drop it into tomorrow's patch.

> Later I build an abstraction with a filter slot. On stage I swap the state variable filter for the ladder from the tile's menu. The cables stay, the cutoff stays where it was, and nothing clicks.

## Terms

| Term | Meaning |
|---|---|
| **Sub-patch** | A module (`loom.struct.subpatch@1`, MT-STRUCT-01) whose inside is a patch. |
| **Definition** | The inside of a sub-patch: its modules, cables, assignments, exposed items, panel, and declared roles. Stored once in the patch's `subpatches` table. |
| **Instance** | A sub-patch tile in a patch. It refers to one definition and holds the values of that definition's exposed parameters. |
| **Patch input, Patch output** | Modules inside a definition (`loom.struct.input@1`, `loom.struct.output@1`, MT-STRUCT-02). Each becomes one port on the instance's tile. |
| **Level** | The patch shown on the canvas: the top patch, or the inside of an opened sub-patch. |
| **Exposed item** | An inner parameter, display, probe, or data slot that the sub-patch shows on its own surface (FR-ENCAP-03, [panels](panels.md#exposed-controls-in-a-sub-patch)). |
| **Linked** | An instance that follows a library file (FR-ENCAP-06). |
| **Role** | A contract of port and parameter names that a module or sub-patch promises to have (ADR-0017). |
| **Slot** | A module inside an abstraction (`loom.struct.slot@1`, MT-STRUCT-03) that holds one part filling a role. |
| **Part** | What fills a slot: a module type or a sub-patch that declares the slot's role. |
| **Mode** | How a sub-patch runs its inside: single, layer, select, or stack (FR-ENCAP-10). |

## Behaviour

### Make, open, and dissolve

| The player does | What happens |
|---|---|
| Selects modules, then **Ctrl+G** (Cmd+G), or **Make sub-patch** in the canvas menu | The selection becomes one sub-patch tile where the selection was. See [Collapsing](#collapsing). |
| Double-clicks a sub-patch's header, or presses **Ctrl+Enter** (Cmd+Enter) with one sub-patch selected | The canvas shows its inside. The breadcrumb bar shows the path. |
| Clicks a name in the breadcrumb (Patch › Voice › Filter) | The canvas shows that level. |
| Presses **Escape** when nothing else uses it | The canvas goes up one level. Escape keeps its other uses first: end a drag, clear pinned lines, clear control focus, clear the selection ([panels](panels.md)). |
| Selects one sub-patch, then **Ctrl+Shift+G**, or **Dissolve sub-patch** | Its inside is put back into the current level, cables reconnected through the old ports. The reverse of collapsing. |

- Each level keeps its own camera (pan and zoom) for the session. *Why:* going in and back out should land where the player left.
- The tile of a sub-patch shows its panel ([panels](panels.md#exposed-controls-in-a-sub-patch)). With nothing exposed, it shows only its name and ports.
- Audio keeps playing while the player moves between levels. Only what is drawn changes.

### Collapsing

1. Every cable from outside the selection into it becomes one **Patch input** inside, and one input port on the tile. Several inner inputs fed from the same outside output share one Patch input.
2. Every cable from the selection to outside becomes one **Patch output** inside, and one output port. One inner output feeding several outside inputs uses one Patch output.
3. A port takes the inner port's label, made unique with a number if needed ("in", "in 2").
4. A modulation assignment from outside to a parameter inside exposes that parameter, and the assignment targets the exposed parameter on the tile. An assignment from inside to outside gets a Patch output for its source, and targets from the tile's new port. *Why:* assignments never cross a level (modulation rule 10).
5. MIDI mappings, probes, and data move with their modules.
6. The Transport and the Scale and tuning module stay outside, even when selected (rule 8). They count as outside the selection, so their cables become ports by points 1 and 2. If only they are selected, collapse does nothing and the status bar says why.
7. The sound does not change. Collapsing is one undo step.

### Exposing

Exposing follows [Panels: exposed controls](panels.md#exposed-controls-in-a-sub-patch). In addition:

- **Expose** is in the inspector's row menu for any parameter, display value, and data slot of an inner module, and in the panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)).
- An exposed parameter's value belongs to the instance. The inner control shows it, and turning either one changes it.
- **Unexpose** removes the surface item. The inner parameter keeps the current value.
- The surface can give a narrower range, in the same unit. The value is clamped to it; it is not rescaled. *Why:* "cutoff 200 to 2000 Hz" on the surface still reads in Hz, and an exposed parameter turns like the inner one.

### Copies and editing

**Settled.** Each sub-patch tile is a distinct part of the patch. Nothing done to one tile ever changes another.

- **Every tile has its own definition.** Definitions are never shared between tiles, not even by two linked instances of the same library file. *Why:* the player should never have to remember where else a sub-patch was copied, or wonder what a local edit does to the rest of the patch. Keeping copies in step would also add complexity for little gain.
- **Copy, paste, and duplicate make an independent copy,** with its own new definition. A copy of a linked tile is linked to the same library file, and is still its own tile.
- **Editing inside a sub-patch changes only that tile.** If the tile is linked, the first edit inside unlinks it, so the next load does not replace the edit. The status bar says: `Voice is now a copy. It no longer follows the library.` The edit and the unlink are one undo step.
- Changing an instance's exposed values is not an edit inside, so a linked instance stays linked.

### Library

**Settled.** A library item is an ordinary patch file.

- **Save to library** (on the sub-patch's tile menu, or **File > Save to library** inside it) writes the definition as a patch file in the library folder. Its Patch inputs and outputs are its ports, and its exposed items and panel come along. Its patch metadata (authors, tags, licence) is the sub-patch's own, and patch info credits it as a part ([credit for parts](patch-metadata.md#credit-for-parts)).
- The **library folder** is a preference, by default `LoomDSP/Library` in the user's Documents folder ([preferences](preferences.md)).
- The module browser lists library items next to modules, marked as sub-patches. Adding one asks nothing: it is inserted **as a copy**. **Insert linked** is on the item's menu.
- Any patch file can be inserted as a sub-patch. A patch with no Patch inputs or outputs gives a tile with no ports.
- **Patch-wide modules are left out** (rule 8). Save to library writes no Transport and no Scale and tuning module, since a sub-patch never holds them. Inserting a file that has them drops them, and each of their cables becomes a Patch input or output on the new tile, as if they had been outside when it was collapsed. The status bar says so: `Arp: its Transport was left out; connect its clock ports.` A dropped Scale and tuning module's tuning and scales are not kept; the patch's own apply. Opening a library file on its own adds a Transport, as for any patch without one ([clock](clock.md)).
- Opening a library file with **File > Open** edits it as a patch.

### Linked library items

**Settled.** A linked instance takes a fresh copy from the library only when the patch loads. Between loads it behaves like any other tile.

- The patch stores, for each linked tile, the library path and that tile's own copy of the definition.
- On load, if the library file differs from a tile's copy, that copy is replaced. The status bar names the sub-patches that changed: `Updated from the library: Voice, Delay chain.` Exposed values are kept by exposed name; a value whose name is gone is dropped.
- A refreshed copy drops a Transport or Scale and tuning module the library file has gained, as inserting does (rule 8). Its cables become ports on the tile, and the status bar says so.
- If the library file is missing or does not load, the stored copy is used, and the status bar says so. *Why:* a patch must always open, even on another computer.
- **Unlink** on the tile menu makes it a normal copy.
- **Save to library** from a linked instance writes the file. No other tile in the open patch changes. Other linked tiles pick up the new file the next time the patch loads. *Why:* no action at a distance. An edit in one place never undoes a decision made in another.
- The app does not watch library files while a patch is open. *Why:* a save in another window must not change the sound during a performance.

### Undo across levels

- Edits at every level share one history ([undo](undo.md)).
- Undo and redo show the level the step changed. If it is not on screen, the canvas moves there first. *Why:* the player must see what was undone.

### Editor model

**Settled** (2026-11-07, with the maintainer on approval of the slice 2 plan).

The engine's flat graph is the only thing that plays and the only home of parameters, cables, assignments, and MIDI mappings. The editor keeps the sub-patch hierarchy beside it as an overlay: where each flat module sits, the definitions with their metadata and exposed items, the instance and port tiles, and the cables that cross a boundary, recorded in the ids of their level. Collapse and dissolve change only the overlay, and the file is written from the overlay and the graph together. [ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md) holds the decision, the options, and the invariants the overlay keeps.

- **What a level shows.** Its modules, its nested sub-patches as tiles with a name and ports, its Patch inputs and outputs as port tiles, and the cables between them. A modulation line is drawn only between two modules of the same level. A line that crosses a boundary waits for the surface panel (slice 3). *Why:* until the tile has a surface, there is nothing on it for the line to end at.
- **The breadcrumb bar appears only while a sub-patch is open.** A flat patch looks as before. *Why:* a player who never collapses anything should see nothing new.
- **Each level keeps its camera for the session,** as [Make, open, and dissolve](#make-open-and-dissolve) says. The camera of a level that is collapsed into a new sub-patch moves with it. *Why:* going in and back out should land where the player left.
- **A new sub-patch is named "Sub-patch"** ("Sub-patch 2", ...), and its definition key is made from the name. Renaming is not in this slice. *Why:* collapsing should not stop to ask a question, and the name is easy to change later.
- **Patch input and output modules at the top level count as outside when collapsing,** like the Transport (collapsing point 6). Their cables into the selection become ports. *Why:* at the top level they are the ports of a library file opened on its own (flattening rule 15), and moving one inside would take a port off that file.
- **Adding the Transport inside a sub-patch is refused** with `A sub-patch cannot hold the Transport: it lives at the top level`. *Why:* rule 8.
- **Adding a Patch input or output inside a sub-patch makes a port** on the instance's tile, named by the module's label. *Why:* inside a definition that is what the module is (flattening rule 2), and the tile should show it at once.
- **A modulation whose source and target are on different levels, or whose source is a port tile, is refused** with `Modulation cannot cross a sub-patch boundary; expose the parameter instead`, from the canvas and from the inspector, whose **Add modulation** list offers only the level's own modules. *Why:* rule 4, and until slice 3 the tile has no exposed parameter to drop onto.
- **Copy, cut, and duplicate of a selection that holds a sub-patch tile are refused** with `Cannot copy a sub-patch yet`, until the clipboard carries definitions (listed under [Copies, library, and linking](#acceptance-criteria) with the library slice). Paste of text that holds sub-patches stays refused too. *Why:* the clipboard text holds modules and cables, not definitions, so a copy would lose the inside.
- **Deleting a module takes the exposed items on it with it,** in the same undo step, at every level that re-exposed them, as Unexpose would. Deleting a port takes the cables through it on both sides. *Why:* an exposed item with nothing behind it has no value to show, and a file that named it would not load.
- **A MIDI mapping on an inner parameter is written inside the definition.** A mapping on an instance's exposed name in a file flattens to the same thing; the inside form is the one the editor writes. *Why:* one canonical form, so a file saves back to the text it loaded from.
- **Known gap: assignment order after save.** The file orders assignments inner first (flattening rule 11), while the live graph sums them in id order. A top-level assignment added before an inner one can therefore sum in a different order after save and reload. Collapse and dissolve do not create the gap, since they add no assignment. It is left to slice 3, which owns exposed parameters. *Why named here:* a player who modulates one parameter from two levels should know the save can reorder them.

### Roles

**Settled** (OQ-20, from its leaning).

- A role has a namespaced ID and a version, like a module: `loom.role.filter@1`.
- A role declares input names, output names, and core parameter names, with their kinds. It is data in the SDK, like a module descriptor.
- A new version may only add optional parameters. Every part that filled the old version still fits.
- A module or a sub-patch can fill several roles.
- A module declares its roles in its descriptor. Each role port and parameter is matched to one of the module's own by name. A declaration can map a role name to a different own name, such as the role's `out` to a filter's `lowpass`. *Why:* the roles fit existing modules without renaming their ports.
- The registry refuses a declaration whose module lacks a role's port or parameter, and names what is missing (FR-SDK-08).
- A sub-patch declares roles in its definition (FR-ENCAP-09). Its exposed parameters and ports are checked against each role when it is saved or inserted. A failed check removes that role from the definition and says why; it does not block the save.
- The initial roles are the ones in the [catalogue](../product/module-catalogue.md#roles).

### Slots

- A slot is a module inside an abstraction. It has the ports and core parameters of its role, and shows the part's panel on its tile.
- **Swap** is a menu on the slot's tile, in the inspector, and on any panel widget bound to it. It lists every module type and library sub-patch that declares the role.
- **Cables stay.** The slot's ports are the role's ports.
- **Values carry across** by role parameter name. A value the new part does not have stays stored, so swapping back restores it (FR-ENCAP-08).
- **Data** (OQ-21, settled from its leaning): each part's data is kept in the patch, keyed by the part, and comes back when the player swaps back. Data is never converted between parts.
- **The swap is click-free.** Old and new parts both run for a 10 ms equal-power crossfade, then the old part is removed. *Why:* a swap is a graph edit, and FR-GRAPH-05 asks for faded connection changes.
- **The swap is one undo step.**
- Parameters only the current part has show in a sub-panel that follows the slot (FR-PANEL-11). A panel widget bound to a role parameter keeps working after a swap.

### Modes

A sub-patch's mode is a setting in its inspector. The default is **single**.

| Mode | Inside | Ports |
|---|---|---|
| Single | One patch, as above. | Its Patch inputs and outputs. |
| Layer | Several **members**. Each member is a sub-patch. | Each outer input feeds the same-named input of every member. Each outer output is the sum of the same-named output of every member. A member without that port is skipped. |
| Select | Several members, as in layer. A **Member** parameter (stepped, modulatable) chooses one. | Each outer output comes from the chosen member. |
| Stack | One patch, run as N **copies**. | Each outer input feeds every copy. Each outer output is the sum of the copies, scaled by 1/√N. |

- **Members** are added, removed, and reordered in the inspector. A member is opened like any sub-patch.
- **Select switches click-free.** Every member runs. A change of the Member parameter crossfades the outputs over 10 ms, equal power. *Why every member runs:* a member that was paused would restart cold, and skipping members per block would make the output depend on the block size (AGENTS.md rule 2). See [Open points](#open-points) for CPU.
- **Stack** copies come from flattening, so each copy is a full set of ordinary modules. **Copies** is a whole-number setting from 1 to 16. It changes the graph, so it is not modulatable.
- **Spread** (TR-04): in stack mode, the player picks exposed parameters to spread and a spread amount for each. Copy `i` of `N` gets the parameter's value plus the amount times an offset evenly spaced from -1 to 1 (copy 0 at -1, the last at +1; one copy has offset 0). The result is clamped to the parameter's range.
- **Stereo spread:** an option in stack mode. With it on, each output becomes stereo (ADR-0020): copy `i` is panned by the same offset, equal power. *Why:* unison is stereo more often than not.
- Copies and members share the definition's data ([module data](module-data.md)).
- Stack mode is not polyphony. Voices, voice allocation, and per-voice notes are in the [polyphony](polyphony.md) specification (FR-VOICE-*). A stacked sub-patch fed a bundle runs copies times voices ([polyphony](polyphony.md#the-voice-count), rule 13).

### Patch file

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Voices"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.osc.saw@1"),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [/* ... */],
            exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff", range: Some((200.0, 2000.0)))],
            roles: ["loom.role.voice@1"],
            // panel, mode, members, copies, spread: present only when set
        ),
        "delay": SubPatch(library: Some("Delays/Tape delay.ron"), /* stored copy */),
    },
    modules: [
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 1200.0}),
    ],
)
```

- A definition's `metadata` is the same type as the patch's (name, description, authors, tags, licence), so a library item's credit travels with it ([#652](https://github.com/brylie/LoomDSP/issues/652)). Its name is what the player sees.

- Module IDs are unique within one definition, not across the patch. *Why:* a definition copied into another patch keeps its own IDs.
- Definition keys are made from the name and made unique ("voice", "voice 2"). They are not shown to the player. Each instance refers to its own key; no two instances share one.
- An instance's `params` holds the values of its exposed parameters, by exposed name.
- A definition that contains itself, directly or through a linked library file, is refused when loading or inserting, with a message naming the loop.

### Flattening

These rules are for implementers. The player never sees the flat graph.

1. The control side expands each instance into its definition's modules, recursively, before every compile.
2. A Patch input or output is removed. A cable into it continues from its other side.
3. An exposed parameter's instance value is written to the inner parameter. A modulation assignment or MIDI mapping on the exposed parameter is attached to the inner parameter.
4. Each flat module gets a `ModuleId` from a table keyed by its path (instance IDs, then the inner ID). A path keeps its ID across edits, so its state survives. A removed path's ID is not reused in the session.
5. Stack copies, layer and select members, and slot parts expand into ordinary modules, with mixing or crossfade modules the flattener adds. Added modules are not shown and not saved.
6. Flattening is deterministic: the same patch gives the same flat graph and the same IDs.
7. The flattener never opens a file. It reads a linked library item through a `Library` trait that the caller implements: the desktop app reads the library folder, and a test or the web runtime passes a map. A missing item falls back to the stored copy (rule 5). *Why:* `loom-patch` must work on WebAssembly, which has no files (hard rule 6), and the loop check needs to read links while it follows them.
8. **Ports with nothing behind them.** A cable or assignment from an instance output whose Patch output has no cable behind it is dropped; the outer input reads its default. A cable into an input port that feeds nothing inside is dropped too.
9. **Pass-through ports.** A Patch output fed straight from a Patch input is a pass-through: the outer cable continues from whatever feeds the instance's input, through any number of levels. A pass-through fed by its own output carries nothing and is dropped.
10. **An assignment never starts at a port.** A modulation assignment inside a definition whose source is a Patch input's `out` is refused. Collapsing never makes one (point 4 turns an outside-to-inside assignment into an exposed parameter).
11. **Assignment order.** Each instance's inner assignments are added before the level's own, instances in id order, depth first, then the level's own in file order. The engine sums assignments on one parameter in that order, so collapsing must keep it (rule 2).
12. **Exposed through levels.** `Exposed.module` may name a nested instance; `param` is then one of that instance's exposed names. The value is clamped to each level's range, outer first. An exposed value is written into the inner module before it is resolved, and a saved control range on that parameter is dropped, so the loader picks a range that holds the value (control-ranges.md, rule 7).
13. **Port names are not made unique by the flattener.** Two Patch inputs or two Patch outputs of one definition with one name are an error. The editor makes names unique when collapsing (point 3). A Patch input or output with no label is called `in` or `out`.
14. **An instance's `label`, `position`, and `size` are the editor's.** The flattener ignores them. An instance's `ranges` is refused, as any parameter the definition does not expose is.
15. **At the top level, Patch input and output are ordinary modules**, as when a library file is opened on its own: the input outputs silence, the output reads its input and does nothing.
16. **No two instances share a key.** A file whose instances name one definition twice, at one level or at two, is refused, naming the key and both instances (`modules 1 and 2 › 5`). *Why:* rule 3. The editor never writes one; a merge or a hand edit can.
17. **Two cables into one input inside a definition are refused.** A flat patch keeps the later cable; flattening cannot keep that order across a boundary, so the file says which it means. The editor never makes two.
18. **Errors and notes from inside a definition carry its name** and the inner module's own id, such as ``in sub-patch "Voice": module #3 has no input port `inn` ``. Unknown modules at every level are reported in one note.

## Rules

1. **Encapsulation has no run-time cost.** The engine runs only flat graphs. *Why:* FR-ENCAP-07. The player should not trade clarity for CPU.
2. **Collapsing and dissolving never change the sound.** *Why:* organising a patch is not editing it.
3. **No action at a distance.** Every tile has its own definition, and no edit, save, or paste changes another tile during a session. A link only refreshes a tile's own copy when the patch loads. *Why:* settled with the maintainer. Each sub-patch is a distinct part of the project in the player's mind, and propagation would make them track every copy before any local edit.
4. **Nothing crosses a level except through ports and exposed items.** Assignments, cables, and mappings stay inside their level. *Why:* a sub-patch must work the same wherever it is used (modulation rule 10).
5. **A patch always opens.** Linked items carry a stored copy. *Why:* a missing library file on another computer must not lose the set.
6. **Swaps and select changes are click-free.** *Why:* they happen during performance.
7. **One edit is one undo step, at any level.** *Why:* the [undo](undo.md) rules apply unchanged.
8. **Patch-wide modules live only at the top level.** A sub-patch never holds the Transport or the Scale and tuning module, so stack copies, layer and select members, slot parts, and linked items refreshed on load never repeat them. Inside a sub-patch, adding one is refused, and a paste leaves them out: their cables are dropped, since a cable cannot reach the top level (rule 4), and the status bar says so. *Why:* settled with the maintainer. A patch has one transport and one scale ([clock](clock.md), [scale and tuning](scale-and-tuning.md)), and one place to set them. The same reason keeps a paste from adding a transport ([clipboard](clipboard.md), rule 6).

## Acceptance criteria

Collapse and navigation:

- [x] Collapsing three modules with two cables in and one out gives a tile with two inputs and one output, and the rendered output is bit-identical before and after (tests: `crates/loom-patch/src/hierarchy/tests.rs`, `crates/loom-modules/tests/subpatches.rs`).
- [x] Dissolving that tile gives back the same modules and cables (test: `crates/loom-patch/src/hierarchy/tests.rs`, `crates/loom-app/src/levels/tests.rs`).
- [x] An outside assignment to an inner parameter becomes an assignment to the exposed parameter (test: `crates/loom-patch/src/hierarchy/tests.rs`).
- [x] Opening a sub-patch shows its inside and the breadcrumb; the breadcrumb and Escape go back; each level keeps its camera (`Ui` harness: `crates/loom-app/src/levels/tests.rs`).
- [x] Undo of an edit made inside a sub-patch, while the canvas shows the top level, moves the canvas inside and reverts it (test: `crates/loom-app/src/levels/tests.rs`).

Copies, library, and linking:

- [ ] Pasting a sub-patch tile and editing inside the copy does not change the original (test).
- [ ] Saving a sub-patch to the library writes a patch file that opens on its own and plays the same (test).
- [x] Collapsing a selection that includes the Transport leaves it outside, and its cables into the selection become input ports (test: `crates/loom-patch/src/hierarchy/tests.rs`).
- [ ] Inserting a patch file that has a Transport and a Scale and tuning module drops both, turns their cables into ports on the tile, and says so in the status bar; the patch still has one of each (test).
- [ ] Two linked instances of one library file have separate definitions in the saved patch (test).
- [ ] Editing inside one of them unlinks it and leaves the other unchanged and linked (test).
- [ ] Save to library from one linked instance leaves the other instance unchanged in the open patch; reloading the patch updates it (test).
- [ ] A linked instance picks up a changed library file on load, keeps exposed values by name, and the status bar names it (test).
- [ ] A missing library file loads the stored copy with a status message (test).
- [ ] A definition that contains itself through a library link is refused with a message naming the loop (test).

Engine:

- [x] A patch with three levels of nesting renders bit-identical to the same patch built flat, at block sizes 1, 77, and 512 (test: `crates/loom-modules/tests/subpatches.rs`).
- [x] A feedback loop that crosses a sub-patch boundary gets a unit delay, as a flat one does (test: `crates/loom-modules/tests/subpatches.rs`).
- [x] Editing inside a sub-patch keeps the state of the other inner modules (test: `crates/loom-modules/tests/subpatches.rs`, a module added inside while a note plays leaves the render unchanged).
- [ ] No allocation on the audio thread during a slot swap or a select change (`no_alloc` test).

Roles, slots, and modes:

- [ ] The registry refuses a role declaration that misses a role parameter, naming it (test).
- [ ] Swapping a slot keeps cables and role parameter values; swapping back restores the first part's own values and data (test).
- [ ] A slot swap during playback has no step larger than the crossfade allows (`loom analyze`, audio QA).
- [ ] Layer mode sums members; select mode crossfades over 10 ms; stack mode with 4 copies and spread on a parameter gives each copy its offset value (tests).
- [ ] Stack mode with stereo spread gives a stereo output (test).

## Out of scope

- Polyphony and voice allocation (FR-VOICE-*): the [polyphony](polyphony.md) specification. Stack mode is not voices.
- The panel editor's tools: placing, aligning, and grouping widgets ([#76](https://github.com/brylie/LoomDSP/issues/76), [panels](panels.md#the-panel-editor)).
- Embedding a sub-patch's panel inside its parent's panel (FR-PANEL-08). After the panel editor.
- Watching library files while a patch is open.
- Converting data between parts on a swap (OQ-21).
- Sharing the library online (M4, FR-SHARE).

## Open points

- **CPU in select mode.** Every member runs, so a select of 8 members costs 8 members. Leaning: measure with the first real abstraction. If needed, pause a member that has been silent and unselected for a while, decided at a fixed sample count, not a block, so output stays block-size invariant.
