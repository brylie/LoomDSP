# Automatic Placement Specification

Where the app puts module tiles that have no saved position, so a loaded patch reads left to right with few crossing cables.

- **Kind:** living specification.
- **Status:** Agreed and Implemented, M1.
- **Decision behind it:** none of its own. Builds on [ADR-0010](../adr/0010-own-patch-canvas.md) (the canvas is free-form; positions belong to the app).
- **Use case and requirements:** FR-CANVAS-09 (signal flowing left to right), FR-CANVAS-01.
- **Prior art:** the first steps of layered graph drawing (Sugiyama and others, 1981): break cycles, give each node a layer, then order each layer to reduce crossings. We use only a simple form of each step.
- **Code:** `auto_layout` in `crates/loom-app/src/layout.rs`.

## Goal

> I open an example patch. I want to see where the sound starts and where it ends, and follow the main signal path without tracing tangled cables.

Every example patch is saved without positions, so this placement is the first thing a learner sees. It only decides where tiles go. A tile's face is drawn the same wherever it sits.

## Behaviour

The canvas is free-form, and tiles snap to the grid. Placement runs when a patch is loaded and some of its modules have no position. Modules that already have a position keep it. Placed modules go below them, so nothing overlaps.

Tiles are placed in columns from left to right, by how far each module is from the start of the signal. Each column is as wide as its widest tile, plus a gap for cables.

## Rules

1. **A module goes one column right of its furthest-right source.** A module with no sources goes in the first column. *Why:* signal flow reads left to right, as on most patching tools and signal diagrams.
2. **A feedback loop is opened where the signal enters it.** When every module left is waiting on a loop, the next one is the module with the most sources already placed (lowest ID on a tie). *Why:* the loop then reads left to right, and only its return cable runs backwards. Opening it at an arbitrary module put the loop's end before its start.
3. **Audio outputs go in the last column.** *Why:* the user looks for where the sound leaves the patch at the right edge.
4. **Each column is ordered by its sources.** Tiles are sorted by the average height of the sources already placed, so a tile sits near the tiles that feed it. Tiles with no placed source come last, by ID. *Why:* one sorting pass removes most crossing cables, for little code.
5. **A tile lines up with its main source.** The main source feeds the tile's first connected input. The tile's top goes level with that source's top, or lower if a tile above in its column is in the way. *Why:* a chain of modules then sits on one row, and a straight cable reads as the main signal path.
6. **No two tiles overlap, and every position is on the grid.** *Why:* overlapping tiles hide ports, and the grid keeps the canvas tidy.

## Acceptance criteria

- [x] A chain of modules is placed in one row, with each cable straight (rule 5).
- [x] In a feedback loop with an entry from outside, the module where the signal enters is placed before the rest of the loop (rule 2).
- [x] An audio output is placed in the last column, even when its source is in an earlier column (rule 3).
- [x] A tile is placed next to its source rather than above an unrelated tile (rule 4).
- [x] No overlaps and grid positions for every example patch and for a generated patch (rule 6).

## Out of scope

- Moving modules that already have a position.
- Placement of pasted modules ([clipboard](clipboard.md)) and modules added from the browser ([module browser](module-browser.md)). They go where the user is working.
- Full crossing minimisation (many sweeps, or routing cables around tiles). One pass is enough for patches of the size people write by hand.
- The `--generate` test patches, which have their own grid layout.

## Open points

None.
