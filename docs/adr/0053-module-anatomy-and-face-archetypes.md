# ADR-0053: Module anatomy and face archetypes

- **Status:** Accepted
- **Date:** 2026-11-27
- **Related requirements:** FR-CANVAS-10 (each tile shows its panel), FR-CANVAS-11 (level of detail), FR-PANEL-09 (one panel model), NFR-UX-01 (keyboard and focus), NFR-UX-06 (screen readers), NFR-PERF-04 (no redraw while nothing changes)
- **Related decisions:** builds on [ADR-0013](0013-modules-carry-their-own-panel.md) (modules carry their panel) and [ADR-0025](0025-one-design-language-and-derived-panels.md) (one design language); [ADR-0052](0052-signal-colour-and-shape.md) (signal colour and shape); [ADR-0041](0041-own-ui-layer.md) (our own UI layer); [ADR-0011](0011-everything-is-a-module.md) (no timeline)
- **Specification:** [Module faces](../design/module-faces.md); [Visual system](../design/visual-system.md#module-anatomy)
- **Research:** [UI redesign canvas](../research/2026-11-ui-redesign.md)

## Context

ADR-0013 puts a module's panel on its tile, and ADR-0025 asks for one design language. Neither says how a tile is laid out, or what picture a module shows. Today a few modules have read-only shape previews (Envelope, LFO, Ladder), each drawn its own way. The visual system's design direction of 2026-11-19 asks a panel to answer three questions: what have I set, what can I change, and what is happening now.

Principles 11, 12, and 13 ask that every decision teaches, that a player learns a control once, and that a player can see how a sound is made. A catalogue of about 170 module types cannot meet that if each one draws a picture of its own. Pictures that move also cost frames, and a 300-module patch must not pin a core. A screen reader user needs the same information in words, and a player who turns motion off needs it without motion.

## Options considered

1. **A picture of its own for each module.** Each face fits its module exactly. But a player learns about 170 pictures, the build cost grows with the catalogue, and accessibility and frame cost are decided one module at a time.
2. **Panels only, no faces.** Controls and lights, as now. Cheap and consistent, but it shows values, not how the module works, so it misses principles 11 and 13.
3. **One anatomy for every module, and a small fixed set of face archetypes.** A face is one of a few kinds of picture, chosen by what a player checks first. Each archetype is specified once: what it draws, what can be dragged, when it redraws, what a screen reader hears, and how it looks with motion reduced.

## Decision

Option 3.

**One anatomy.** Every module tile has, from top to bottom:

1. **The header,** which is the handle: an icon, the name, the family word, and badges such as POLY. It is a real button: a click or Enter selects, and a drag moves. Its fill is neutral (ADR-0052).
2. **Inputs on the left edge and outputs on the right,** so a module reads left to right like the patch. Each port is a labelled row, and the whole row, at least 26 px high, is its pointer target (WCAG 2.2, SC 2.5.8, asks for 24 px). A later layout must not shrink it.
3. **The face,** one of the archetypes below.
4. **The controls,** in one knob style everywhere, with values in the units a player thinks in (ADR-0016).

Text on a tile uses only the five existing tokens: SMALL 11, BODY 12, CONTROL 13, HEADING 15, TITLE 18. Nothing on a module is smaller than 11 px.

**Twelve face archetypes.** Response curve, number line, phase ring, waveform, time trace, transfer curve, event lanes, voice lanes, levels, taps, structure, and state space. A new module takes the archetype that answers what a player checks first. A thirteenth is added only when none can, by a new decision.

**Rules every face follows.**

- **Every handle has a knob twin.** A handle on a face is a shortcut to a parameter that also has a control. Dragging is never the only way. A focused handle moves by one display step with the arrow keys, and by ten with Shift.
- **Each archetype declares how often it redraws:** never, on change, while playing, or continuously.
- **Each archetype declares what a screen reader hears, and when.** The face's name and settings are its accessible name. Live values are read while the face has focus, at most every 250 ms (ui-layer.md, section 7, rule 7), and are never pushed to a live region.
- **Each archetype has a reduced-motion form.** Nothing sweeps, scrolls, or flashes. The sound never changes with this setting.
- **One shared frame budget for moving faces** ([ui-layer.md](../design/ui-layer.md#3-change-and-cost-model), section 3, rule 9). All of them share one animation tick, at most 30 fps, and a fixed slice of the UI thread's frame time (4 ms to start), drawn in priority order: the focused face, then selected faces, then the rest by visible area. A face that keeps missing its slice steps down to 15 fps, then to redrawing on change, and says so. Faces off screen, below the minimum zoom, or behind a docked editor redraw only on change.
- **Colour follows ADR-0052.** Orange is a handle, pink is modulation acting on a parameter, white inside a face is "now", signal colours draw signals, and lamps use the light colours.

**Build first:** the response curve, the number line, and the phase ring. They cover filters and equalisers, all the arithmetic, and every phase module, which is where seeing the model helps most.

## Consequences

- **The work waits for #683.** Each archetype becomes a renderer in `loom-canvas`, on the custom element host of the UI layer, with its own accessibility nodes. Nothing changes until the migration closes.
- **`ModuleDescriptor` names its archetype,** as it names its panel today (ADR-0013), so the registry can check it. The [module faces spec](../design/module-faces.md) gives each archetype's draws, drag, redraw policy, screen-reader line, and reduced-motion form.
- **A new module's face is decided with its design.** Its specification names the archetype, and [module faces](../design/module-faces.md) lists which modules use each one.
- **Faces need telemetry.** A face that shows live values reads them through the existing bounded telemetry and display values, never by waking an idle or hidden panel.
- **A Curve module and its docked editor.** "Arrange" in the design canvas becomes a Curve module (MT-MOD-02) read by a phase, so ADR-0011 holds. Its editor docks below the canvas as a region of its own. It joins the region cycle of ui-layer.md, section 6, rule 7: Ctrl+Tab and Ctrl+Shift+Tab, with F6 and Shift+F6 as an alias (Brylie, 2026-11-27).
- **The floor holds on the canvas too, and the default zoom makes up for it** (Brylie, 2026-11-27). Tile text is 9 units for port labels and 7 for badges today (`LABEL_SIZE`, `BADGE_TEXT_SIZE`), and port rows are 16 units, so at 100% zoom tiles are under the 11 px floor and the 26 px row. Tiles grow in world units until both hold at 100% zoom, and a patch's saved positions scale with them, so its layout keeps its shape. A patch opens at a default zoom chosen so tiles look about as large on screen as they do today. Zooming in to 100% always gives the floor. *Why:* the options were to grow tiles and let every layout spread (about 1.3 to 1.5 times the area for a 300-module patch), to keep tile text small and leave reading to zoom, or this, which keeps modules compact on screen (principle 9) and still gives a readable size one zoom step away. The patch format stays at version 1 (AGENTS.md, early alpha), so the scaled positions need no migration.
- **Level of detail stays** (FR-CANVAS-11). Zoomed out, a face is a plain tile with no picture.
