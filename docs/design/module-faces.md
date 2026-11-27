# Module Faces Specification

What a module tile shows between its ports and its controls, so a player can see how the module works, not only its values. Twelve face archetypes cover the whole catalogue. Each archetype is specified once here: what it draws, what can be dragged, when it redraws, what a screen reader hears, and how it looks with motion reduced.

- **Kind:** living specification.
- **Status:** Agreed, 2026-11-27, with ADR-0053. **Waiting for implementation after #683** (the UI layer migration). Nothing here is built.
- **Decision behind it:** [ADR-0053](../adr/0053-module-anatomy-and-face-archetypes.md) (module anatomy and face archetypes), building on [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) and [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md). Colours: [ADR-0052](../adr/0052-signal-colour-and-shape.md).
- **Use case and requirements:** FR-CANVAS-10, FR-CANVAS-11, FR-PANEL-09, NFR-UX-01, NFR-UX-03, NFR-UX-06, NFR-PERF-04.
- **Builds on:** [visual system](visual-system.md#module-anatomy) (anatomy, colours, shapes, text tokens), [panels](panels.md) and [panel widgets](panel-widgets.md) (controls, read-only shape previews), [UI layer](ui-layer.md) (custom elements, section 9; the accessibility tree and its 250 ms cap, section 7, rule 7; regions, section 6, rule 7), [telemetry](telemetry.md).
- **Research:** [UI redesign canvas](../research/2026-11-ui-redesign.md), the Faces and Face spec boards.

## Goal

A module's controls say what is set. Its face says what that means: which frequencies pass, where a cycle is, what adding does. Principles 11 and 13 ask for this. Principle 12 asks that a player learns it once, so faces come from a small set of archetypes shared across families: Multiply and Compare both show "what happens to a signal over time", so both use the time trace.

## Behaviour

### Redraw policies

| Policy | Meaning |
|---|---|
| Never | Redraws only when the patch structure changes. |
| On change | Redraws when a parameter changes, or an input crosses a display step. Near-zero cost at rest. |
| While playing | Moves only while the transport runs or a gate is high. Still otherwise. |
| Continuous | Moves every frame, within the shared frame budget below. Off screen or below the minimum zoom it falls back to On change. |

### The archetypes

Module names are the built modules' labels, or the catalogue's working names for planned ones. "Build first" marks the three to build before the rest.

| Face | Used by | Draws | Drag | Redraw | Screen reader hears, and when | Reduced motion |
|---|---|---|---|---|---|---|
| **Response curve** (build first) | SVF, Ladder, One-Pole, EQ, Crossover, All-Pass, Spectrum | Magnitude on a log-Hz axis with an octave grid; a cutoff handle; a pink band for how far the inputs move the cutoff now | Cutoff along x; resonance or gain along y | On change. The band edge is While playing when the cutoff input is patched. | "SVF low-pass. Cutoff 1.2 kHz, resonance 0.40. Cutoff input sweeps 1.2 to 1.8 kHz." Name and settings always; the sweep range while focused, at most every 250 ms. | The band is the range over the last second, as a still shape. No moving edge. |
| **Number line** (build first) | Add, Subtract, Divide, Scale, Clamp, Constant, Level Constant, Pitch Constant, Convert, Macro, Quantizer | An axis in the port's units; dots for the inputs and the output; arrows for adding; brackets for limits | Offset, limits, the constant's value | On change, in any transport state, for settings and the equation. The dots for live inputs are While playing. | "Add. A 0.64 plus B minus 0.58 plus offset 0.10 equals 0.16." The equation on focus, then at most every 250 ms while it changes. | Dots step every 250 ms; the equation text carries the values. |
| **Phase ring** (build first) | Phase, Phase Reset, Phase Reverse, Phase Scale, Phase Shift, Phase Trigger, Phase Wrap and Fold, Clock, Transport, Euclidean Rhythm, Euclidean Rings, Coupled phases | A track ring, an arc from 0, and a hand. Phase modules: the outer ring is the input, the inner ring the output. Event modules: marks on the ring | The hand, for shift or rotation; scroll on the ring to multiply | While playing | "Phase Scale, 3 over 2. Input 0.37, output 0.56. Both meet at 0 every 2 bars." Ratio and meeting point always; positions while focused, at most every 250 ms. | The hand steps once per beat instead of sweeping; the arc fills in steps. |
| **Waveform** | Sine, Saw, Triangle, Pulse, Noise, LFO, Scope | One cycle of the shape at the current settings; a dot at the current phase | Pulse width, shape handles | On change. The phase dot is While playing; Scope is Continuous. | "Saw, 3 voices: C4, E4, G4. Level minus 2 dB." On focus, and when the voice list changes. | No phase dot. Scope shows a still snapshot labelled with its age. |
| **Time trace** | Multiply, Compare, Slew, Sample and Hold, Random, Bounce, Fractal noise | The last few seconds: input faint, output bright, thresholds dashed | Thresholds and times on the trace | Continuous. Decimated to the face's width in pixels. | "Compare. Gate high. A crossed B 3 times in the last 2 seconds." Gate state on change while focused; counts on request. | No scrolling: a snapshot that refreshes each second, labelled "1 s ago". |
| **Transfer curve** | Envelope, Amplifier, Dynamics, Crossfade, Curve, Shaper | The input-to-output curve, or the contour a gate starts; one dot per active voice. For Curve, the input is its phase: the value over the whole phase span (16 bars, say), drawn left to right, with a white playhead. It is the same picture as any transfer curve whose input is a phase, never an x-to-y shaper of a signal | Breakpoints; threshold and ratio | While playing. The curve is On change; only the voice dots move. | "Envelope ADSR. Attack 4 ms, decay 180 ms, sustain 0.40, release 220 ms. 3 voices: C3 decay, E3 sustain, G3 sustain." Settings on focus; voice stages when they change, at most every 250 ms. | Voice dots hidden; a voice table below the face carries the stages. |
| **Event lanes** | Chance, Clock Divide, Logic, Memory, Selector, Button, Shift register, Automaton, Polymeter | Gate blocks and trigger ticks per lane; a lamp per output | Chance per lane, counts, selection | While playing. A lamp holds at least 100 ms, so a one-sample trigger is visible. | "Clock Divide by 3. Count 2 of 3." The count while focused and changed, at most every 250 ms. | Lamps hold on and off per beat; no flashing. |
| **Voice lanes** | MIDI Note, Voice Assigner, Voice Merge, Voice Split, Voice Spread, Markov, Phrase, Fugue | A row per voice or playhead: note, gate, level. Unused voices are dim | A per-voice offset (Voice Spread) | On change: note events, and changes to a per-voice offset or level | "3 of 8 voices: C4, E4, G4." When the held notes change, while focused. | No change needed: nothing moves between notes. |
| **Levels** | Mixer, Level Meter, Audio In, Audio Out, Join, Split, MIDI Controller | Peak and average meters in dB beside faders; mute state | Faders; a click mutes | Continuous for the meters; the fader is On change | "Mixer. Saw minus 3 dB, Pad minus 6 dB muted, Drums 0 dB. Out peak minus 9 dB." Fader values on focus; meter values on request. | Slow meter ballistics with a 1 s peak hold; no flicker. |
| **Taps** | Delay Line, 1-Sample Delay | Stems along a time axis in ms and beats; faded stems for feedback repeats | A tap along the time axis | On change. An optional playhead is While playing. | "Delay 375 ms, a dotted eighth. Feedback 0.50, 6 repeats above minus 60 dB." On focus and on change. | No playhead. |
| **Structure** | Patch Input, Patch Output, the seven slot roles, a closed sub-patch | Boundary rails with port names; dashed slots with their role; a small map of what is inside | A module from the browser into a slot | Never | "Voice sub-patch. Inputs Pitch, Gate. Output Out. Filter slot: Ladder. Envelope slot: empty, passes through." On focus. | No change needed. |
| **State space** | Attractor, Pendulum, Brownian, Voronoi field, Iterated map, Escape-time fractal, XY Scope | The system's state as a point in a plane, with a trail or the shape it lives on | The probe or the start point | Continuous. A fixed trail length; still backgrounds (a fractal, an attractor's cloud) are cached and redrawn On change. | "Lorenz attractor, left wing. X minus 0.40, Z 0.53." Wing or cell changes while focused; values on request. | Trail frozen; the point steps every 250 ms over the cached shape. |

