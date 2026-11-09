# Flattening: how a sub-patch costs nothing

A sub-patch is a group of modules shown as one tile. The player sees a box with ports. The engine never sees the box at all. This explainer shows what happens in between, why the obvious way to run a box inside a box was rejected, and how a module inside a sub-patch keeps its state while you edit around it.

- **Specification:** [sub-patches](../design/sub-patches.md), the section "Flattening".
- **Decision:** [ADR-0031](../adr/0031-sub-patches-stored-as-definitions-and-flattened.md).
- **Code:** `crates/loom-patch/src/flatten.rs`, and `hierarchy.rs` for the editor's side.

## Start from a real patch

[`examples/patches/nested-subtractive.ron`](../../examples/patches/nested-subtractive.ron) is the subtractive example built as three levels. The top patch holds a MIDI note input, a **Voice** sub-patch, the audio output, the transport, and a scope. The Voice holds a saw, a **Filter** sub-patch, an amplitude envelope, and a VCA. The Filter holds the state variable filter, its envelope, and a scaler.

Each level is written once in the file, in a `subpatches` table, and used by an instance:

```ron
subpatches: {
    "filter": SubPatch(
        metadata: Metadata(name: "Filter"),
        modules: [
            PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
            PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0}),
            PatchModule(id: 6, kind: "loom.struct.output@1", label: Some("out")),
            // ...
        ],
        exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff", range: Some((20.0, 20000.0)))],
    ),
    // ...
},
modules: [
    PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 300.0}),
    // ...
],
```

Three things to notice. The Patch input and Patch output modules are ordinary entries in the list: they are the ports. The Filter stores a cutoff of 1000 Hz, but the top-level instance says 300 Hz through the exposed parameter. And module ids start at 1 inside every definition; they only have to be unique within their level.

## What the engine gets

Before the patch is applied to the engine, `loom-patch` flattens it. The result is a plain graph of ten modules, the same ten as `examples/patches/subtractive.ron`:

| Flat id | Path | Module |
|---|---|---|
| 1 | 1 | MIDI note |
| 2 | 2 | *(the Voice instance; it owns the id, but there is no module)* |
| 12 | 2 › 4 | Saw |
| 13 | 2 › 5 › 3 | State variable filter, cutoff 300 Hz |
| 14 | 2 › 5 › 4 | Filter envelope |
| 15 | 2 › 5 › 5 | Scaler |
| 16 | 2 › 6 | Amplitude envelope |
| 17 | 2 › 7 | VCA |
| 9 | 9 | Audio out |
| 10 | 10 | Transport |
| 11 | 11 | Scope |

The **path** is where a module came from: the ids of the instances it is inside, then its own id. The ports are gone. The cable from the note's `pitch` to the Voice's `pitch` port, and the cable from the port to the saw inside, became one cable from module 1 to module 12. The cable out of the Filter's `out` port fans out to the VCA and, through the Voice's `filter` port, to the scope. The exposed cutoff was written into module 13 before it was resolved, clamped to the exposed range.

A test renders this file and the flat one at host buffers of 1, 77, and 512 frames and asserts the output is bit-identical (`crates/loom-modules/tests/subpatches.rs`). That is what "encapsulation has no run-time cost" (FR-ENCAP-07) means: not "cheap", but "the same".

## The wrong turn: a box that runs a box

The obvious design is a hierarchical engine. A sub-patch module would hold a small engine of its own, and `process` would copy its input buffers in, run the inner schedule, and copy the outputs out. It is easy to picture and it matches the file.

It fails in three ways.

1. **Every boundary costs.** Each port is a buffer copy, and each level is a nested call per block. A player who organises a 40-tile patch into five sub-patches pays for the tidiness. The product principle is the other way round: clarity should be free.
2. **Feedback across a boundary needs special handling.** The compiler finds feedback loops with Tarjan's algorithm on one graph and runs each loop one sample at a time ([feedback loops](feedback-loops.md)). A loop that goes out of a sub-patch and back in crosses two engines. The outer compiler cannot see inside, so it cannot find the loop, cannot place the unit delay, and cannot run the loop sample by sample.
3. **Two engines mean two of everything.** Telemetry, probes, parameter smoothing, MIDI feedback, and schedule swaps would each need a nested form.

Flattening removes all three. One graph, one compiler, one engine. A loop that crosses a sub-patch boundary is found like any other loop, and a test checks that it gets a unit delay where a flat loop would.

