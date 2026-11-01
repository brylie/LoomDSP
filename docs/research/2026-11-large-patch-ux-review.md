# UX Review: Working in a Large Patch

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at e59132e (after #515).
- **Follows:** the persona reviews of the same day, from [P1 UX audit, round 2](2026-11-p1-ux-audit-2.md) to the [accessibility review](2026-11-accessibility-ux-review.md).
- **Used by:** issues #526 and #527, and a comment on #512. Decisions go in specs, ADRs, or the product docs.

This review reads the desktop app as Mira, the sound designer in the [personas](../product/personas-and-use-cases.md), once her patches grow: a polysynth built by hand from eight voices, a drum kit of a dozen small instruments, or an effect chain with modulation everywhere. A hundred to three hundred modules. Every earlier review used patches of six to eleven modules, where everything fits on one screen. Here the questions change: which of these twenty filters is the one that rings, where is it, and what is connected to it. Do not update this snapshot later; write a new one after the fixes.

## Method

- Generated a patch of about 300 modules (`loom-app --generate 300`) and rendered it with `--screenshot` framed to fit, as a patch opens, and at zoom 1. The generator builds repeated synthetic voices, so it shows scale, not a real design.
- Read how modules are named in the app (`module_name` in `crates/loom-app/src/modulation.rs`, the inspector, the scene), the patch format's `PatchModule` (`crates/loom-patch/src/lib.rs`), and the canvas specs: level of detail, framing, selection, the module browser, the modulation matrix's sort and filter, and clipboard.
- Read FR-CANVAS-01 to 12 and searched the open issues for comments, groups, inserting on a cable, and finding modules.

**Limits.** No real 300-module patch was built by hand, and frame rate was not measured here (the 300-module benchmark and frame meter cover it).

Priority means: **High** blocks the task; **Medium** slows the user or looks broken; **Low** is polish.

## Summary

Some of the tools for scale are already good. The modulation matrix sorts, filters by name, and can show only the selected module's assignments. Cables outside the hovered or selected modules fade to 40%. Level of detail keeps a large patch fast, and there is a 300-module frame-rate gate. Duplicate keeps incoming cables, so a voice is copied already played by the same keyboard.

What a large patch needs most is missing. Modules cannot be named: the patch format stores a label for every module, but the app neither shows nor edits it, so the twenty filters are "SVF #4" to "SVF #127" everywhere, in the matrix, MIDI learn, undo, and hover text. Nothing finds a module in the patch: the browser searches module types, not the modules you placed. The overview, framed to fit, has no words at all. And there is no way to group or annotate parts of the patch until comments, colour groups, and sub-patches arrive in M2.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Names | Modules cannot be named; the patch format already stores a label | High |
| 2 | Finding | Nothing finds a module in the patch, and nothing frames the selection | High |
| 3 | Overview | Framed to fit, a large patch shows no words | Medium |
| 4 | Structure | No groups or comments on the canvas until M2 | Medium |
| 5 | Editing | Inserting a module into a cable means dragging cables again (FR-CANVAS-08) | Low |
| 6 | Selection | No "select connected" or "select the same type" | Low |

## Findings

### 1. Modules cannot be named (High)

Every module in a patch file can carry a `label` (`PatchModule::label`, `crates/loom-patch/src/lib.rs`). The app does not show or edit it for ordinary modules ([clipboard.md](../design/clipboard.md): "The patch format has them, but the app does not show them yet"); only the placeholder for an unknown module has a label field. Everywhere else a module is named by its type and ID (`module_name`, `crates/loom-app/src/modulation.rs`): "SVF #4", "SVF #57", "SVF #127".

In a six-module patch that is fine. In a large one, the matrix's filter, MIDI learn's "Mapped CC 74 to SVF #57 Cutoff", the undo history, and every hover text name a filter that cannot be told from nineteen others without finding its number on the canvas. A name such as "Bass filter" or "Voice 3 VCA" is the cheapest structure a large patch can have.

Direction: edit the label in the inspector (and with a key on the selected module), show it as the tile's title with the type still visible (smaller, or in hover text), and use it wherever `module_name` is used today. Copy and paste keep it (clipboard.md already says so). The file format needs no change.

### 2. Nothing finds a module (High)

The module browser searches the module types you can add, not the modules in the patch. F and Home frame the whole patch. There is no way to type "bass" and jump to the module labelled "Bass filter", or to frame only the selected modules. Tab moves through modules in reading order, which helps in a small patch and is slow in a large one. In a 300-module patch, finding one module means zooming out, losing every word (finding 3), guessing, and zooming in.

Direction: a Find command (Cmd/Ctrl+F) that searches the patch's modules by label, type, and ID, selects the match, and frames it, with Enter and Shift+Enter for next and previous; and a key that frames the selection (for example Shift+F, beside F for the whole patch). The matrix's filter already searches module names; Find should use the same matching.

### 3. The overview has no words (Medium)

A patch opens at no less than the zoom where labels draw, and shows its left edge if it is too wide (#455). Zoomed out to see all 300 modules, the view is about 0.15: titles are hidden below 6.5 screen pixels and port labels below 7.5, so every tile is a coloured header over a grey bar. Family colours help a little; nothing says which block is the bass voice and which the drums.

Direction: at overview zoom, draw the names that matter at a readable screen size: module labels (finding 1) and group or note titles (finding 4), so the zoomed-out view works as a map. A minimap is an alternative, but a readable overview of the canvas itself costs less screen and stays in place. Keep it cheap: a few labels drawn at a fixed screen size, rebuilt only when the scene changes.

### 4. No groups or comments until M2 (Medium)

FR-CANVAS-07 (comments and colour groups on the canvas, Should, M2) has no issue. Notes on the canvas are #512 (from the learner review). Sub-patches, the real answer for structure, are #51 (M2). Until then, a large patch has no way to say "these 30 modules are the drum section" except by placing them close together.

Direction: plan comments and colour groups (frames) with the notes in #512, as one spec for annotations on the canvas: a frame is a coloured, titled rectangle that moves the modules inside it, and its title shows at overview zoom (finding 3).

### 5. Inserting into a cable (Low)

FR-CANVAS-08 (drop a module onto a cable to insert it, Could, M2) has no issue. In a large patch, adding a filter between two modules means dragging the old cable off and two new ones on, among many others. Not filed here; it belongs with M2 canvas work.

### 6. Selection helpers (Low)

Selection is by click, Shift-click, a box, or Cmd/Ctrl+A. There is no "select everything connected to this", "select upstream", or "select every module of this type", which are the usual ways to pick out one voice or every filter for a shared change. Not filed; worth considering with Find (finding 2), which could select all its matches.

## What works well

- **The modulation matrix scales.** It sorts by any column, filters by module or parameter name, and can show only the selected module's assignments.
- **Cables fade outside focus.** Cables not touching the hovered or selected modules draw at 40%, so the routing of one module stands out.
- **Level of detail and a frame-rate gate.** Zoomed out, tiles draw simply; the canvas is measured at 300 modules (`--generate 300 --bench`).
- **Duplicate keeps incoming cables,** so a copied voice is played by the same note and modulation sources at once (clipboard.md).
- **Copy and paste across patches,** so a voice built in one patch moves to another.
- **Reading order for Tab** is stable (top to bottom, then left to right), so keyboard traversal is predictable even in a large patch.

## Issues

Grouped to keep the backlog small. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#526](https://github.com/brylie/LoomDSP/issues/526) Name modules: show and edit the label the patch format already stores, and use it everywhere | 1 | M2 | P1 | M |
| [#527](https://github.com/brylie/LoomDSP/issues/527) Find a module, frame the selection, and a readable overview | 2, 3 | M2 | P2 | M |
| [#512](https://github.com/brylie/LoomDSP/issues/512) (comment): frames and colour groups (FR-CANVAS-07) in the same annotation spec, with titles shown at overview zoom | 3, 4 | | | |
| Not filed here: insert onto a cable (FR-CANVAS-08), selection helpers, sub-patches (#51) | 5, 6 | | | |

## Suggested order

1. Name modules (1, #526). The format already has the field; this is inspector, tile, and naming work.
2. Find and frame the selection (2, #527), using the names.
3. A readable overview (3, #527), with the frames and notes of #512 (4).
4. Sub-patches (#51) for real structure, in M2.
