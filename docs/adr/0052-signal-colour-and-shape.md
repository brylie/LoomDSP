# ADR-0052: Signal colour and shape

- **Status:** Accepted
- **Date:** 2026-11-27
- **Related requirements:** FR-CANVAS-07 (colour groups), FR-PANEL-06 (themes), NFR-UX-03 (colour is never the only signal)
- **Related decisions:** ADR-0007 (polymorphic signal model; unchanged), ADR-0025 (one design language), ADR-0018 (clean room)
- **Supersedes:** [ADR-0029](0029-module-families-and-a-shared-palette.md) in part (family header colours). Its families, its reserved colours, its 16-swatch palette, and its test method still hold.
- **Specification:** [Visual system, Signal colours and shapes](../design/visual-system.md#signal-colours-and-shapes)
- **Research:** [UI redesign canvas](../research/2026-11-ui-redesign.md)

## Context

Colour on the canvas does three jobs today, and they collide:

- **Families.** ADR-0029 gives each module family a muted header colour.
- **Signals.** Each of the five signal types has a cable and port colour (`SignalType::colour` in `signal.rs`).
- **State.** The theme reserves colours for editable values, modulation assignments, lights, focus, and warnings (`theme.rs`).

Audio `#E04F4F` is the same red as warning, so a cable can read as an error. Pitch `#F09A3E` sits next to the value orange `#F5A623`: only ΔE 5.6 apart in tritanopia. Selection uses the value orange too. And family headers echo the signal colours, so a red header and a red cable mean different things.

A modular player's first question about a cable is "what kind of signal is this?", and the second is "can I patch it there?". The module's family is easy to find without colour: its name, its icon, and the module browser carry it. Shapes are free to use, and ports have room for them.

The design canvas proposed giving colour to signals and state only, measured with our method and our real reserved colours ([research](../research/2026-11-ui-redesign.md#the-colour-roles-measured)).

## Options considered

1. **Keep ADR-0029.** Family headers keep their colours, and today's signal colours stay. No work. But audio still looks like a warning, Pitch still looks like a value, and headers compete with cables for the same hues.
2. **Signal colour, with Pitch a colour of its own** (the canvas's option B). Headers become neutral. Five signal colours, with Pitch at `#EC854D`, and each type keeps a shape of its own as the second cue. Colour alone tells the five types apart, as on most modular systems a player knows. It passes ΔE 14: Pitch is 30.5 from the value orange in normal vision, and 16.0 at worst (tritanopia). But Pitch stays in the orange family near the value colour.
3. **Signal colour, with Modulation and Pitch sharing one colour and differing by shape** (the canvas's option A, its recommendation). Headers become neutral. Four signal colours. Modulation is a diamond, and Pitch is a pill. Both are control signals that a player patches into parameters, and shape already has to carry the type, because colour is never the only cue (NFR-UX-03). But two of the five types would be told apart only by shape, while the other three differ by colour too.

## Decision

Option 2 (Brylie, 2026-11-27). This departs from the design canvas, which recommended option 3.

*Why:* Pitch is not just another control signal. It is read as notes, it doubles a frequency per octave, and a player patches it to pitch inputs. And if two types differed only by shape while the others differed by colour, the language would be inconsistent. So every signal type has a colour of its own, easy to tell apart, and that colour is the main cue. Each type also keeps a shape of its own, because colour must never be the only cue (NFR-UX-03).

**Roles and colours.** Twelve roles, each with one colour and a second cue that is not colour:

| Role | Colour | Second cue |
|---|---|---|
| Audio signal | `#35E6F5` | circle jack |
| Modulation signal | `#8399FF` | diamond jack |
| Pitch signal | `#EC854D` | pill jack |
| Gate signal | `#F6DE5F` | square jack; a trigger port is labelled "Trig" and its lamp flashes |
| Phase signal | `#9154F2` | a ring with a hand; text in its tint `#C3A6EA` |
| Editable value | `#F5A623` (unchanged) | handle shape, value text beside it |
| Modulation assignment | `#E86AC8` (unchanged) | an outer arc, outside the value arc |
| Focus | `#6CB4F0` (unchanged) | 2 px ring with a 2 px offset |
| Selection | `#F4F6F8` (was the value orange) | an outline around the whole module or cable |
| Warning | `#E04F4F` (unchanged) | an icon and a word |
| Light on, light off | `#7CE08A`, `#777D88` (unchanged) | filled lamp, ring-only lamp |

**Rules that go with it.**

- **Five signal types stay** (ADR-0007). A trigger is a Gate port, and a level is a Modulation port. No new types.
- **Shapes.** Audio stays a circle, and Gate a square. Modulation goes from round to a diamond, Pitch from square to a pill, and Phase from round to a ring with a hand.
- **Unpatched jacks are hollow, and patched ones are filled.** Every jack's ring is at least 3:1 against the tile it sits on (WCAG 2.2, SC 1.4.11).
- **Pink marks only a parameter being modulated.** It is never a cable colour. A knob modulated by an audio-rate signal must not look like a Modulation cable.
- **White inside a face means "now"** (the playhead, the sounding note). **A white outline around a module or cable means "selected".** A face never draws a white outline, and a selection never fills a mark.
- **Module headers are neutral.** A module's family shows by its name, an icon for its family (see Consequences), and its section in the module browser.
- **The measure.** Every pair of the twelve role colours is at least ΔE 14 (CIE76) apart in normal vision and in protanopia, deuteranopia, and tritanopia (Machado 2009, severity 1, applied in linear sRGB). Every role colour used as a mark is at least 3:1 against the canvas and the tile. Text in a role colour meets 4.5:1, using a tint where the colour does not. Only Phase needs one: its tint `#C3A6EA` is 7.91:1 on the canvas `#1C1E23`, 6.45:1 on the tile `#2B2E35`, and 7.36:1 on the panels' `PANEL_FILL` `#22242A`. These are ADR-0029's method and thresholds, applied to roles instead of families.

**What still holds from ADR-0029.** The eight families and `ModuleDescriptor::family`, the browser listing by family, the reserved colours, the 16-swatch palette (kept for the user colours of FR-CANVAS-07, no longer for headers), and the method of measuring. Only the family header colours, and the rule that families echo signal colours, are replaced.

**A measured change from the canvas.** The canvas's Phase was `#8F5BCC`. On the real tile `#2B2E35` it is 2.94:1, under 3:1, because the canvas measured against its own mockup fills. `#9154F2` is 3.08:1 on the tile and 3.78:1 on the canvas, and its nearest role is ΔE 20.4 away. The smallest ΔE over all twelve roles is 15.5 (focus and modulation assignment, protanopia), the same as the canvas reported for its eleven. Adding Pitch `#EC854D` does not lower it: Pitch's nearest role is the value orange at ΔE 16.0 (tritanopia), and the warning red at 16.6 (deuteranopia). Pitch is 5.20:1 on the tile.

**Selection white is one of the twelve.** It was measured like every other role, not left out as an exception: its nearest role is Audio, at ΔE 15.6 in protanopia. It is also told apart by its form, because it is always an outline around a whole module or cable. Brylie chose this value on 2026-11-27.

## Consequences

- **The work waits for #683.** Nothing changes in the app until the UI layer migration closes.
- **`signal.rs`.** `SignalType::colour` returns the new values, one per type.
- **`scene.rs`.** `signal_shape` gains a diamond, a pill, and a phase ring, and a port's view gets a hollow or filled state from whether it is patched. The canvas shader draws the new shapes.
- **`theme.rs`.** `families` leaves the header path: a tile's header uses a neutral fill. `selection` becomes `#F4F6F8`. The swatches stay for FR-CANVAS-07. The `hover` white of a port under the cursor does not collide with the selection white, because one fills a port and the other outlines a module or cable, but the spec says so.
- **Tests.** The ΔE test moves from family pairs to the twelve role colours. A new test checks each role colour's contrast on the canvas and on the tile. The test that a header uses its family colour is replaced by one that headers are neutral. Tests that check port shapes by signal type follow the new shapes.
- **Port activity moves to another cue.** The canvas already fills a port that carries signal and outlines a silent one ([panels](../design/panels.md#signal-activity-on-ports)). Filled and hollow now mean patched and unpatched (Brylie, 2026-11-27), so a carrying port shows a lit ring around its jack instead, in the light-on green `#7CE08A` (Brylie, 2026-11-27). It is a mark, at 8.34:1 on the tile, and it stands apart from the jack's own signal-coloured fill; lamps already mean "on" in that colour. That section changes when this is built.
- **A family icon.** Each family gets an icon of our own (ADR-0018), carried by `Family` or the descriptor, drawn by the canvas on the header, and shown in the browser and help. Without it, neutral headers would leave the family with no cue beyond its name.
- **Help and the browser.** A family loses its colour cue, so its name and icon must show wherever the colour used to help: the header's hover text, the browser row, and help.
- **A future light theme** (FR-PANEL-06) defines its own twelve role colours under the same names, measured the same way.