### Choosing a face for a new module

1. Ask what a player checks first: where in a cycle, which frequencies, which value, or which events.
2. Take the archetype that answers it. Add a thirteenth only when none can, by a new decision (ADR-0053).
3. Put the main control on the face as a handle, and keep the same control as a knob.
4. Write the face's screen-reader line first. If it is hard to say, the face is doing too much.

### Rules

1. **Every handle has a knob twin.** A handle on a face edits a parameter that also has a control on the tile. Dragging is a shortcut, never the only way. A focused handle moves by one display step with the arrow keys, and by ten with Shift. *Why:* a pointer gesture must have a keyboard path (NFR-UX-01, WCAG 2.2 SC 2.5.7), and a player who learns the knob already knows the handle.
2. **The face's name and settings are its accessible name, always.** Live values are read only while the face has focus, at most every 250 ms, only when they changed, and never through a live region. A key may also read the face's full live state once; which key is decided with the first face (out of scope). *Why:* a live region on a moving face would talk over the music and every other announcement.
3. **Reduced motion: nothing sweeps, scrolls, or flashes.** Moving marks step at beat or 250 ms boundaries, traces become snapshots labelled with their age, and lamps hold instead of flashing. The sound never changes with this setting. *Why:* WCAG 2.2 SC 2.2.2 and 2.3.3.
4. **Colour follows ADR-0052.** White inside a face marks only what is happening now: the playhead, the sounding note, the live point. A face never draws a white outline, because a white outline means a selected module or cable. Orange marks only what can be dragged or typed. Pink marks only modulation acting on a parameter. A curve, trace, or ring takes the colour of the signal type it shows. Lamps use the light colours, never white or a signal colour. Marks are at least 3:1 against the face.
5. **One shared frame budget.** Moving faces share one animation tick, at most 30 fps, and a fixed slice of each frame, and step down when they miss it. The budget covers every moving element, so it is [ui-layer.md](ui-layer.md#3-change-and-cost-model), section 3, rule 9. A Continuous face is one that asks for frames under that rule.
6. **Faces read bounded telemetry.** A face that shows live values reads display values or probes through the existing queues, and never wakes an idle or hidden panel ([telemetry](telemetry.md)).
7. **Level of detail.** Below the minimum zoom, a tile draws no face (FR-CANVAS-11).

### The Curve module's docked editor

**Status:** Agreed, 2026-11-27. **Waiting for implementation after #683.**

The design canvas's "Arrange" is a Curve module (MT-MOD-02): breakpoint curves read by a phase, each lane one Modulation output. There is no timeline (ADR-0011). Its face is the transfer curve in compact form, with all lanes stacked and the playhead in white. Its full editor docks below the canvas:

- **Where.** Full width, in a bottom dock region. One docked editor at a time; opening another replaces it. It is resizable by its top edge, and the height is remembered per patch. It never covers the right column.
- **Opening** (Brylie's decision, 2026-11-28). The Edit button on its face, by pointer, or Enter or Space while it has focus. The header stays the module's select-and-move handle, so a click or Enter on it only selects. The canvas does not move, and the module keeps its selection outline while its editor is open. Focus moves to the editor's first lane.
- **Focus.** The dock is a region, not a modal. It joins the region cycle of [ui-layer.md](ui-layer.md), section 6, rule 7: Ctrl+Tab and Ctrl+Shift+Tab, or F6 and Shift+F6 as an alias, move through the canvas, the dock, the right column, and the toolbar.
- **Panel model.** The dock is a panel of its own in the window's root panel, in a slot below the canvas, built from the Curve module's model only (ui-layer.md, section 3, rule 3). It is not an overlay: it takes no input from the canvas, and the canvas keeps its keys while the dock does not have focus. Inside, Tab moves between lanes, the arrows move between breakpoints, and a handle has the same keys as any handle (rule 1).
- **Closing.** Escape, or Close in its title bar, closes it and returns focus to the Curve module that opened it. Deleting that module closes the editor.

## Acceptance criteria

None is ticked: the work starts after #683.

- [ ] `ModuleDescriptor` names one archetype for each module's face, and the registry checks it.
- [ ] The response curve, number line, and phase ring faces are built, each with its screen-reader line and reduced-motion form, and tested with the `loom-test` and `Ui` harnesses.
- [ ] Every face handle has a knob twin on the same parameter (a registry test over every descriptor).
- [ ] A focused face announces live values at most every 250 ms, only when they change, and never through a live region (`ui.a11y_tree()` test).
- [ ] With reduced motion on, no face moves between beat or 250 ms boundaries (headless test on the frames drawn).
- [ ] No face draws a white outline (test on the paint list).
- [ ] With 300 modules on screen and every face moving, face drawing stays within its slice, and faces step down in the order of rule 5 (`--measure` bench).
- [ ] The Curve module's docked editor joins the Ctrl+Tab cycle in the order of the Curve section, returns focus to its module on Escape, and closes when the module is deleted (headless `Ui` test).

## Decisions made for this spec

| Decision | Choice | Where |
|---|---|---|
| How the Curve module's docked editor opens (Brylie, 2026-11-28) | The Edit button on its face, by pointer, Enter, or Space. The header stays the select-and-move handle, because a click and Enter on it both select (ui-layer.md, `App::clicked`) | The Curve module's docked editor |

## Out of scope

- The other nine archetypes' renderers, until the first three are built and reviewed.
- Faces on front panels (FR-PANEL-01): they follow once tiles have them.
- The cue bus and listening to a port, shown on the design canvas's patch view.
- A key that reads a face's full live state at once. The design canvas used S. It is decided with the first face, against the canvas's letter shortcuts (panels.md, Keys), and like every letter it needs no fn key.

## Open points

None. The frame budget's rule is agreed; its slice, 4 ms to start, is set by measurement when the first moving face is built (ui-layer.md, section 3, rule 9).
