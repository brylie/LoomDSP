# ADR-0013: Modules carry their own panel

- **Status:** Accepted
- **Date:** 2026-10-28
- **Related requirements:** FR-PANEL-*, FR-CANVAS-10, FR-LIB-18, FR-VIS-01, FR-VIS-02, FR-ENCAP-03, NFR-PERF-04, NFR-UX-01
- **Related decisions:** ADR-0001 (lock-free boundary), ADR-0010 (own canvas), ADR-0011 (everything is a module)
- **Specification:** [Panels](../design/panels.md)

## Context

The first app edits parameters in an inspector: a side panel for the selected module, as in Blender. To use a filter, you look at the tile, then look away to the sidebar, then back at the tile to see what changed.

Bitwig's Grid and Eurorack work the other way. Each module carries its own controls and displays: knobs, toggles, radio buttons (such as the 6, 12, 18 and 24 dB slope buttons on a low-pass filter), lights, and graphs. A step sequencer shows its steps and its playing position, and you edit the steps right there. This is **direct manipulation**: the object shows what it can do, and you act on it where you see it.

LoomDSP already needs panels in two other places:

- **Front panels** of a patch: the performance view, and later the plugin GUI (FR-PANEL-01, FR-NATIVE-04).
- **Sub-patches,** which should behave like built-in modules (FR-ENCAP-03).

PR #58 also tried scopes as views docked below the canvas. They worked, but they were a second kind of thing, apart from the grid. They were not saved with the patch, could not go on a front panel, and did not show which signal they were looking at.

## Options considered

1. **Inspector first.** Tiles show a name and ports, and every edit happens in the sidebar. Simple to build and cheap to draw. But the user's eyes and hands are always somewhere other than the object, and tiles cannot show state.
2. **Docked views for displays.** Scopes and meters open in a strip at the edge of the window. Quick to open, but they are a separate system: not saved with the patch, not available on front panels, not inside sub-patches.
3. **Each module draws its own UI with its own code,** like VCV Rack widgets. Total freedom. But the drawing code would have to be rewritten for the web and plugin targets, it shares nothing with front panels, and third-party code would run in the UI's draw loop.
4. **One panel model at every level.** A module's tile shows its panel: controls and displays, declared as data and edited in place. A sub-patch's tile shows the sub-patch's front panel. A patch's front panel is the performance view. All three use the same widgets, the same layout format, and the same editor.

## Decision

Option 4. **Every module carries a panel, and every panel is built the same way.**

- A module's tile is its panel. You edit it directly on the canvas.
- A patch and a sub-patch are designed with the same panel editor. A sub-patch's front panel is what its tile shows in the parent patch, so a finished sub-patch looks and works like a built-in module.
- Built-in modules declare their panels as data in their descriptor, in the same format the editor produces.
- Signal displays (scope, level meter, spectrum) are modules with a display on their panel. They are not docked views. A one-step gesture adds a scope module on a cable or output.
- The inspector stays, but it is secondary: exact values, modulation ranges, MIDI learn, and settings that do not fit on the tile.
- The canvas redraws when something visible changes. A display that changes every frame animates every frame; a patch with nothing changing on screen does not redraw.

## Consequences

- **One vocabulary.** Users learn one set of controls and one editor, and use them at every level: module, sub-patch, patch, performance view, and later plugin GUI. This follows ADR-0011: the panel is part of the module, not a separate system.
- **Abstraction is the same act everywhere.** Building a sub-patch's panel is the same work as building an instrument's front panel. Both hide wiring behind controls that suit the music.
- **Panels are data.** They save in the patch file, export with it, and draw the same way on every target. No module UI code runs at draw time.
- **The canvas grows.** It must draw and hit-test controls inside tiles, size tiles per module, simplify controls when zoomed out, and keep every control reachable by keyboard (NFR-UX-01). This is more work than ADR-0010's "few controls inside modules" plan. ADR-0010 is updated to point here.
- **Modules need a display channel.** A step sequencer's position or an envelope's stage is internal state, not a port. Modules publish a few display values per block through a lock-free path (ADR-0001). The telemetry queues from [#6](https://github.com/brylie/LoomDSP/issues/6) carry port probes; module display values extend them.
- **Editing on the tile needs module data.** A step pattern is not a parameter. On-tile editors for patterns depend on module data (OQ-17, [#50](https://github.com/brylie/LoomDSP/issues/50)).
- **Redraw cost follows what is on screen.** The idle target in NFR-PERF-04 becomes "no redraw when nothing visible changes", with a separate budget while displays animate. Performance on battery matters on stage, so this budget is measured, not assumed.
- **A third-party module cannot draw anything it likes.** It picks from the widget set. New display kinds are added to the set, through the SDK, when a real module needs one.
