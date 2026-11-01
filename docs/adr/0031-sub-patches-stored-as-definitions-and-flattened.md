# ADR-0031: Sub-patches are stored as definitions and flattened before compiling

- **Status:** Accepted
- **Date:** 2026-11-01
- **Related requirements:** FR-ENCAP-01 to FR-ENCAP-10, FR-LIB-24, FR-PANEL-09, FR-PANEL-10, FR-SDK-08
- **Related decisions:** ADR-0001 (lock-free boundary), ADR-0002 (graph execution), ADR-0008 (patch format), ADR-0013 (modules carry their own panel), ADR-0017 (primitives, traits, and abstractions), ADR-0020 (inferred channel count)
- **Specification:** [Sub-patches](../design/sub-patches.md)

## Context

A sub-patch collapses part of a patch into one tile with its own ports and panel (FR-ENCAP-01). Sub-patches nest (FR-ENCAP-02), are saved to a user library (FR-ENCAP-05), can stay linked to that library (FR-ENCAP-06), and must cost nothing at run time (FR-ENCAP-07). Abstractions with slots and sub-patch modes build on them (FR-ENCAP-08 to 10).

Three things must be decided together, because each constrains the others:

1. How a sub-patch is written in the patch file.
2. What the compiler and the engine see.
3. What a sub-patch in the library is.

Today a patch is one flat list of modules and cables, and the compiler turns one flat `Graph` into a `Schedule` (ADR-0002).

## Options considered

**Storage in the patch file.**

1. **Inline in each instance.** Each sub-patch module carries its own modules and cables. Easy to read. But a library sub-patch used twice is written twice, and linking needs a second mechanism later.
2. **A table of definitions, and instances that refer to one.** The patch has a `subpatches` table. A sub-patch module names its definition. The file stays self-contained, and the modules list stays short and readable.
3. **Separate files.** Each sub-patch is its own file, referenced by path. Linking is free, but a patch is no longer one file, and a missing file breaks loading.

**What the engine sees.**

1. **A hierarchical engine.** The engine runs sub-patches as nested schedules. Every boundary costs a call and a buffer copy, which breaks FR-ENCAP-07, and feedback loops that cross a boundary need special handling.
2. **Flatten before compiling.** The control side expands every sub-patch into the flat graph the compiler already knows. The engine never sees a boundary.

**What a library sub-patch is.**

1. **An ordinary patch file.** Its patch input and output modules become its ports. Any patch can be inserted as a sub-patch, and any library item can be opened on its own.
2. **Its own file type.** Clearer intent, but a second format to version.

## Decision

- **Storage:** option 2. A patch has a table of sub-patch definitions. A sub-patch module refers to one by key. Every tile has its own definition; definitions are never shared between tiles. Copying a tile makes a new definition, and editing inside a tile changes only that tile. A tile linked to a library file refreshes its own copy only when the patch loads. *Why:* no action at a distance. Each sub-patch is a distinct part of the project, and a local edit must never change, or undo a decision in, another part.
- **Engine:** option 2. The control side flattens the hierarchy into one flat `Graph` before compiling. Patch input and output modules disappear in flattening; cables pass straight through them. Exposed parameters forward to the inner parameter they expose. The engine, the compiler, and the module API do not change.
- **Library:** option 1. A library sub-patch is an ordinary patch file.

## Consequences

- **No run-time cost.** A flattened patch runs exactly as if the user had built it flat, so FR-ENCAP-07 holds by construction. Feedback loops that cross a boundary are found by Tarjan's algorithm like any other loop.
- **The control side keeps two views.** The editor edits the hierarchy. The `Controller` owns the flat graph. A table maps each module's path (instance IDs down to the inner module ID) to a flat `ModuleId`. Paths keep their flat IDs across edits, so module state survives (ADR-0002's slot rule), and a removed path's ID is never reused in that session.
- **Patch files stay one file.** A patch with sub-patches is still self-contained. A linked library item is stored with its library path and a copy of its definition, so a missing library file never breaks loading.
- **One format everywhere.** Built-in abstractions (FR-LIB-24), user library items, and whole patches use the same format.
- **The CLI and the web runtime flatten too.** Flattening lives in a portable crate (`loom-patch` or `loom-core`), so `loom render` and a future browser runtime play sub-patches without the app (AGENTS.md rule 6).
- **Modes and slots are graph rewrites.** Stack, layer, select, and slot swaps all expand into ordinary modules during flattening. A swap is an ordinary graph edit, applied at a buffer boundary (ADR-0017).
- **Ports change the patch format.** `PatchModule` gains a definition reference and per-instance values for exposed parameters, and `Patch` gains the definitions table. Early alpha allows this without a migration.