## Why ids come from a table

The engine keeps each module's state in a slot found by its `ModuleId`. When the graph changes, a new schedule replaces the old one at a buffer boundary, and every module whose id is still there keeps its state ([hot patching](hot-patching.md)). An oscillator keeps its phase; a delay keeps its buffer.

Flattening runs again after every edit. If it numbered the inner modules afresh each time, adding one module to the Voice would shift every id after it, and every shifted module would lose its state. The filter would restart cold in the middle of a note.

So flat ids come from a table keyed by path. "2 › 5 › 3" got id 13 the first time; it gets 13 every time after, whatever is added or removed around it. A removed path keeps its entry, so its id is never handed to something else while the patch is open. Top-level ids are reserved first, instances included, so an inner module can never take the id of a module at the top that is visited later. The result is deterministic: the same patch gives the same table.

## Ports with nothing behind them

A cable can run into a Patch input that feeds nothing, or out of a Patch output that nothing feeds. Flattening drops such a cable. The input on the other side reads its default, as any unconnected input does. A Patch input cabled straight to a Patch output is a pass-through: the outer cable continues from whatever feeds the instance, through as many levels as it takes, and a pass-through fed by its own output carries nothing and is dropped too.

## The editor keeps the hierarchy beside the graph

The app shows one level at a time, and a player can collapse a selection into a sub-patch or dissolve one. The obvious way to build that is to make the nested file the editor's model and re-flatten after every edit. LoomDSP does the opposite ([ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md)): the flat graph stays the editor's model too, and the hierarchy is a record kept beside it. The record says where each module sits (its path), what the definitions are, which tiles are instances and ports, and which cables run into or out of a port, in the ids of their level. Everything else, parameters included, is read from the graph.

Two things follow. First, collapsing is free in the same sense as flattening: it moves paths in the record and turns the cables that crossed the new boundary into port records, and the graph does not change at all. The oscillator inside keeps its phase, because no module was rebuilt. Second, saving is the inverse of flattening. The file is written from the graph and the record together, and a test loads the nested example, writes it back, and checks the text is the same.

The record and the graph must never disagree about a cable. So every change to the record is checked with the flattener itself: the candidate record is written out as a file, flattened on a scratch id table, and the cables that come out are compared with the controller's. The difference is applied to the controller, and only then is the candidate kept. After a collapse or a dissolve the difference is empty, which is the proof that the sound did not change.

One id space makes this simple to use. An instance tile and a port tile have flat ids of their own, from the same table as the modules, so the canvas, the selection, and the undo history handle them as they handle any tile. At the top level a tile's id in the file is its flat id; inside a definition, the table maps the path to the flat id.

## Why the order of assignments matters

Several modulation assignments on one parameter add up. The engine starts at zero and adds each assignment's depth times its source, in the order of the assignments' ids, which is the order they were made in. With two assignments the order does not matter: `a + b` and `b + a` are the same number, bit for bit, because adding two floating-point numbers is commutative. With three it does: `(a + b) + c` and `(c + a) + b` can differ in the last bit, because floating-point addition is not associative. Nobody hears that bit, but the determinism rule says the same patch must give the same output, and a patch that reloads to a different last bit breaks it.

A nested file cannot write the order the player made. Each level lists its own assignments, so the flattener has to pick: it adds each sub-patch's inner assignments before the level's own (rule 11). A player who first modulates an exposed cutoff from outside, then adds a source inside, has made the outer one first, but the file loads the inner one first.

The editor settles this on its own side. After every change to the hierarchy, and after every assignment is added, `sync` compares the live order on each parameter with the order the file would load, and gives a new id to any assignment that sits too early, so the live order is the file's. In the usual case that is the one outer assignment, moved behind the new inner one, at the moment the player adds it. The only other time the order can change is a collapse that moves some of three or more assignments on one parameter inside: the sum is then summed in the file's order from the collapse on, which is the exception written into rule 2 of the specification. A save and reload never changes it.

## How other tools do it

Max has both private sub-patchers and shared abstractions in files, and runs them in one scheduler. Reaktor and Bitwig Grid nest containers and compile them into one program for the audio thread. The common thread is the same as here: the nesting is for the person, and the engine runs something flat. LoomDSP names the step, flattening, and keeps it in one portable crate so that the command-line renderer and a future browser build play sub-patches without the app.
