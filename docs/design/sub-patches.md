# Sub-patches Specification

How a player collapses part of a patch into one module with its own ports and panel, opens it to edit, saves it to a library, and builds abstractions with swappable slots and modes.

- **Kind:** living specification.
- **Status:** Agreed, P2. The choices marked "Settled" were made with the maintainer on 2026-11-01. Nothing is built yet.
- **Decision behind it:** [ADR-0029](../adr/0029-sub-patches-stored-as-definitions-and-flattened.md) (definitions, flattening, library items as patch files). [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md) for roles and slots. [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) and [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) for the sub-patch's panel.
- **Use case and requirements:** FR-ENCAP-01 to FR-ENCAP-10, FR-SDK-08, FR-PANEL-10, FR-PANEL-11, FR-LIB-24, FR-MOD-06. Settles OQ-20 and OQ-21.
- **Builds on:** [Panels: exposed controls in a sub-patch](panels.md#exposed-controls-in-a-sub-patch) (what the surface shows), [Module data](module-data.md) (data in slots and copies), [Undo](undo.md), [Copy, paste, and duplicate](clipboard.md), [Changing a running patch](live-patching.md).
- **Background:** none yet. An explainer on "flattening" and "abstraction" belongs in `docs/learn/` with the first build.
- **Prior art:** Max has both private sub-patchers and shared file abstractions. Reaktor and Bitwig Grid nest containers. Design tools have "instances" of a shared component. Named only to explain the trade-offs; the rules below are our own.
- **Issues:** [#51](https://github.com/brylie/LoomDSP/issues/51) (umbrella), [#76](https://github.com/brylie/LoomDSP/issues/76) (panel editor).
- **Code:** none yet.

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
6. The sound does not change. Collapsing is one undo step.

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
- Opening a library file with **File > Open** edits it as a patch.

### Linked library items

**Settled.** A linked instance takes a fresh copy from the library only when the patch loads. Between loads it behaves like any other tile.

- The patch stores, for each linked tile, the library path and that tile's own copy of the definition.
- On load, if the library file differs from a tile's copy, that copy is replaced. The status bar names the sub-patches that changed: `Updated from the library: Voice, Delay chain.` Exposed values are kept by exposed name; a value whose name is gone is dropped.
- If the library file is missing or does not load, the stored copy is used, and the status bar says so. *Why:* a patch must always open, even on another computer.
- **Unlink** on the tile menu makes it a normal copy.
- **Save to library** from a linked instance writes the file. No other tile in the open patch changes. Other linked tiles pick up the new file the next time the patch loads. *Why:* no action at a distance. An edit in one place never undoes a decision made in another.
- The app does not watch library files while a patch is open. *Why:* a save in another window must not change the sound during a performance.

### Undo across levels

- Edits at every level share one history ([undo](undo.md)).
- Undo and redo show the level the step changed. If it is not on screen, the canvas moves there first. *Why:* the player must see what was undone.

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
- Stack mode is not polyphony. Voices, voice allocation, and per-voice notes have their own specification (FR-VOICE-*).

### Patch file

```ron
Patch(
    version: 1,
    subpatches: {
        "voice": SubPatch(
            name: "Voice",
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

## Rules

1. **Encapsulation has no run-time cost.** The engine runs only flat graphs. *Why:* FR-ENCAP-07. The player should not trade clarity for CPU.
2. **Collapsing and dissolving never change the sound.** *Why:* organising a patch is not editing it.
3. **No action at a distance.** Every tile has its own definition, and no edit, save, or paste changes another tile during a session. A link only refreshes a tile's own copy when the patch loads. *Why:* settled with the maintainer. Each sub-patch is a distinct part of the project in the player's mind, and propagation would make them track every copy before any local edit.
4. **Nothing crosses a level except through ports and exposed items.** Assignments, cables, and mappings stay inside their level. *Why:* a sub-patch must work the same wherever it is used (modulation rule 10).
5. **A patch always opens.** Linked items carry a stored copy. *Why:* a missing library file on another computer must not lose the set.
6. **Swaps and select changes are click-free.** *Why:* they happen during performance.
7. **One edit is one undo step, at any level.** *Why:* the [undo](undo.md) rules apply unchanged.

## Acceptance criteria

Collapse and navigation:

- [ ] Collapsing three modules with two cables in and one out gives a tile with two inputs and one output, and the rendered output is bit-identical before and after (test).
- [ ] Dissolving that tile gives back the same modules and cables (test).
- [ ] An outside assignment to an inner parameter becomes an assignment to the exposed parameter (test).
- [ ] Opening a sub-patch shows its inside and the breadcrumb; the breadcrumb and Escape go back; each level keeps its camera (`Ui` harness).
- [ ] Undo of an edit made inside a sub-patch, while the canvas shows the top level, moves the canvas inside and reverts it (`Ui` harness).

Copies, library, and linking:

- [ ] Pasting a sub-patch tile and editing inside the copy does not change the original (test).
- [ ] Saving a sub-patch to the library writes a patch file that opens on its own and plays the same (test).
- [ ] Two linked instances of one library file have separate definitions in the saved patch (test).
- [ ] Editing inside one of them unlinks it and leaves the other unchanged and linked (test).
- [ ] Save to library from one linked instance leaves the other instance unchanged in the open patch; reloading the patch updates it (test).
- [ ] A linked instance picks up a changed library file on load, keeps exposed values by name, and the status bar names it (test).
- [ ] A missing library file loads the stored copy with a status message (test).
- [ ] A definition that contains itself through a library link is refused with a message naming the loop (test).

Engine:

- [ ] A patch with three levels of nesting renders bit-identical to the same patch built flat, at block sizes 1, 77, and 512 (test).
- [ ] A feedback loop that crosses a sub-patch boundary gets a unit delay, as a flat one does (test).
- [ ] Editing inside a sub-patch keeps the state of the other inner modules (test: a running oscillator's phase continues).
- [ ] No allocation on the audio thread during a slot swap or a select change (`no_alloc` test).

Roles, slots, and modes:

- [ ] The registry refuses a role declaration that misses a role parameter, naming it (test).
- [ ] Swapping a slot keeps cables and role parameter values; swapping back restores the first part's own values and data (test).
- [ ] A slot swap during playback has no step larger than the crossfade allows (`loom analyze`, audio QA).
- [ ] Layer mode sums members; select mode crossfades over 10 ms; stack mode with 4 copies and spread on a parameter gives each copy its offset value (tests).
- [ ] Stack mode with stereo spread gives a stereo output (test).

## Out of scope

- Polyphony and voice allocation (FR-VOICE-*). They get their own specification. Stack mode is not voices.
- The panel editor's tools: placing, aligning, and grouping widgets ([#76](https://github.com/brylie/LoomDSP/issues/76), [panels](panels.md#the-panel-editor)).
- Embedding a sub-patch's panel inside its parent's panel (FR-PANEL-08). After the panel editor.
- Watching library files while a patch is open.
- Converting data between parts on a swap (OQ-21).
- Sharing the library online (P4, FR-SHARE).

## Open points

- **CPU in select mode.** Every member runs, so a select of 8 members costs 8 members. Leaning: measure with the first real abstraction. If needed, pause a member that has been silent and unselected for a while, decided at a fixed sample count, not a block, so output stays block-size invariant.
