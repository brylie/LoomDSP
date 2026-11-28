# Designer Brief

What a UI designer needs on day one with LoomDSP: where the tokens are, which decisions bind a design, how we measure accessibility, and what the renderer can and cannot do. This page only links. The documents it links are the source, so when one changes, this page needs no edit.

- **Kind:** living guide.
- **Status:** Agreed, 2026-11-27.
- **Why it exists:** the [UI redesign canvas](../research/2026-11-ui-redesign.md) was first drawn with placeholder colours, seven signal types, and text under our smallest size, because none of this was in one place. Two review rounds then carried it to the designer by hand.

## Design in our tokens

Propose a change as a token diff, by name, not as new values in a mockup.

| What | Where |
|---|---|
| Canvas colours: tile, text, value orange, modulation pink, lights, focus, warning, the 16 swatches | `crates/loom-canvas/src/theme.rs` |
| App colours, text sizes, button roles | `crates/loom-app/src/style.rs` |
| Signal colours | `SignalType::colour` in `crates/loom-core/src/signal.rs` |
| Port shapes | `signal_shape` in `crates/loom-app/src/scene.rs` |
| The rules for all of them, with measured contrast | [Visual system](visual-system.md): type scale, button roles, role tokens, palette tokens, [signal colours and shapes](visual-system.md#signal-colours-and-shapes), [module anatomy](visual-system.md#module-anatomy) |
| Module faces | [Module faces](module-faces.md) |

The text sizes are five: `SMALL` 11, `BODY` 12, `CONTROL` 13, `HEADING` 15, `TITLE` 18. The app's panels use nothing smaller than 11 px. On canvas tiles the same floor is planned for 100% zoom ([ADR-0053](../adr/0053-module-anatomy-and-face-archetypes.md), after #683); today tile port labels are 9 units and badges 7.

**Planned, not built:** a JSON export of the tokens, with a CI check that keeps it in sync with the code, for design tools. It is code, so it waits for M3.

## Signals

There are five signal types, and every cable is a 32-bit float at the sample rate. Types are hints: any output can be patched to any input (ADR-0007).

| Type | Meaning | Shown to the player as | Jack shape (ADR-0052) |
|---|---|---|---|
| Audio | Sound | dB, waveforms | circle |
| Modulation | Envelopes, LFOs, levels, macros | the parameter's units | diamond |
| Pitch | 0.1 per octave, 0.0 is middle C | note names and Hz | pill |
| Gate | High above 0.5. A trigger is a one-sample pulse on a Gate port | on and off; a lamp | square |
| Phase | 0 to 1, wrapping | a ring with a hand | ring with a hand |

Explainers: [pitch](../learn/pitch.md), [triggers and gates](../learn/triggers-and-gates.md), [phase and cycles](../learn/phase-and-cycles.md), [normals](../learn/normals.md) (what an unpatched input reads).

**Ports and parameters carry their own text and units** (ADR-0016): a label, a one-sentence description, and a display unit, so "1.2 kHz" comes from the descriptor, not from the design. See [port and parameter text](port-and-parameter-text.md).

## Modules

The [module catalogue](../product/module-catalogue.md) lists every planned module type, with an `MT-` ID and a milestone. A new module starts from its entry. Use the catalogue's names, or say which entry a new idea belongs to.

## Decisions that bind a design

- [ADR-0007](../adr/0007-polymorphic-signal-model.md): five signal types, hints and never locks.
- [ADR-0011](../adr/0011-everything-is-a-module.md): everything is a module; there is no arranger timeline.
- [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md): ports and parameters carry their labels, descriptions, and units.
- [ADR-0018](../adr/0018-clean-room-module-development.md): clean room. Our own names, words, and layouts; never another product's panels or text.
- [ADR-0029](../adr/0029-module-families-and-a-shared-palette.md): eight module families, the reserved colours, and how we measure colour.
- [ADR-0043](../adr/0043-unpatched-means-normalled.md): an unpatched input reads its default; an empty slot passes through.
- [ADR-0052](../adr/0052-signal-colour-and-shape.md): colour for signals and state, not families; jack shapes.
- [ADR-0053](../adr/0053-module-anatomy-and-face-archetypes.md): one module anatomy and twelve face archetypes.

## Keys

The keyboard rules and their reasons are in [UI layer](ui-layer.md), section 6. The app's shortcut list is the Keyboard shortcuts help page ([help pages](help-pages.md)). Check a new key against both before drawing it: for example, `[` and `]` already follow cables, and Ctrl+Tab, with F6 as an alias, moves between regions.

**Every key must work without fn** (Brylie, 2026-11-27). Many laptops have no Page Up, Page Down, Home, End, or keypad, and their F-keys need fn. So a command on any of those keys also needs a binding without fn, or a visible route such as a button or a menu item. F6 passes, because Ctrl+Tab does the same. Check every proposed key against this rule.

## Accessibility, and how we measure it

- **WCAG 2.2, level AA.** Text 4.5:1 (SC 1.4.3). A graphic that carries meaning, or a control's only mark, 3:1 (SC 1.4.11). Pointer targets at least 24 px (SC 2.5.8); a port's target is its whole 26 px row.
- **Colour is never the only cue** (NFR-UX-03): every role also has a shape, a pattern, or a word.
- **Colour difference:** every pair of role colours at least ΔE 14, by CIE76, in normal vision and in protanopia, deuteranopia, and tritanopia, simulated by Machado, Oliveira and Fernandes (2009) at severity 1 in linear sRGB. Measure against the colours the code draws on (the tile is `#2B2E35`), not against a mockup's fills.
- **Screen readers:** every control has a name and a value in its units; live values reach a screen reader at most every 250 ms, and never through a live region ([UI layer](ui-layer.md), section 7, rule 7).
- **Motion:** anything that moves can be stopped, and each face has a reduced-motion form (SC 2.2.2, 2.3.3; [module faces](module-faces.md)).
- **Keyboard:** everything a pointer can do, a key can do (NFR-UX-01). Where a key or a role is in doubt, follow the WAI-ARIA Authoring Practices pattern.

## Product principles

Read [the vision](../product/vision.md), above all principle 2 (patch anything to anything), 11 (every decision teaches), 12 (one design language), and 13 (let players see how a sound is made).

## What the renderer can do

- **The canvas draws on the GPU** with our own pipelines: rounded boxes, cable curves, glyphs, and shadows ([UI layer](ui-layer.md), [ADR-0041](../adr/0041-own-ui-layer.md)). No gradients, blurs, or blend modes today; ask before relying on one.
- **The system UI font**, through Parley. The app embeds no UI font, so do not rely on a mockup font's metrics.
- **Frames cost.** The canvas redraws only when something visible changes (NFR-PERF-04), and moving faces share one frame budget ([UI layer](ui-layer.md), section 3, rule 9). Say for each moving part whether it must move, or can redraw on change.
- **Desktop first.** Plugin windows and the web come later, on the same UI layer.

## Scope

The [roadmap](../product/roadmap.md) says what each milestone builds. M3, "Living systems", holds front panels, performance mode, scenes, sequencers, and most new modules. A design for something later is welcome, but say which milestone it is for.

## Wanted

Explainers in [`docs/learn/`](../learn/README.md), suggested by the design canvas and not yet written: phase drawn as a ring, arithmetic as geometry, and a template for reference cards (an idea, a picture, its theory, and cited sources).
