# ADR-0039: The editor keeps the sub-patch hierarchy as an overlay on the flat graph

- **Status:** Accepted
- **Date:** 2026-11-07
- **Related requirements:** FR-ENCAP-01, FR-ENCAP-02, FR-ENCAP-07, FR-GRAPH-05
- **Related decisions:** ADR-0002 (graph execution and module slots), ADR-0008 (patch format), ADR-0031 (sub-patches stored as definitions and flattened), ADR-0034 (the controller keeps a flat API)
- **Specification:** [Sub-patches](../design/sub-patches.md), section "Editor model"
- **Background:** [Flattening](../learn/flattening.md)
- **Issue:** [#51](https://github.com/brylie/LoomDSP/issues/51)

## Context

ADR-0031 decided that a patch file holds a table of sub-patch definitions, and that the control side flattens them into one flat `Graph` before compiling. Slice 1 of [#51](https://github.com/brylie/LoomDSP/issues/51) built that: the nested patch format, the flattener in `crates/loom-patch/src/flatten.rs`, the `FlatIds` table that gives each module path a flat id, and the Patch input and output modules. The command line could flatten and play a nested patch. The app could not open one.

ADR-0031 also said the control side keeps two views: the editor edits the hierarchy, and the `Controller` owns the flat graph. It did not say how the editor holds its view. That is the decision here.

The app today works on the flat `Controller` graph everywhere. Canvas events connect, disconnect, move, and delete by flat id. The inspector reads and writes parameters by flat id. Undo snapshots the graph and applies the difference back to the controller. The clipboard copies and pastes groups of flat modules. The control channel names modules by flat id. MIDI learn maps a control to a flat module's parameter. Every one of these paths calls the `Controller` directly.

Slice 2 has to add collapse, dissolve, open, the breadcrumb, and undo across levels, without changing the sound (sub-patches rule 2) and without losing the state of inner modules.

## Options considered

1. **A hierarchical engine.** The engine runs nested schedules, and the editor edits the same nested structure. Rejected in ADR-0031: every boundary costs a call and a buffer copy, which breaks FR-ENCAP-07, and feedback loops across a boundary need special handling. Nothing in slice 2 changes that reasoning.

2. **A nested document as the source of truth.** The session holds the nested patch, every edit changes it, and the document is flattened again after each edit to update the controller. For: one structure, and saving is writing the document. Against: every edit path in the app calls the `Controller` on flat ids. They would all be rewritten to edit the document by path instead, and undo, the clipboard, the control channel, and MIDI learn would each need a path form. Re-flattening after every edit also has to find which flat modules changed, so that inner state is kept, and a flat patch would pay for all of this with no sub-patch in it.

3. **The hierarchy as an overlay on the flat graph.** The flat graph stays the only thing that plays and the only home of parameters, cables, assignments, and MIDI mappings. Beside it, the session keeps a `Hierarchy`: where each flat module sits (its path, in `FlatIds`), the definitions with their metadata, exposed items, and exposed-parameter defaults, the instance and port tiles (which have flat ids of their own but no graph module), and the boundary cables of each level, recorded in that level's ids. Everything else is read from the graph. For: every existing edit path keeps working unchanged, a flat patch has an empty overlay, and collapse and dissolve never touch the controller. Against: two structures must be kept in step, and the overlay needs rules that make a drift impossible.

## Decision

**Option 3.** The editor keeps the sub-patch hierarchy as an overlay on the flat graph, in `crates/loom-patch/src/hierarchy.rs`. Saving is the flattener's inverse, `unflatten`. The overlay keeps these invariants:

1. **One id space for every tile.** Every path, whether it is a module, an instance, or a port, has a flat id from `FlatIds`, and `FlatIds` is the only allocator of ids in the app. Selection, positions, sizes, scene tiles, and canvas events keep using one `u32`. At the top level a tile's level id is its flat id. Inside a definition, level ids belong to that definition and the flat id comes from the table. *Why:* one id space means no edit path has to learn a second kind of handle.
2. **One cable per input, from one side.** An input of a module at a level is fed either by a direct graph cable, with both ends members of that level, or by a boundary record, never both. The flat cable in the controller mirrors whichever is there, and connecting replaces whichever is there. *Why:* a cable that two records claim cannot be removed by either.
3. **Implied flat cables are recomputed with the flattener itself, before any change is committed.** Every overlay edit builds a candidate hierarchy, writes it out with `unflatten`, flattens it on a scratch table, computes the cable difference against the controller, applies that difference, and only then swaps the candidate in. A refusal leaves both untouched. After a collapse or a dissolve the difference is empty. *Why:* the flattener is the one definition of what a boundary cable means, and the overlay must never have a second one.
4. **The hierarchy and every tile's layout are in the undo snapshot.** A step is made when the hierarchy changed even if the graph did not, so collapse and dissolve are steps. Positions and sizes are recorded for every tile the hierarchy knows, not only graph nodes. A step records the level it was made on. *Why:* undo must restore what the player saw, and the overlay is part of that.
5. **Everything that reaches the file goes through `unflatten`.** The app has no second writer. For a patch with no definitions the output is byte-identical to the flat writer's. *Why:* one writer means one canonical text, and a flat patch saves as it did before.
6. **Exposed values round-trip through definition-side defaults.** Flattening writes an instance's exposed value into the inner module and drops that parameter's saved range, so the graph alone cannot restore the file. A definition keeps, for each exposed parameter, the value and range as loaded; `unflatten` writes those inside the definition and the current graph value, clamped, as the instance's value. A collapse captures the values at collapse time as the defaults. *Why:* a nested file must load and save back to the same text.

## Consequences

- **A flat patch behaves exactly as today.** Its overlay is empty, every tile's level id is its flat id, and its saved text does not change. Nothing a player does with a flat patch goes through new code paths except id allocation.
- **Collapse and dissolve are free, and inner modules keep their state.** Neither touches the controller: they move paths in the overlay and rewrite boundary records, and invariant 3 proves the flat cables did not change. The sound cannot change, which is rule 2 by construction, and no module is rebuilt, so an oscillator inside keeps its phase (ADR-0002's slot rule).
- **The file is the flattener's inverse.** `unflatten` and `Patch::flatten` are tested as a round trip over the nested example and over every flat example. A change to one must keep the other in step.
- **Cross-level assignments and mappings need no records.** An assignment whose source and target are at different levels is written through the exposed name or the output port it crosses, and a MIDI mapping on an inner parameter is written inside the definition.
- **Known gap: assignment order after save.** The file orders assignments inner first (flattening rule 11), while the live graph sums them in id order. A top-level assignment added before an inner one can therefore sum in a different order after save and reload. Collapse and dissolve do not create the gap, since they add no assignment. It exists for an edit made after a collapse, and it is left to slice 3, which owns exposed parameters.
- **Four call sites change their allocator.** The Transport at boot, generated layouts, the browser and probes, and paste and duplicate take ids from the table instead of `Controller::add_module`. A graph node with no path is adopted as a top-level module, so tests that add modules through the controller keep working.
- **The editor and the engine still agree on one thing only: the flat graph.** If the overlay is ever found to disagree with the controller, the controller is right, and the fix is in the overlay. *Why:* the graph is what the player hears.
