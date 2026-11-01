# ADR-0029: Module families and a shared palette

- **Status:** Accepted
- **Date:** 2026-11-01
- **Related requirements:** FR-CANVAS-03 (module browser), FR-CANVAS-07 (colour groups), FR-PANEL-06 (themes), NFR-UX-03 (colour is never the only signal)
- **Related decisions:** ADR-0018 (clean room), ADR-0025 (one design language; themes are global)
- **Specification:** [Visual system, Module families](../design/visual-system.md#module-families), [Module browser](../design/module-browser.md)
- **Research:** [Module families and colour](../research/2026-11-module-families-and-colour.md)

## Context

Modules use 17 category strings, and a tile's header colour came from its category. Eight categories had a colour and nine shared one grey, so more than half of all tiles had grey headers. Four colours failed text contrast, and some pairs looked the same to colour-blind users. FR-CANVAS-07 will also need a palette users pick from to colour modules and groups. Without a shared one, each feature would pick its own colours.

The 17 categories are too many to tell apart by colour. People hold about seven groups in mind, and categorical colour stops working well at about eight.

## Options considered

1. **Colour each category.** Seventeen colours cannot stay distinct, and categories grow with the catalogue.
2. **A family per module, coloured from a shared palette.** A small, fixed set of families sets the colour and groups the browser. The detailed category stays for search and help.
3. **No category colour.** The calmest look, but loses a quick way to find a kind of module in a large patch.

## Decision

Option 2.

- **Eight families,** in this order: Sources, Filters and effects, Modulation, Time and rhythm, Notes and pitch, Logic and math, Level and routing, Interface. Every module declares one in its descriptor (`ModuleDescriptor::family`). The category stays as a finer label for search and help. The names are our own (ADR-0018).
- **Boundary cases.** Slew and the 1-Sample Delay are in Level and routing: they shape or route any signal. Clock Divide is in Time and rhythm. The Delay Line and the All-Pass are in Filters and effects.
- **One palette.** The theme holds 16 named swatches: a colour for each family and a lighter tint of each. Family colours are muted and dark, and each echoes the signal its family mostly makes: Sources red like audio, Modulation blue, Time and rhythm violet like phase, Notes and pitch brown near pitch orange, Logic and math olive like gates. Every family colour takes the light header text. The user colour picker of FR-CANVAS-07 will offer the same 16.
- **Reserved colours stay out of the palette:** orange for editable values, pink for modulation, light green for lights, the focus blue, and the warning red.
- **No family mark on the header.** The family name shows in the header's hover text, in each module browser row beside a swatch of its colour, and in help, so colour is never the only cue.
- **Measured, then tested.** Every family colour has at least 4.5:1 contrast with its text, and every pair stays at least ΔE 14 apart in normal vision and in protanopia, deuteranopia, and tritanopia simulation. A test checks both.

## Consequences

- More than half the tiles stop being grey, and a performer can find "the modulation" or "the sources" in a large patch by colour.
- Echoing signal colours costs some distance: ΔE 14 instead of 18 with free hues. The family name in hover text, the browser, and help covers the difference.
- `ModuleDescriptor` gains a required field. Every module, including third-party ones (FR-SDK-01), declares its family; the compiler refuses one without it.
- The module browser lists modules by family, then by name, and each row shows its family.
- A future light theme (FR-PANEL-06) defines its own 16 swatches under the same names.
