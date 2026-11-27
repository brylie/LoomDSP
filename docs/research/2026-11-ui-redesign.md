# UI Redesign Canvas

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-27. The canvas and both reviews were read on this date.
- **Source:** the "LoomDSP UI redesign" design canvas, a private Design artifact: <https://claude.ai/artifact/A54rM33EepzzEpxt55dgQ3>, version `1795814958-894b`. An AI design agent made it, and the project's navigator session reviewed it twice for Brylie.
- **Used by:** [ADR-0052](../adr/0052-signal-colour-and-shape.md) (signal colour and shape), [ADR-0053](../adr/0053-module-anatomy-and-face-archetypes.md) (module anatomy and face archetypes), the [visual system](../design/visual-system.md#signal-colours-and-shapes), [module faces](../design/module-faces.md), and the [module catalogue](../product/module-catalogue.md).

This snapshot records what the canvas proposed, what the two reviews asked for, and what we measured. It is evidence, not a decision. The decisions are in the two ADRs. It is a dated record: write a new snapshot rather than update this one.

## How the work went

1. The design agent drew 15 boards: a patch view, the module anatomy, faces for each family, and many new sequencer and modulator ideas.
2. **Review 1** (navigator, 2026-11-27) kept the anatomy, the faces, and phase drawn as a ring. It found three conflicts with accepted decisions: the colour roles (ADR-0029), an "Arrange" timeline (ADR-0011), and scene morphs fixed at one bar. It also found seven accessibility problems, measured: dim unpatched jacks under 3:1, two signal colours that merge for colour-blind viewers, text under 11 px, and live faces with no text form or reduced-motion form.
3. **Round 2.** The agent replaced its placeholder colours with the real tokens, added hollow jacks, renamed Arrange to a Curve module, and drew two new boards: Colour roles and the Face spec. The navigator's second review gave the real token values, said there are five signal types and not seven, and pointed out that pink marks a modulated parameter, not the Modulation signal.
4. **This snapshot** checks the round 2 palette with our own script, against the colours `theme.rs` really draws.

## Board by board

| Board | What it proposed | What we took |
|---|---|---|
| Patch view | The whole window: modules with faces, cables behind modules until one is selected, a docked Curve editor below the canvas, `P` on a port to list destinations, `L` to listen on a cue bus | Cables behind modules and the docked editor, as ideas for later specs. The cue bus is not decided anywhere. |
| Module anatomy and signal language | Header as handle; inputs left, outputs right; face; controls. Shape is the role, fill means patched, a ring means the cable carries voices. Text at 11 px or more, from the five existing tokens. Button roles mapped onto ours | ADR-0053 (anatomy), ADR-0052 (shapes and fill). Not the voices ring: cable width already shows voices, and a ring now marks activity |
| Encapsulation and Perform surface | `G` makes a module from a selection; a closed module's face is a small map of its inside; macros and buttons gather on a Perform surface; scenes morph over a morph time, 1 bar by default | Nothing yet. The sub-patch spec and FR-LIB-23 already cover most of it. |
| Knob | One knob: an orange value arc, a pink modulation arc outside it, a native range input with `aria-valuetext` | Matches the panels we have |
| Live envelope and pinned help | One dot per voice on the envelope contour; help text that rewrites itself with the values as you drag | The transfer curve archetype, ADR-0053 |
| Displays, mixer, and shipped encapsulations | Spectrum with the filter curve over it, a Mixer with meter and fader side by side, six effects shipped as open encapsulations with their recipe shown | The levels and response curve archetypes. The recipes fit ADR-0017. |
| Groups | A labelled frame that never changes the sound, whose edge counts the cables crossing it; `F6` jumps between regions | Nothing yet. `F6` became an alias; the group keys are open (see below). |
| Reference card | A card per idea (feedback comb filter) with theory, a picture, and cited sources | Fits principle 11 and `docs/learn/`. Nothing decided. |
| Phase | Phase drawn as a violet ring everywhere: input as the outer ring, output as the inner ring, triggers as marks | The phase ring archetype and the Phase jack shape |
| Arithmetic as geometry | Two pictures for all math: a number line (adding is walking) and a time trace | The number line and time trace archetypes |
| Eleven faces for 66 modules | Every module face is one of eleven archetypes, chosen by what a player checks first | ADR-0053 |
| Note sequencers | **Phrase**, a piano roll read by a phase; **Fugue**, one subject read by four playheads, each with its own direction, rate, and transposition | Catalogue: MT-SEQ-03 updated, MT-SEQ-08 added |
| Rhythm sequencers | **Euclidean Rings**, four Euclidean rhythms on one phase; **Polymeter**, rows with their own length and rate, and a readout of when they line up again | Catalogue: MT-GATE-04 added, MT-SEQ-01 updated |
| Sequencers that behave like systems | Pendulum wave, **Coupled phases** (fireflies), **Shift register** (mutating loop), cellular **Automaton**, **Bounce**, **Markov** melody, walk on a scale with a **Quantizer**, arpeggiator | Catalogue: MT-PHASE-10, MT-SEQ-09, MT-SEQ-10, MT-MOD-05 added; MT-LOGIC-06 and MT-PITCH-02 updated |
| Modulators from nature and mathematics | Brownian motion, fractal noise, Voronoi field, Lorenz and Clifford attractors, double pendulum, logistic map, Mandelbrot and Julia orbits. Angles come out as phases | Catalogue: MT-MOD-06 to MT-MOD-12 added, one type each |
| Colour roles, round 2 | Eleven roles with the reserved colours fixed and only the signal colours searched; a distance matrix in four visions | ADR-0052, with one measured change (Phase) and one decision against its recommendation (Pitch keeps its own colour) |
| Face spec | Per archetype: what it draws, what can be dragged, when it redraws, what a screen reader hears, its reduced-motion form; a shared frame budget; the Curve module's docked editor | ADR-0053 and [module faces](../design/module-faces.md) |

## The colour roles, measured

**Method.** Contrast by the WCAG 2.2 formula. Colour difference by CIE76 in CIELAB (D65). Colour-vision simulation by Machado, Oliveira and Fernandes (2009) at severity 1, applied in linear sRGB. For each pair we take the smallest ΔE over normal vision, protanopia, deuteranopia, and tritanopia. This is the method of the existing palette tests in `crates/loom-canvas/src/theme/tests.rs`. The agent's board reproduced the first review's figures with it (ΔE 32.1, 5.2, and 4.2 for its old control and phase pair).

**Today's code** (`signal.rs`, `theme.rs`): Audio `#E04F4F` is the same red as warning, and Pitch `#F09A3E` sits next to the value orange `#F5A623`. So signal colours and state colours already collide.

**The round 2 table**, with the six reserved colours fixed at their values in `theme.rs`:

| Role | Hex | Contrast on canvas `#1C1E23` | On tile `#2B2E35` | Nearest role, worst vision |
|---|---|---|---|---|
| Audio | `#35E6F5` | 10.95 | 8.93 | 15.6, selection, protanopia |
| Modulation and Pitch | `#8399FF` | 6.33 | 5.16 | 15.7, focus, tritanopia |
| Gate | `#F6DE5F` | 12.33 | 10.05 | 15.6, value, deuteranopia |
| Phase (board) | `#8F5BCC` | 3.60 | **2.94** | 15.7, Modulation, deuteranopia |
| Editable value | `#F5A623` | 8.23 | 6.71 | 15.6, gate, deuteranopia |
| Modulation assignment | `#E86AC8` | 5.82 | 4.74 | 15.5, focus, protanopia |
| Focus | `#6CB4F0` | 7.48 | 6.10 | 15.5, pink, protanopia |
| Selection | `#F4F6F8` | 15.39 | 12.55 | 15.6, audio, protanopia |
| Warning | `#E04F4F` | 4.29 | 3.50 | 22.8, light on, deuteranopia |
| Light on | `#7CE08A` | 10.23 | 8.34 | 15.6, audio, tritanopia |
| Light off | `#777D88` | 4.03 | 3.28 | 19.6, pink, deuteranopia |

The smallest ΔE over all eleven is **15.5** (focus and modulation assignment, protanopia), above our rule of 14. The board reported the same.

**What the board missed.** It measured contrast against its own mockup fills. A jack sits on the module tile, `#2B2E35` in `theme.rs`, and there the board's Phase is **2.94:1**, under the 3:1 that SC 1.4.11 asks of a graphic. We searched violets with the other ten roles fixed. **`#9154F2`** gives 3.78 on the canvas, **3.08** on the tile, and its nearest role is ΔE **20.4** away (light off, tritanopia), so it also moves away from the Modulation colour. The overall minimum stays 15.5. Brylie chose it (2026-11-27). Phase text keeps the board's lighter tint `#C3A6EA`: 7.91 on the canvas and 6.45 on the tile.

**Option B on the board.** If Pitch kept its own colour, `#EC854D` passed at ΔE 16.0 to the value orange, but it stays in the orange family, and its shape would still be needed.

## Points the reviews settled

- **Pitch keeps a colour of its own** (Brylie, 2026-11-27, ADR-0052). He chose the board's option B over its recommended option A: Pitch is read as notes and behaves differently from modulation, and a language where two types differ only by shape and three by colour too would be inconsistent. So every type has its own colour and its own shape. Pitch is `#EC854D`: 6.38 on the canvas, 5.20 on the tile, and its nearest role is the value orange at ΔE 16.0 (tritanopia; 30.5 in normal vision). With twelve roles the smallest ΔE stays 15.5. The ochre `#AA8109` stayed further from every role (ΔE 20.9) but is only 3.79:1 on the tile, so a thin cable reads less clearly.

- **Five signal types stay** (ADR-0007). A trigger is a Gate port labelled "Trig" whose lamp flashes. A level is a Modulation port.
- **Pink is not a cable colour.** It marks a parameter being modulated, whatever the source signal type is, because a knob modulated by audio must not look like a Modulation cable.
- **White has two meanings, kept apart.** Inside a face, it means "happening now". As an outline around a module or cable, it means "selected". A face never draws a white outline.
- **Arrange is a Curve module**, read by a phase, with a docked editor. ADR-0011 holds: there is no timeline.
- **Text uses the five tokens** in `style.rs`: SMALL 11, BODY 12, CONTROL 13, HEADING 15, TITLE 18.
- **Build three faces first:** response curve, number line, phase ring.
- **F6 is an alias for the region keys** (Brylie, 2026-11-27). It needs fn on many laptops, which is fine because Ctrl+Tab is its route without fn. The Face spec and Groups boards cycled regions with F6. [ui-layer.md](../design/ui-layer.md), section 6, rule 7, had chosen Ctrl+Tab and Ctrl+Shift+Tab over F6, because F6 needs Fn on most laptop keyboards. F6 and Shift+F6 now do the same as Ctrl+Tab and Ctrl+Shift+Tab, and the docked editor joins that cycle. The spec line and the code come in another pull request.

## Open

- **Scene morph default.** The Perform board argues for 1 bar, because it lands on a downbeat. Scenes are M3 work (FR-LIB-23), and nothing is decided here.
- **The cue bus and `L` to listen.** Shown on the patch view, but no requirement or spec covers them.
- **The key that jumps between groups.** Page Down and Page Up, in reading order, once groups are built (FR-CANVAS-07, M3), plus a key without fn still to choose. The Groups board first proposed `[` and `]`, but those already follow cables (ui-layer.md, section 1, rule 3, and the Outline), so the design agent dropped them. Brylie's rule of 2026-11-27 is that every command on an F-key, Page Up or Down, Home or End, or the keypad also needs a binding without fn or a visible route, because many laptops have none of those keys. G and Shift+G were suggested, but the canvas uses G to make a module from a group. Open, for the design agent and the keys audit. The Outline stays a second way to reach a group.
- **Tile text: the floor holds, and the default zoom makes up for it** (Brylie, 2026-11-27, ADR-0053). The canvas draws port labels at 9 units, badges at 7, and port rows at 16 today, under the anatomy's 11 px and 26 px. Tiles grow in world units, and a patch opens zoomed out enough that tiles look about as large as now. Growing tiles alone would have spread a 300-module patch over about 1.3 to 1.5 times the area.
- **Filled means patched, not carrying** (Brylie, 2026-11-27; the ring's colour 2026-11-28; ADR-0052). The built activity markers use fill for "carrying signal". They move to a ring around the jack in the light-on green `#7CE08A` (8.34:1 on the tile).
- **Delete module stays a secondary button.** The Anatomy board made it a danger button. Red is kept for Panic, so a performer finds it at a glance, and a deleted module can be undone (visual-system.md, Button roles, rules 3 and 4).
- **Explainers the canvas suggests:** phase as a ring, arithmetic as geometry, and a template for reference cards, in `docs/learn/`. Wanted, not yet written ([designer brief](../design/designer-brief.md#wanted)).
