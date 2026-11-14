# Sub-patches Specification

How a player collapses part of a patch into one module with its own ports and panel, opens it to edit, saves it to a library, and builds abstractions with swappable slots and modes.

- **Kind:** living specification.
- **Status:** Agreed, M2. The choices marked "Settled" were made with the maintainer on 2026-11-01. The [patch file](#patch-file) and [flattening](#flattening) are Implemented (slice 1 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-06), without `library`, `roles`, `panel`, and the mode fields, which their slices add. Collapse, dissolve, open, the breadcrumb, and undo across levels are Implemented (slice 2 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-07); see [Editor model](#editor-model). Exposing parameters, the sub-patch surface, modulation and MIDI learn through it, renaming, and the canonical assignment order are Implemented (slice 3 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-07); see [Exposing](#exposing). The library, linked library items, and copies of sub-patch tiles on the clipboard are Implemented (slice 4 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-08); see [Library](#library), [Linked library items](#linked-library-items), and [Notes from building](#notes-from-building). MIDI mappings follow an exposed range in the engine ([#687](https://github.com/brylie/LoomDSP/issues/687), 2026-11-08). Roles and slots are Implemented (slice 5 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-08); see [Roles](#roles), [Slots](#slots), and [Notes from building: slots](#notes-from-building-slots). Modes are Implemented (slice 6 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-09); see [Modes](#modes) and [Notes from building: modes](#notes-from-building-modes). With them the specification is Implemented, except what waits for the panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)): the panel, and clamping a value when an exposed range narrows. Built-in abstractions, empty slots from the module browser, Remove part, and slots inside a stack are Implemented ([#700](https://github.com/brylie/LoomDSP/issues/700), settled with the maintainer on its plan, 2026-11-14); see [Built-in abstractions](#built-in-abstractions-agreed-2026-11-14), [Empty slots](#empty-slots-agreed-2026-11-14), and [How modes meet the rest](#how-modes-meet-the-rest-settled-2026-11-09).
- **Decision behind it:** [ADR-0043](../adr/0043-unpatched-means-normalled.md) (an empty slot is a pulled cable, an empty Filter or Effect slot a thru). [ADR-0044](../adr/0044-swap-fades-are-transient-at-every-copy.md) (swap fades in every stack copy). [ADR-0031](../adr/0031-sub-patches-stored-as-definitions-and-flattened.md) (definitions, flattening, library items as patch files). [ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md) (the editor keeps the hierarchy as an overlay on the flat graph). [ADR-0040](../adr/0040-stack-copies-and-mode-modules-are-implied.md) (stack copies and the modules a mode adds are implied by the flattener). [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md) for roles and slots. [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) and [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) for the sub-patch's panel.
- **Use case and requirements:** FR-ENCAP-01 to FR-ENCAP-10, FR-SDK-08, FR-PANEL-10, FR-PANEL-11, FR-LIB-24, FR-MOD-06. Settles OQ-20 and OQ-21.
- **Builds on:** [Panels: exposed controls in a sub-patch](panels.md#exposed-controls-in-a-sub-patch) (what the surface shows), [Module data](module-data.md) (data in slots and copies), [Undo](undo.md), [Copy, paste, and duplicate](clipboard.md), [Changing a running patch](live-patching.md).
- **Background:** [Flattening](../learn/flattening.md): how a sub-patch becomes plain modules, why a hierarchical engine was rejected, and why flat ids come from a table keyed by path. The explainer [Abstraction: roles, slots, and swapping](../learn/abstraction.md) covers why a swap needs a contract and why it crossfades with equal power. [Unison](../learn/unison.md) covers copies, spread, why a stack sums at 1/√N, the constant-power pan law, and how a select fade reverses without a step.
- **Prior art:** Max has both private sub-patchers and shared file abstractions. Reaktor and Bitwig Grid nest containers. Design tools have "instances" of a shared component. Named only to explain the trade-offs; the rules below are our own.
- **Issues:** [#51](https://github.com/brylie/LoomDSP/issues/51) (umbrella), [#76](https://github.com/brylie/LoomDSP/issues/76) (panel editor).
- **Code:** `crates/loom-patch/src/subpatch.rs` (the definition types), `flatten.rs` (flattening and the id table), `level.rs` (what the top patch and a definition share), `hierarchy.rs` (the editor's overlay: paths, definitions, ports, instances, boundary cables, `unflatten`, `collapse`, `dissolve`, `insert`, `unlink`), `library.rs` (the `Library` trait, a file as a definition and back, the refresh of linked copies on load, and the loop check through links), and `crates/loom-modules/src/structure.rs` (the Patch input and output modules). In the app: the library folder as a `Library` in `crates/loom-app/src/patch_file.rs`, library items in the module browser (`browser.rs`), Save to library in `file_commands.rs`, Unlink and inserting in `levels.rs`, the first edit inside a linked tile in `editing.rs`, and sub-patch tiles on the clipboard in `clipboard.rs`; tests in `crates/loom-app/src/library_tests.rs`. Flattening lives in `crates/loom-patch`, next to the patch file types and the code that applies a patch to a `Controller`. `loom-core`'s `Graph` stays the flat engine graph with no file types (ADR-0002, ADR-0034). ADR-0031 left the crate open; settled with the maintainer on 2026-11-06 ([#651](https://github.com/brylie/LoomDSP/issues/651)). Roles: `crates/loom-core/src/role.rs` (descriptors, declarations, the registry check) and `role/roles.rs` (the initial roles). Slots: `crates/loom-patch/src/slot.rs` (the file types and the definition role check), `hierarchy/slots.rs` (make, swap, finish, dissolve, the fade wiring), `crates/loom-modules/src/structure.rs` (the swap fade), `crates/loom-app/src/slots.rs` (the gestures). Modes: `crates/loom-patch/src/modes.rs` (the fields, the refusal texts), the flattener in `flatten.rs` (`flatten_stack`, `mix_members`), `hierarchy/modes.rs` (implied modules, mode edits, the Member control), `crates/loom-dsp/src/blend.rs` (the gain laws and the member fade), `crates/loom-modules/src/structure/modes.rs` (the hidden Sum, Stereo spread sum, Member, and Select), the MIDI followers in `crates/loom-core/src/controller/midi.rs` and `engine.rs`, and `crates/loom-app/src/modes.rs` (the gestures and the fan out).

## Goal

> My patch has 40 tiles. I select the oscillator, filter, and two envelopes, press Ctrl+G, and they become one tile called "Voice" with the same cables going in and out. I double-click it to change the filter, and the breadcrumb takes me back. I expose cutoff and resonance, so the Voice tile has two knobs. I save it to my library and drop it into tomorrow's patch.

> Later I build an abstraction with a filter slot. On stage I swap the state variable filter for the ladder from the tile's menu. The cables stay, the cutoff stays where it was, and nothing clicks.

## Terms

| Term | Meaning |
|---|---|
| **Sub-patch** | A module (`loom.struct.subpatch@1`, MT-STRUCT-01) whose inside is a patch. |
| **Definition** | The inside of a sub-patch: its modules, cables, assignments, exposed items, panel, and declared roles. Stored once in the patch's `subpatches` table. |
| **Instance** | A sub-patch tile in a patch. It refers to one definition and holds the values of that definition's exposed parameters. |
| **Patch input, Patch output** | Modules inside a definition (`loom.struct.input@1`, `loom.struct.output@1`, MT-STRUCT-02). Each becomes one port on the instance's tile. |
| **Level** | The patch shown on the canvas: the top patch, or the inside of an opened sub-patch. |
| **Exposed item** | An inner parameter, display, probe, or data slot that the sub-patch shows on its own surface (FR-ENCAP-03, [panels](panels.md#exposed-controls-in-a-sub-patch)). |
| **Linked** | An instance that follows a library file (FR-ENCAP-06). |
| **Role** | A contract of port and parameter names that a module or sub-patch promises to have (ADR-0017). |
| **Slot** | A module (`loom.struct.slot@1`, MT-STRUCT-03) that holds one part filling a role, at any level. An abstraction is where slots are most useful, not the only place they may be. |
| **Part** | What fills a slot: a module or a sub-patch that declares the slot's role. The current part is an ordinary module of the slot's level, hidden behind the slot's tile. |
| **Held part** | A part the slot has held before: its values, kept so swapping back restores them. |
| **Mode** | How a sub-patch runs its inside: single, layer, select, or stack (FR-ENCAP-10). |
| **Thru** | An empty slot whose role carries an audio path (Filter, Effect): its `out` carries what feeds its `in` ([normals](../learn/normals.md)). |
| **Built-in** | An abstraction that comes with LoomDSP, such as the Synth voice: a patch file bundled with the app, inserted as a copy. |

## Behaviour

### Make, open, and dissolve

| The player does | What happens |
|---|---|
| Selects modules, then **Ctrl+G** (Cmd+G), or **Make sub-patch** in the canvas menu | The selection becomes one sub-patch tile where the selection was. See [Collapsing](#collapsing). |
| Double-clicks a sub-patch's header, or presses **Ctrl+Enter** (Cmd+Enter) with one sub-patch selected | The canvas shows its inside. The breadcrumb bar shows the path. |
| Clicks a name in the breadcrumb (Patch › Voice › Filter) | The canvas shows that level. |
| Presses **Escape** when nothing else uses it | The canvas goes up one level. Escape keeps its other uses first: end a drag, clear pinned lines, clear control focus, clear the selection ([panels](panels.md)). |
| Selects one sub-patch, then **Ctrl+Shift+G**, or **Dissolve sub-patch** | Its inside is put back into the current level, cables reconnected through the old ports. The reverse of collapsing. |

- Each level keeps its own camera (pan and zoom) for the session. *Why:* going in and back out should land where the player left.
- The tile of a sub-patch shows its panel ([panels](panels.md#exposed-controls-in-a-sub-patch)). With nothing exposed, it shows only its name and ports.
- Audio keeps playing while the player moves between levels. Only what is drawn changes.

### Collapsing

1. Every cable from outside the selection into it becomes one **Patch input** inside, and one input port on the tile. Several inner inputs fed from the same outside output share one Patch input.
2. Every cable from the selection to outside becomes one **Patch output** inside, and one output port. One inner output feeding several outside inputs uses one Patch output.
3. A port takes the inner port's label, made unique with a number if needed ("in", "in 2").
4. A modulation assignment from outside to a parameter inside exposes that parameter, and the assignment targets the exposed parameter on the tile. An assignment from inside to outside gets a Patch output for its source, and targets from the tile's new port. *Why:* assignments never cross a level (modulation rule 10).
5. MIDI mappings, probes, and data move with their modules.
6. The Transport and the Scale and tuning module stay outside, even when selected (rule 8). They count as outside the selection, so their cables become ports by points 1 and 2. If only they are selected, collapse does nothing and the status bar says why.
7. The sound does not change, except for the order of a sum in the case rule 2 names. Collapsing is one undo step.

### Exposing

Exposing follows [Panels: exposed controls](panels.md#exposed-controls-in-a-sub-patch). In addition:

**Status:** Implemented for parameters (slice 3, 2026-11-07), with MIDI following an exposed range (#687, 2026-11-08). Display values and data slots wait for the widgets that would show them on the surface.

- **Expose** is a button on the inspector's row for any parameter of an inner module, and for a surface item of a nested sub-patch's tile, so a parameter can be exposed through several levels. The button names the surface it acts on: "Expose on Voice", and on an exposed row "as Cutoff" with "Unexpose on Voice". The panel editor adds its own way ([#76](https://github.com/brylie/LoomDSP/issues/76)). A row menu, when one exists, holds it too. *Why a button:* the inspector has no row menu yet, and the gesture should not wait for one. *Why name the surface:* a row on a nested tile can act on two surfaces, its own and the enclosing one.
- The surface item takes the parameter's label as its name, made unique on that surface ("Cutoff", "Cutoff 2"). Renaming an exposed item comes with the panel editor.
- An exposed parameter's value belongs to the instance. The inner control shows it, and turning either one changes it. A modulation line from a source on the tile's level ends at the surface control.
- **Unexpose** is the same button on an exposed row inside, and on each row of the tile's own inspector ("Unexpose on Voice"). It removes the surface item, and every re-exposure of it on the surfaces above, and with them, in the same undo step, every modulation assignment onto that parameter that no surface of its source's level reaches any more, since the file would have no place for them (rule 4). An assignment through a surface below stays. The inner parameter keeps the current value, and its MIDI mapping, which is written inside the definition.
- The surface can give a narrower range, in the same unit. A range comes from a file for now, and is set in the panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)). The surface control spans it, and every write to the parameter is clamped to it, from either control, a typed value, a reset, an inspector row, or the control channel; it is not rescaled. *Why:* "cutoff 200 to 2000 Hz" on the surface still reads in Hz, and an exposed parameter turns like the inner one. *Why clamp the inner control too:* the file saves the instance's value, so a value the inner control could reach outside the range would come back changed on reload. A range set through several levels is each level's range applied in turn, outer first.
- **MIDI follows the range.** Agreed (2026-11-07; the span settled 2026-11-08 with the maintainer), Implemented ([#687](https://github.com/brylie/LoomDSP/issues/687), 2026-11-08). A MIDI mapping on an exposed parameter sweeps the exposed range, as the surface control does, in every mode: Absolute and Pickup cover it from one end of the hardware travel to the other, on the parameter's own curve, and a Relative step is a share of it. A mapping's own range and invert apply inside it. The span follows the range: when the range changes or goes, the mapping follows at once, with no new learn, and a pickup waits again until the hardware reaches the value. The flattener works out the range each flat parameter gets (every level's range, outer first, as the surface control spans it) when a patch loads and after every edit to the hierarchy, and the compiler maps each control change into it, the same way the selected range reaches the engine ([control ranges](control-ranges.md), rules 4 and 8). The mapping row of such a parameter, inside and on the sub-patch tile, says in one friendly sentence that Low and High (or each Relative step) are a share of the exposed range, and names the outermost surface whose range sets it, so the player knows why the numbers mean a narrower span. A waiting pickup's ghost mark sits at the hardware's value on every control that shows the parameter, each on its own scale: the surface control and the sub-patch's inspector row on the exposed range, every surface between on its own range, and the inner knob on its selected range. Modulation is unchanged. No saved data changes. *Why map, not clamp:* a clamp alone leaves hardware travel that does nothing (on a 20 to 20 000 Hz cutoff, CC 0 to about 42 would all give 200 Hz), and a span stored when learning would go stale when the range changes; the hardware and the surface knob should always agree. *Why in the engine:* a control change is applied on the audio thread and never passes through the app, so an app-side clamp cannot see it, and a value outside the range would be saved clamped and come back changed.
- **A narrower range clamps the value.** Agreed (2026-11-08), not built yet: the panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)) builds it. When an exposed range changes so that the parameter's value is outside it, the value moves to the nearest end of the new range, as switching to a smaller selected range does ([control ranges](control-ranges.md), rule 6). A wider range keeps the value. *Why:* the surface control must show the real value, and the file saves the value clamped to the range, so a value left outside it would sound different after reload. *Why it can wait:* today only a hand-edited file or the undo and redo of an expose changes a live range, and undo and redo restore the values with it. The panel editor makes it a normal gesture.
- **Seeing the link.** Inside, an exposed control has a small square at its corner, and its hover text adds "Exposed as" and the surface name. On the surface, the hover text adds the inner module and parameter it controls ([panels](panels.md#exposed-controls-in-a-sub-patch)).

### Copies and editing

**Settled.** Each sub-patch tile is a distinct part of the patch. Nothing done to one tile ever changes another.

- **Every tile has its own definition.** Definitions are never shared between tiles, not even by two linked instances of the same library file. *Why:* the player should never have to remember where else a sub-patch was copied, or wonder what a local edit does to the rest of the patch. Keeping copies in step would also add complexity for little gain.
- **Copy, paste, and duplicate make an independent copy,** with its own new definition. A copy of a linked tile is linked to the same library file, and is still its own tile.
- **Editing inside a sub-patch changes only that tile.** If the tile is linked, the first edit inside unlinks it, so the next load does not replace the edit. The status bar says: `Voice is now a copy. It no longer follows the library.` The edit and the unlink are one undo step.
- Changing an instance's exposed values is not an edit inside, so a linked instance stays linked.

### Library

**Settled.** A library item is an ordinary patch file. Implemented (slice 4, 2026-11-08).

- **Save to library** (on the sub-patch's tile menu, or **File > Save to library** inside it) writes the definition as a patch file in the library folder. Its Patch inputs and outputs are its ports, and its exposed items and panel come along. Its patch metadata (authors, tags, licence) is the sub-patch's own, and patch info credits it as a part ([credit for parts](patch-metadata.md#credit-for-parts)).
- The **library folder** is a preference, by default `LoomDSP/Library` in the user's Documents folder ([preferences](preferences.md)).
- The module browser lists library items next to modules, marked as sub-patches. Adding one asks nothing: it is inserted **as a copy**. **Insert linked** is on the item's menu.
- Any patch file can be inserted as a sub-patch. A patch with no Patch inputs or outputs gives a tile with no ports.
- **Patch-wide modules are left out** (rule 8). Save to library writes no Transport and no Scale and tuning module, since a sub-patch never holds them. Inserting a file that has them drops them, and each of their cables becomes a Patch input or output on the new tile, as if they had been outside when it was collapsed. The status bar says so: `Arp: its Transport was left out; connect its clock ports.` A dropped Scale and tuning module's tuning and scales are not kept; the patch's own apply. Opening a library file on its own adds a Transport, as for any patch without one ([clock](clock.md)).
- Opening a library file with **File > Open** edits it as a patch.

### Linked library items

**Settled.** A linked instance takes a fresh copy from the library only when the patch loads. Between loads it behaves like any other tile. Implemented (slice 4, 2026-11-08).

- The patch stores, for each linked tile, the library path and that tile's own copy of the definition.
- On load, if the library file differs from a tile's copy, that copy is replaced. The status bar names the sub-patches that changed: `Updated from the library: Voice, Delay chain.` Exposed values are kept by exposed name; a value whose name is gone is dropped.
- Everything else that named a port or an exposed name the file no longer has goes with it, at every level: a cable into or out of the tile's port, an assignment or a MIDI mapping onto the exposed name, an enclosing sub-patch's exposure of it, and the entries above that re-exposed it. The status bar says what went, one line each: `Voice: the library file no longer has "out"; its cable was removed.` *Why:* the file says what the surface is now, and a patch must still open (rule 5).
- If the refreshed copies still cannot play together, every linked tile keeps its stored copy, and the status bar says `The library's changes could not be used (reason), so the stored copies are used.` *Why:* rule 5, for a file edited by hand or by a newer build.
- A refreshed copy drops a Transport or Scale and tuning module the library file has gained, as inserting does (rule 8). Its cables become ports on the tile, and the status bar says so.
- If the library file is missing or does not load, the stored copy is used, and the status bar says so. *Why:* a patch must always open, even on another computer.
- **Unlink** on the tile menu makes it a normal copy.
- **Save to library** from a linked instance writes the file. No other tile in the open patch changes. Other linked tiles pick up the new file the next time the patch loads. *Why:* no action at a distance. An edit in one place never undoes a decision made in another.
- The app does not watch library files while a patch is open. *Why:* a save in another window must not change the sound during a performance.

### Built-in abstractions (Agreed 2026-11-14)

The maintainer agreed these on the plan for [#700](https://github.com/brylie/LoomDSP/issues/700).

- **They ship as patch files bundled with the app** (`crates/loom-app/library/`, embedded in the binary). They are not copied into the player's library folder. *Why:* copying would write to the player's Documents folder, leave stale copies after an update, and bring back a built-in the player deleted. *Why files and not code:* a built-in is an ordinary patch, the same thing Save to library writes.
- **The module browser lists them first** in an empty search, with the mark `Built in`. Each row's text is its name, and the area under the list shows its one-sentence description, with the detail `Built in, inserted as a copy`. A row has no menu. Search finds a built-in by its name, `sub-patch`, and `built in` ([module browser](module-browser.md)).
- **Inserting one makes a copy.** There is no Insert linked: a link follows a file in the library folder, and a built-in has none. A link to a file that changes with each app update would change a saved patch when it loads (rule 3). A built-in never follows a link into the player's library folder.
- **Metadata:** authors `LoomDSP contributors`, licence `CC0-1.0`, as the example patches have, so a patch built from a built-in carries no attribution duty.
- **To learn from one,** insert it and open it, or read its example patch (`examples/patches/synth-voice.ron`, `effect-chain.ron`). Inside, modules read by their type names until module labels are shown everywhere ([#526](https://github.com/brylie/LoomDSP/issues/526)).
- **Dissolve sub-patch is left off the tile menu** of a sub-patch that holds a slot, while that refusal stands (collapsing and dissolving slots is [#702](https://github.com/brylie/LoomDSP/issues/702)).
- **Neither built-in declares a role yet.** A sub-patch that fills a role cannot change mode, so a Synth voice that filled Voice could not be stacked. [#766](https://github.com/brylie/LoomDSP/issues/766) lets a stack fill the roles its inside fills.

#### The Synth voice (MT-SRC-08)

A single sub-patch named `Synth voice`, mono, one note. Feed it a polyphonic bundle for chords ([polyphony](polyphony.md)).

- **Ports:** inputs `pitch`, `gate`, and `velocity`; output `out`, the Voice role's names.
- **Inside:**

| Module | Kind | Part and values | Wiring |
|---|---|---|---|
| Osc 1 | Sound source slot | Saw | Pitch sum into `pitch`; out to Mixer in 1 |
| Osc 2 | Sound source slot | Pulse, width 0.5, octave −1 | Pitch sum into `pitch`; out to Mixer in 2 |
| Fine tune span | `loom.math.constant` | 1/120 (one semitone) | out to Fine tune `in` |
| Fine tune | `loom.level.scale` | factor 0 | out to Pitch sum `b` |
| Pitch sum | `loom.math.add` | | Patch input `pitch` into `a` |
| Mixer | `loom.mix.mixer` | levels set so one note peaks near −6 dBFS | out to Filter in |
| Filter | Filter slot | SVF, cutoff 1200 Hz, resonance 0.3 | out to Amp in; `cutoff` input from Env amount |
| Mod envelope | Envelope slot | Envelope, attack 0.005 s, decay 0.3 s, sustain 0.2, release 0.3 s | gate in; out to Env amount |
| Env amount | `loom.level.scale` | factor 0.3 (+3 octaves at the peak) | out to Filter `cutoff` input |
| Amp envelope | Envelope slot | Envelope, attack 0.005 s, decay 0.2 s, sustain 0.7, release 0.3 s | gate in; out to Velocity `in` |
| Velocity | `loom.level.amp` | gain 1 | Patch input `velocity` into `level` (normal 1: full level unpatched); out to Amp `level` |
| Amp | `loom.level.amp` | gain set with the mixer for the −6 dBFS peak | out to Patch output `out` |

- **Surface, in order:** `cutoff` and `resonance` (the Filter slot's role parameters, so the tile menu offers `Swap Filter to Ladder`), `env amount` (range 0 to 0.5), `amp attack`, `amp decay`, `amp sustain`, `amp release`, `fine tune` (−1 to 1, which is ±1 semitone at the ends), and `level` (range 0 to 1). Exposed names are lowercase, matching role names.
- **Unison.** Set Mode to Stack, raise Copies, and spread `fine tune`. A spread `s` moves copy *i*'s pitch by `2·s·o_i` semitones (`o_i` from −1 to 1), since the spread is a share of the factor's −1 to 1 range. **A spread of 0.1 puts the outer copies at ±20 cents**, a usual unison detune; the inner copies of 4 sit at ±6.7 cents. A fresh stack's copies start in phase, so the first moments are louder until the detune spreads them ([unison](../learn/unison.md)).

#### The Effect chain (MT-STRUCT-04)

A single sub-patch named `Effect chain`.

- **Ports:** `in`, `out`.
- **Inside:** three Effect slots, labelled `Effect 1` to `Effect 3`, in series. After each slot, a `loom.mix.crossfade` labelled `Effect 1 mix` (and so on) takes the dry signal on `a` and the slot's output on `b`, with the Linear law. Its output feeds the next slot.

| Slot | Part and values | Mix |
|---|---|---|
| Effect 1 | EQ, high shelf −3 dB at 8 kHz | 1 |
| Effect 2 | Dynamics, threshold −12 dB, ratio 2, output set so the chain at defaults keeps its input's level within ±2 dB | 1 |
| Effect 3 | Delay line, time 0.3 s | 0.3 |

- **Surface:** `effect 1 mix`, `effect 2 mix`, `effect 3 mix`.
- **Why a linear mix:** a straight blend is the usual dry and wet practice. For a correlated pair (an EQ, a compressor, a short all-pass) it avoids the 3 dB rise an equal-power blend gives in the middle, and dry plus all-pass at 0.5 gives the expected comb. For an echo the two laws differ little.
- **Why an outer mix when an effect has its own:** the chain's mix stays with the slot across swaps; an effect's own `mix` goes with the effect.
- **An empty slot here is a thru,** so Remove part on an effect leaves the chain playing. A mix of 0 also bypasses it. To swap an effect, open the chain and use the slot's menu or inspector; swapping a nested slot from the outer tile is [#765](https://github.com/brylie/LoomDSP/issues/765).

### Undo across levels

- Edits at every level share one history ([undo](undo.md)).
- Undo and redo show the level the step changed. If it is not on screen, the canvas moves there first. *Why:* the player must see what was undone.

### Editor model

**Settled** (2026-11-07, with the maintainer on approval of the slice 2 plan).

The engine's flat graph is the only thing that plays and the only home of parameters, cables, assignments, and MIDI mappings. The editor keeps the sub-patch hierarchy beside it as an overlay: where each flat module sits, the definitions with their metadata and exposed items, the instance and port tiles, and the cables that cross a boundary, recorded in the ids of their level. Collapse and dissolve change only the overlay, and the file is written from the overlay and the graph together. [ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md) holds the decision, the options, and the invariants the overlay keeps.

- **What a level shows.** Its modules, its nested sub-patches as tiles with a name, ports, and the surface of their exposed parameters, its Patch inputs and outputs as port tiles, and the cables between them. A modulation line is drawn between two modules of the level, or from a module of the level to the surface control that stands for an inner parameter. *Why:* a line ends where the player can act on it.
- **The breadcrumb bar appears only while a sub-patch is open.** A flat patch looks as before. *Why:* a player who never collapses anything should see nothing new.
- **Each level keeps its camera for the session,** as [Make, open, and dissolve](#make-open-and-dissolve) says. The camera of a level that is collapsed into a new sub-patch moves with it. *Why:* going in and back out should land where the player left.
- **A new sub-patch is named "Sub-patch"** ("Sub-patch 2", ...), and its definition key is made from the name. The name is changed in the tile's inspector, as one undo step; the key stays, since it is not shown. *Why:* collapsing should not stop to ask a question, and the name is easy to change later.
- **Patch input and output modules at the top level count as outside when collapsing,** like the Transport (collapsing point 6). Their cables into the selection become ports. *Why:* at the top level they are the ports of a library file opened on its own (flattening rule 15), and moving one inside would take a port off that file.
- **Adding the Transport inside a sub-patch is refused** with `A sub-patch cannot hold the Transport: it lives at the top level`. *Why:* rule 8.
- **Adding a Patch input or output inside a sub-patch makes a port** on the instance's tile, named by the module's label. *Why:* inside a definition that is what the module is (flattening rule 2), and the tile should show it at once.
- **A modulation reaches inside a sub-patch only through an exposed parameter.** A drop on a surface control, an inspector drop or **Add modulation** on a surface row, and a matrix retarget onto one all assign the inner parameter from a source on the tile's level. An assignment onto an inner parameter that no tile of the source's level exposes, or from a port tile, is refused with `Modulation cannot cross a sub-patch boundary; expose the parameter instead`. The **Add modulation** list on a surface row offers the tile's level's modules. *Why:* rule 4.
- **The clipboard carries sub-patch tiles with their definitions** ([clipboard](clipboard.md#the-clipboard-text)). Copy, cut, paste, and duplicate of a selection with a sub-patch tile give the copy a definition of its own, as [Copies and editing](#copies-and-editing) says; a cable into the tile from a selected module, and an assignment onto its exposed name, come too. A port tile in the selection is left out, as the Transport is: the clipboard holds no ports. *Why:* a port belongs to its definition, and pasting one elsewhere would make a port the tile never asked for.
- **Deleting a module takes the exposed items on it with it,** in the same undo step, at every level that re-exposed them, as Unexpose would. Deleting a port takes the cables through it on both sides. *Why:* an exposed item with nothing behind it has no value to show, and a file that named it would not load.
- **A MIDI mapping on an inner parameter is written inside the definition.** A mapping on an instance's exposed name in a file flattens to the same thing; the inside form is the one the editor writes. *Why:* one canonical form, so a file saves back to the text it loaded from.
- **Assignment order is kept canonical.** `sync` puts the live assignments on each parameter in the order the file loads them (flattening rule 11), after every change to the hierarchy and after each assignment is added or retargeted. Only assignments that must move get new ids, and the matrix selection and an arc drag follow them. A collapse that moves one of three or more assignments on a parameter inside can change the order of the sum once, at the collapse (rule 2). A save and reload never does. *Why:* the engine sums from zero in id order, and with three or more terms the last bits depend on the order, so the live order and the file's must be one order ([flattening](../learn/flattening.md#why-the-order-of-assignments-matters)).

### Roles

**Settled** (OQ-20, from its leaning).

- A role has a namespaced ID and a version, like a module: `loom.role.filter@1`.
- A role declares input names, output names, and core parameter names, with their kinds. It is data in the SDK, like a module descriptor (`loom_core::RoleDescriptor`; the initial roles are `static`s in `loom_core::roles`). A port's kind is its signal type. A parameter's kind is its unit and whether it is a whole number. *Why:* a value carries across by name, so Hz must meet Hz, and a choice must not land on a knob. Minimum and maximum may differ: the value is clamped to the new part's range.
- **Each port and parameter is required or optional** (settled 2026-11-08). The registry refuses a declaration that lacks a required one. An optional one is checked only when the part has it. A slot shows every role port; one the current part lacks connects to nothing while that part is in, as a port with nothing behind it does (flattening rule 8). A value for an optional parameter carries across when both parts have it. *Why:* the modules that should fill a role do not all have every core parameter (no oscillator has `level`, the one-pole has no `resonance`), and a new version of a role only adds optional parameters.
- A new version may only add optional parameters. Every part that filled the old version still fits. A file naming a role version newer than the build knows is refused, as a module kind is; an older one resolves. A part fits a slot by role **id**, whatever version either declares. *Why:* an older part has everything an older slot asked for, and a newer slot asks for nothing more that is required.
- A module or a sub-patch can fill several roles.
- A module declares its roles in its descriptor (`ModuleDescriptor::roles`, a list of `FillsRole`). Each role port and parameter is matched to one of the module's own by name. A declaration can map a role name to a different own name, such as the role's `out` to a filter's `lowpass`, separately for inputs, outputs, and parameters. *Why:* the roles fit existing modules without renaming their ports.
- The registry refuses a declaration whose module lacks a role's required port or parameter, has it in another kind, maps a role name to a name the module lacks, or maps two role names onto one own name, and names what is missing (FR-SDK-08).
- A sub-patch declares roles in its definition (FR-ENCAP-09), as a plain list of role ids with no name map: a definition fits a role when it has a Patch input or output of each required port's name and an exposed item of each required parameter's name, whose inner parameter is of the role's kind. The player renames a port or an exposed item to fit. Its exposed parameters and ports are checked against each role when it is saved to the library, inserted, or placed in a slot. A failed check at save or insert removes that role from the definition with `Voice no longer fills Filter: no exposed "cutoff"`; it does not block the save. The gesture that declares a role is a **Fills role** row in the sub-patch tile's inspector, one check box per role; turning on one that does not fit is refused with the same wording.
- The initial roles are the ones in the [catalogue](../product/module-catalogue.md#roles). Sound source takes a `phase`, not a pitch, because every LoomDSP oscillator is phase driven (ADR-0012). Voice takes `pitch`, `gate`, and `velocity`, the per-voice signals of ADR-0038.

### Slots

The points marked "settled 2026-11-08" were put to the maintainer as a proposal while building slice 5 and agreed.

- A slot is a module at any level. It has the ports and core parameters of its role, and shows the part's panel on its tile.
- **Make slot** (settled 2026-11-08) is on the tile menu of a module or sub-patch tile that declares a role: one item per role, `Make Filter slot`. The module becomes the slot's current part in place, keeping its values, running state, position, MIDI mappings, and assignments. Each cable on a role port moves to the slot's port of the role's name, and an exposure of a role parameter follows it onto the slot. A cable on a port the role does not have (the SVF's `highpass`) is removed, and the status bar says `Filter slot made. The cable from Highpass was removed: a slot has only the Filter role's ports.` One undo step. *Why remove and not refuse:* the player chose the role knowing its ports, and undo brings the cable back. An empty slot comes from the module browser ([Empty slots](#empty-slots-agreed-2026-11-14)).
- **Dissolve slot** (settled 2026-11-08) is on a slot tile's menu: the part becomes a plain module in place with its cables on its own ports, and the held parts are forgotten. One undo step.
- **Swap** is a menu on the slot's tile, in the inspector, and on any panel widget bound to it. It lists every module type and library sub-patch that declares the role, the current part marked. On the tile menu the items read `Swap to Ladder`; the inspector has a **Part** list. A surface control bound to a slot's role parameter offers it through its tile's menu (`Swap Filter to Ladder`), since the canvas has no menu on a control (settled 2026-11-08). A library sub-patch is inserted as a copy and placed as the part.
- **Cables stay.** The slot's ports are the role's ports.
- **Values carry across** by role parameter name, clamped to the new part's range. A value the new part does not have stays stored, so swapping back restores it (FR-ENCAP-08). Assignments and MIDI mappings on role parameters move to the new part and keep their ids. One on a parameter only the leaving part has is removed with the swap, and the status bar says `Swapped Filter to Ladder. 1 modulation on Slope was removed.`; undo brings it back.
- **Data** (OQ-21, settled from its leaning): each part's data is kept in the patch, keyed by the part, and comes back when the player swaps back. Data is never converted between parts.
- **The swap is click-free.** Old and new parts both run for a 10 ms crossfade, then the old part is removed. The fade's law follows each output's signal type: equal power for audio, linear for pitch and modulation ([How a swap crossfades](#how-a-swap-crossfades-settled-2026-11-08), point 6). *Why:* a swap is a graph edit, and FR-GRAPH-05 asks for faded connection changes. How, without the engine learning anything new, is under [Flattening](#flattening), after the rules.
- **The swap is one undo step.** Undo and redo of a swap go through the swap again, so they fade too (settled 2026-11-08). *Why:* rule 6, and an undo on stage is a swap. The part that comes back is a new module at its stored values, as undo of a delete is (live-patching, out of scope).
- Parameters only the current part has show in a sub-panel that follows the slot (FR-PANEL-11), on the abstraction's front panel, which waits for the panel editor. On the slot's own tile the whole part panel shows. A panel widget bound to a role parameter keeps working after a swap: **Expose** on a role parameter of the part writes the exposure against the slot and the role name, so the surface control follows the slot across swaps. Expose on a parameter only the current part has is refused with `Expose a Filter parameter: Slope is only on Ladder` (settled 2026-11-08). *Why:* the surface item would have nothing behind it after a swap.
- **The slot tile** shows the slot's label (default: the role's name) in the header, the role's ports with the role's labels, and the current part's panel with live values. Hover text on the header and the ports adds the part's name. The inspector shows the name, the Part list, the role's parameter rows, a heading with the part's name over its other parameters, and Dissolve slot.
- A selection that includes a slot tile is refused by the clipboard with `Cannot copy a slot yet`; a sub-patch tile that contains a slot copies whole, since its definition's text carries it (settled 2026-11-08; a follow-up carries the slot itself).

#### Empty slots (Agreed 2026-11-14)

The maintainer agreed these on the plan for [#700](https://github.com/brylie/LoomDSP/issues/700). [ADR-0043](../adr/0043-unpatched-means-normalled.md) holds the rule behind them: unpatched means normalled.

- **From the module browser.** It lists one row per role that a part fills: a registered module, or a library item whose declared role resolves (an unknown or newer role kind adds no row). Today that is Sound source, Filter, Envelope, Modulator, and Effect; Shaper and Voice appear once something fills them. A row reads `Filter slot`, with the mark `Slot`; the area under the list shows the role's description and `Add an empty slot, then swap a part in from its menu or the inspector.`, with the role's kind as the detail ([module browser](module-browser.md)).
- **Adding one** puts a `loom.struct.slot@1` with no part at the pick point, labelled with the role's name, made unique as sub-patch names are (`Filter 2`), and selects it. One undo step; the status says `Added Filter slot. Swap a part in from its menu or the inspector.` It is refused at a layer or select member level, with that level's text.
- **The tile** shows the role's ports with the role's labels, and draws its note in the panel: `No part. Passes its input through. Swap to choose one.` for a thru role, and `No part. Swap to choose one.` for any other. The note is text, not a control: Tab skips it, and the tile's description, which a screen reader reads, says the same sentence once. The header's hover text is `Slot for Filter. No part. …`. A filled slot's hover text says `Slot for Filter. Holds Ladder. Swap from its menu.`
- **What an empty slot plays.** It is a pulled cable: its outputs carry nothing, so each input it feeds reads its normal. **An empty Filter or Effect slot is a thru:** its `out` carries whatever feeds its `in`. The other role inputs of a thru, such as Filter's `cutoff`, feed nothing. For example, an empty Sound source slot into Audio Out is silent; an empty Envelope slot into an Amplifier's `level` plays the Amplifier's input at full level, as pulling that cable would; and an empty Effect slot in a chain lets the chain play on. The thru applies to cables only: a modulation assignment from an empty slot moves nothing, thru or not. *Why:* ADR-0043. A role is a thru role when its descriptor says so (`RoleDescriptor::thru`); the registry refuses it on a role without a required audio `in` and `out`.
- **Port badges, widths, and probes** on an empty thru slot's `out` follow what feeds its `in`, so a scope there shows the sound that plays.
- **Remove part** is on a filled slot's tile menu, after the `Swap to …` items and before `Dissolve slot`, and the inspector's Part list starts with `No part`. It empties the slot: the leaving part goes into `held`, so swapping it back restores its values. Assignments and MIDI mappings on role parameters, and assignments from the slot's outputs, have nowhere to go: they are removed and the status names them, as a swap does: `Removed Ladder from Filter. 1 modulation on Cutoff was removed.` One undo step; undo brings them back with their ids. From the tile menu, focus returns to the tile; from the inspector, it stays on the Part list.
- **An empty slot's menu** lists `Swap to …` for each part and has no `Dissolve slot` or `Remove part`; the inspector leaves out Dissolve slot too. Delete removes the slot. Choosing the part the slot holds, or no part for an empty slot, is not offered as a swap.
- **A surface control bound to an empty slot's role parameter** stays in place, drawn dimmed. It shows no value, moves nothing, is not a Tab stop, and its hover text and description say `Filter has no part.`
- **Swaps into and out of an empty slot.** For a **thru role**, the swap fade softens it, with the **linear** law (the dry signal and the signal through the part are nearly the same, so equal power would swell by up to 3 dB): into the slot, the fade's `a` takes the thru source and `b` the new part; out of it (Remove part, or undo of a swap-in), `a` takes the leaving part and `b` the thru source. The fade takes over only the inputs the slot's `out` fed, so other consumers of the thru source are untouched; with nothing feeding `in`, the leaving part fades to 0. For **any other role**, the inputs the slot feeds change between their normal and the part's output on the swap frame, as a hand-plugged cable does, until live-patching rule 9 is built ([#21](https://github.com/brylie/LoomDSP/issues/21)).
- **The values a part takes** when swapped into an empty slot: the slot's stored surface values for its role parameters, then the part's held values, then its defaults.

### Modes

A sub-patch's mode is a setting in its inspector. The default is **single**. The points below marked "settled 2026-11-09" were put to the maintainer as a proposal while building slice 6 and agreed. [ADR-0040](../adr/0040-stack-copies-and-mode-modules-are-implied.md) holds the decision on how copies and the modules a mode adds fit the editor's overlay.

| Mode | Inside | Ports |
|---|---|---|
| Single | One patch, as above. | Its Patch inputs and outputs. |
| Layer | Several **members**. Each member is a sub-patch. | Each outer input feeds the same-named input of every member. Each outer output is the sum of the same-named output of every member. A member without that port is skipped. |
| Select | Several members, as in layer. A **Member** parameter (stepped, modulatable) chooses one. | Each outer output comes from the chosen member. |
| Stack | One patch, run as N **copies**. | Each outer input feeds every copy. Each outer audio output is the sum of the copies, scaled by 1/√N; a pitch or modulation output is their mean, a gate their sum. |

- **Members** are added, removed, and reordered in the inspector. A member is opened like any sub-patch.
- **Select switches click-free.** Every member runs. A change of the Member parameter crossfades the outputs over 10 ms, equal power. *Why every member runs:* a member that was paused would restart cold, and skipping members per block would make the output depend on the block size (AGENTS.md rule 2). See [Open points](#open-points) for CPU.
- **Stack** copies come from flattening, so each copy is a full set of ordinary modules. **Copies** is a whole-number setting from 1 to 16. It changes the graph, so it is not modulatable.
- **Spread** (TR-04): in stack mode, the player picks exposed parameters to spread and a spread amount for each, from 0 to 1, a share of the knob as a modulation depth is (settled 2026-11-09). Copy `i` of `N` gets the parameter's value moved by the amount times an offset evenly spaced from -1 to 1 (copy 0 at -1, the last at +1; one copy has offset 0), in the control's 0 to 1 range, and clamped there, as a modulation assignment is ([modulation](modulation.md), rules 2 and 5). The flattener adds it as hidden modulation: a constant source of 1.0 and, on each copy's parameter, an assignment of depth `amount × offset`. *Why a share of the knob:* the same amount gives a musical interval on a cutoff and a step on a whole-number parameter, like a depth. *Why modulation:* the base value stays the same in every copy, so MIDI and outside modulation, which act on the base, keep the offsets. A value written into each copy would be wiped by the first MIDI control change, which the engine applies to each copy's parameter.
- **Stereo spread:** an option in stack mode. With it on, each audio output becomes stereo (ADR-0020): copy `i` is panned by the same offset, equal power. *Why:* unison is stereo more often than not.
- Copies and members share the definition's data ([module data](module-data.md)).
- Stack mode is not polyphony. Voices, voice allocation, and per-voice notes are in the [polyphony](polyphony.md) specification (FR-VOICE-*). A stacked sub-patch fed a bundle runs copies times voices ([polyphony](polyphony.md#the-voice-count), rule 13).

#### Members are nested instances (settled 2026-11-09)

- In layer and select mode, a definition's own `modules` hold only its members. Each member is an ordinary sub-patch instance with its own definition, and `members` lists their module ids in order. A member's name is its definition's name.
- *Why:* a member is then a nested sub-patch in every way that exists: flat ids by path, opening, the breadcrumb, renaming, exposing its surface items on the layer's surface (flattening rule 12), slots inside it, Save to library on it, and its own link to a library file.
- A file whose layer or select level holds anything else (another module, a cable, an assignment, a mapping, a Patch input or output), lists a member twice, leaves one out, or lists a module that is not an instance, is refused, naming the sub-patch.
- At most **16 members**, as for copies and voices ([polyphony](polyphony.md), rule 1): the modules the flattener adds have a fixed number of inputs.
- **Ports** of a layer or select instance are the union of its members' ports by name, in the order they first appear: member order, then each member's own port order. An outer input feeds the same-named Patch input of every member that has it. A member without an output adds nothing to that output's sum, or gives silence there when it is chosen.

#### Copies in the editor (settled 2026-11-09)

- **The editor shows copy 0.** Tiles, values, the inspector, probes, lights, meters, and live values inside a stack are copy 0's. The breadcrumb names the stack with its count: `Voice ×4`. The outer tile's outputs carry the sum. *Why copy 0:* its knob shows the base value, which is the same in every copy (the spread is modulation), and its signals are a copy that plays, not an average no module plays.
- **Paths.** Copy 0 keeps the paths a single sub-patch has: `[instance, inner]`. Copy `k` (from 1) is `[instance, Copy(k), inner]`, where `Copy(k)` is a path step that no module id can take. Turning a sub-patch into a stack, or raising Copies, never moves copy 0, and copy `k` keeps its id and state while Copies stays above `k`. A removed copy's ids are not reused (rule 4).
- **An edit inside reaches every copy, as one undo step.** A parameter write, from a control, an inspector row, a reset, or the control channel, writes the same value to every copy at once, so a knob drag moves every copy. Everything else (a module, a cable, an assignment, a mapping, an exposure, Copies, a spread) reaches the copies through `sync` before the commit, from the flattener's output. MIDI and modulation inside reach every copy by themselves, since each copy has its own mapping and assignment.
- **Seeds differ.** Copy `k` of a module with a `seed` parameter gets copy 0's seed plus `k`, wrapped in the parameter's range. *Why:* copies that play the same random numbers are one sound in disguise ([stereo](stereo.md), rule 15).
- **Undo snapshots** leave copies 1 to N-1 and every added module out, and `sync` rebuilds them after a restore. A select's Member module is the exception: its value, and an assignment or mapping on it, belong to the player and are saved on the instance, so a snapshot keeps them and undo restores a Member change like any parameter's (settled in review, 2026-11-09).

#### The modules the flattener adds (settled 2026-11-09)

All are registered as internal: not in the browser or the help pages, checked by every test that runs over all modules. They have no panel, hold no buffer, allocate nothing on the audio thread, count per sample, and give the same output at any block size. Each has a flat id from a path `[instance, Added(n)]`, where `Added(n)` is a path step no module id can take, numbered by the outer port's place, so ids are deterministic (flattening rule 6). They are never shown or saved.

| Module | Inputs | Output | Parameters | For |
|---|---|---|---|---|
| `loom.struct.sum@1` (Sum) | `in1` to `in16` | `out` | `gain` | each output of a layer (gain 1) and of a mono stack (gain 1/√N) |
| `loom.struct.spread_sum@1` (Stereo spread sum) | `in1` to `in16` | `out`, stereo | `copies` | each output of a stack with stereo spread |
| `loom.struct.member@1` (Member) | | `out` | `member`, 1 to 16, whole number | a select instance's Member control |
| `loom.struct.select@1` (Select) | `in1` to `in16`, `member` | `out` | `members`, `fade` (Off, Linear, Equal power) | each output of a select instance |

- **Sum.** `out = gain × (sum of the inputs)`, the gain smoothed over 10 ms, so a change of Copies does not step the level. A change of Copies is a cable change ([live patching](live-patching.md), rule 9). Going from 2 copies to 3 plugs a cable into the Sum, so the new copy fades in over 5 ms, and a removed copy fades out the same way. Going from 1 copy to 2 adds the Sum, which starts at its final gain, and going from 2 to 1 removes it, so whatever reads the stack fades once, over 5 ms, to its new output: no step. A layer's gain is 1. A stack's gain follows the signal type of what feeds the output (settled with the maintainer in review, 2026-11-09):
  - **Audio:** 1/√N. *Why:* copies that differ are uncorrelated, and the power of a sum of uncorrelated signals is the sum of their powers, so 1/√N keeps the loudness of one copy. Identical copies sum coherently and come out √N times louder; [Unison](../learn/unison.md) says why the spread matters.
  - **Pitch and modulation:** 1/N, so the output is the copies' mean. *Why:* these are values, not sounds; four copies of pitch 0.2 must give 0.2, not 0.4.
  - **Gate and trigger:** 1, a plain sum, so a gate stays high while any copy is high.
  - **Phase:** no Sum: copy 0's phase goes out alone. *Why:* a sum of ramps that wrap at different moments runs from 0 to N and jumps at each wrap, which no phase input can read.
  - A pass-through or an output with nothing behind it counts as audio.
- **Stereo spread sum.** Only an audio output takes stereo spread; pitch, modulation, gate, and phase outputs stay mono, with the Sum above, since a pitch has no side. Copy `k` is panned by its offset `o`: `θ = (o + 1)·π/4`, left `cos θ`, right `sin θ`, both times 1/√N. Equal power, so a copy keeps its loudness wherever it sits, and the centre is -3 dB on each side, the constant-power pan law. One copy with stereo spread sits in the centre. The inputs are `Downmix` and the output `Stereo` (ADR-0020), so a stereo copy is folded to mono before it is panned, and the outer output is stereo.
- **Member** owns the Member parameter, the one home of its value (ADR-0039), and outputs it per sample. Every Select of the instance reads it on its `member` input, so all outputs switch on the same frame.
- **Select.** Per sample it takes `t = round(member)`, clamped to 1 to the member count, and runs one state machine:
  1. **Not fading, and `t` is not the current member:** a fade starts from the current member to `t`: `out = from·cos θ + to·sin θ`, `θ = (π/2)·n/N`, `N = round(0.010 × sample rate)`, one step per sample. After `N` samples it outputs exactly `to`.
  2. **`t` goes back to `from` during a fade:** the fade reverses from where it is (`from` and `to` swap, `n` becomes `N - n`). The gains on that frame are the same, since `cos((π/2)(N-n)/N) = sin((π/2)n/N)`, so nothing steps.
  3. **`t` goes to a third member during a fade:** it waits. When the fade ends, a new one starts from there to the latest `t`. Only the latest is kept, so a fast sweep ends where the control stopped, never more than 10 ms late. *Why not finish at once, as a second slot swap does:* a swap is a gesture, but Member can be modulated at audio rate, and a jump would click each time.
  4. **A modulated Member between two whole values** takes the nearest one, as every whole-number parameter does ([modulation](modulation.md), rule 5).
  5. **The law follows the signal type** of what feeds the members' Patch outputs (settled with the maintainer in review, 2026-11-09):
     - **Audio:** equal power, `from·cos θ + to·sin θ`, as above.
     - **Pitch and modulation:** linear, `from·(1 - p) + to·p` with `p = n/N`: the gains sum to 1, so a value both members share stays put. *Why:* equal power would lift a shared pitch of 0.2 to 0.283 in the middle of the fade, about 10 semitones for 5 ms; a constant-sum fade is the usual practice for control voltages. Turning back stays continuous: the gains at `N - n` are the gains at `n` swapped, under either law.
     - **Gate, trigger, and phase:** `fade` off. They switch to `t` on the frame it changes, with no fade and no wait, for the reason live-patching rule 9 gives.
     - Members of mixed kinds take the safest: off if any is a gate or phase, else linear if any is pitch or modulation.
- **Spread sources** are `loom.math.constant` modules at 1.0, one per stack instance with a spread, also at `[instance, Added(n)]`, with hidden assignments (Spread, above). They are left out of the matrix, the inspector's modulation rows, the range arc, and the file.

#### The editor for modes (settled 2026-11-09)

- **Mode** is a choice row in the sub-patch tile's inspector: Single, Layer, Select, Stack. Changing it is one undo step and **never changes the sound by itself**:
  - **Single to layer or select:** the inside becomes member 1, named after the sub-patch. Its modules keep their flat ids and state (their paths move under the new member, as a collapse moves paths), its Patch inputs and outputs keep their names, and each exposed item is exposed again through the member, so the surface keeps its names and values. Select starts at Member 1.
  - **Single to stack:** Copies starts at 1, so nothing changes until the player raises it.
  - **Layer to select, and back:** the members stay.
  - **Layer or select to single or stack:** only with one member, whose inside moves back up. With more, it is refused with `Remove members until one is left to make Voice single` (`stack` in place of `single`). When the one member has a mode of its own, it is refused with `Make Saw single before making Pad single`, since its inside would lose that mode (settled with the maintainer in review, 2026-11-09).
  - **To select:** a surface item already called `member` takes the next free name, `member 2`, with its value, its outer exposures, and what targets it, so the Member control can have the name.
  - **Stack to single or another mode:** copies 1 to N-1 go, copy 0 keeps its state, and Copies, spread, and stereo spread are cleared.
- **Members** (layer and select): the inspector lists them by name, each row with **Open**, **Up**, **Down**, and **Remove**, and **Add member** under them, which adds an independent copy of the last member, named as a duplicate is ("Saw 2"). Removing the last member, from the row or with Delete or Cut on its tile, is refused with `A layer needs at least one member` (`select` for a select). A 17th is refused with `A layer holds at most 16 members`. **Member follows the member that plays** (settled with the maintainer in review, 2026-11-09): reordering keeps it playing at its new place, and removing an earlier member keeps it too; when the member that plays is removed, the one now at its place plays. Member's new value is part of the same undo step. *Why:* moving rows in the inspector must never switch the sound. Renaming is the member tile's own inspector, as for any sub-patch. Each is one undo step.
- **Inside a layer or select.** Opening it shows its members as tiles in a row, with no cables and no port tiles; the breadcrumb reads `Patch › Layer › Saw`, and opening a member shows its inside. At the member level, inserting a library item or pasting sub-patch tiles adds members, and deleting a member tile removes it. Any other module, a cable, a port, or a collapse is refused with `A layer holds only its members; open a member to add modules` (`select` for a select). *Why:* the level has no ports of its own; its ports come from its members.
- **Stack.** The inspector shows **Copies** (1 to 16, one undo step per change), **Stereo spread** (a toggle), and **Spread**: one row per exposed item with an amount from 0 to 1. A row at 0 is not spread and is left out of the file. *Why no separate pick list:* the amount says it, and an exposed item is what the player chose to show. Raising Copies keeps the state of existing copies; new copies start from reset at copy 0's values ([live patching](live-patching.md), rule 4). Opening a stack shows copy 0's inside.
- **Member control.** A select instance's surface shows a stepped control labelled `Member` first, spanning 1 to the member count, so MIDI sweeps exactly the members. The inspector row names the chosen member: `Member: 2 (Square)`. Removing members clamps the value to the new count.
- **Dissolve** of a layer, select, or stack instance is refused with `Make Voice single before dissolving it`, and so is dissolving a member inside its layer or select, with `A layer holds only its members; open a member to add modules`.

#### How modes meet the rest (settled 2026-11-09)

Each refusal is a follow-up, not a gap.

- **Linked library items.** Changing mode, members, Copies, a spread, or stereo spread is an edit inside, so the first one unlinks a linked tile, as any edit inside does. **Save to library** of a sub-patch that is not single is refused with `Cannot save a layer to the library yet` (`select`, `stack`): a patch file has no place for a mode yet. A member's own Save to library works, since a member is a single sub-patch.
- **Roles.** A sub-patch that is not single fills no role: turning on Fills role on one is refused with `A layer cannot fill a role yet`, and changing the mode of one that fills a role is refused with `Voice fills Filter; turn that off before changing its mode`. A slot's part always fills its role, so this covers slots' parts too.
- **Slots** (changed 2026-11-14, [#700](https://github.com/brylie/LoomDSP/issues/700)). A sub-patch that holds a slot may be a stack: Copies above 1 is allowed, Make slot and an empty slot from the browser work inside a stack, and Single ↔ Stack is allowed, since copy 0 keeps its paths and no slot moves. **A swap between two parts inside a stack fades in every copy:** the swap adds each copy's fades and keeps each copy's leaving part, all named by the slot's fade record, and all start and finish on the same frame ([ADR-0044](../adr/0044-swap-fades-are-transient-at-every-copy.md)). Finishing reads copy 0's fades. A change of Copies, spread, stereo spread, or mode during a fade finishes every fade first. A swap that would pass the module limit in any copy is refused whole, adding nothing. **A swap into or out of an empty slot inside a stack** steps in every copy, copy 0 included, until [#768](https://github.com/brylie/LoomDSP/issues/768) gives each copy its own thru wiring. Changing to layer or select is still refused, with `Cannot make Voice a layer: it holds a slot` (`select` for a select), since the inside would move under a member. A slot inside a member works, since a member is a nested sub-patch.
- **Clipboard.** A sub-patch tile copies whole, mode and members included. A copy inside a stack copies copy 0's modules. At a member level, a paste of sub-patch tiles adds members, and a paste with anything else is refused with the text above.
- **Exposed parameters through members.** The layer's surface exposes a member's surface item with the existing Expose button. One surface control for the same parameter in every member is not built.
- **Nesting.** A stack inside a stack runs copies times copies, and a layer inside a member works.

### Patch file

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Voices"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.osc.saw@1"),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [/* ... */],
            exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff", range: Some((200.0, 2000.0)))],
            roles: ["loom.role.voice@1"],
            // panel, mode, members, copies, spread, stereo_spread: present only when set
        ),
        "delay": SubPatch(library: Some("Delays/Tape delay.ron"), /* stored copy */),
    },
    modules: [
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 1200.0}),
    ],
)
```

- A definition's `metadata` is the same type as the patch's (name, description, authors, tags, licence), so a library item's credit travels with it ([#652](https://github.com/brylie/LoomDSP/issues/652)). Its name is what the player sees.
- A patch file has an `exposed` list of its own, omitted when empty: the surface it shows when it is inserted as a sub-patch ([Library](#library)). Save to library writes the definition's list there, and inserting the file reads it back. Opened on its own, the list is kept and saved back, less an entry whose module is gone, and nothing shows it; an entry follows its module through a collapse (onto the new tile's surface) and a dissolve. *Why:* a library item is an ordinary patch file, and its surface has to live somewhere in it.
- A linked definition's `library` is its path relative to the library folder, with `/` between the parts. The definition itself is the stored copy.

A sub-patch with a mode (settled 2026-11-09) carries it in its definition. Every field is omitted at its default:

```ron
"layer": SubPatch(
    metadata: Metadata(name: "Layer"),
    mode: Select,                 // Single (omitted), Layer, Select, or Stack
    members: [2, 1],              // layer and select: member instance ids, in order
    modules: [
        PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("saw")),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("square")),
    ],
),
"unison": SubPatch(
    metadata: Metadata(name: "Unison"),
    mode: Stack,
    copies: 4,                    // 1 to 16, omitted at 1
    spread: {"detune": 0.2},      // exposed name to amount, 0 to 1; omitted when empty
    stereo_spread: true,          // omitted when false
    modules: [/* the one inside, as for single */],
),
```

- A select instance holds its Member value in `params` under the reserved name `member`, 1-based: `params: {"member": 2.0}`. An exposed item cannot take that name on a select surface; the next free name is used ("member 2"). An assignment or MIDI mapping on Member is written on the instance, the form flattening rule 3 reads.
- A `members`, `copies`, `spread`, or `stereo_spread` that does not fit the mode, a spread name that is not exposed, or an amount outside 0 to 1, is refused, naming the sub-patch.

A slot (settled 2026-11-08) is a module of kind `loom.struct.slot@1` with a `slot` field, as an instance has a `subpatch` field. Its current part is an ordinary module of the same level, named by id:

```ron
modules: [
    PatchModule(id: 3, kind: "loom.filter.ladder@1", params: {"cutoff": 800.0, "drive": 0.3}),
    PatchModule(id: 9, kind: "loom.struct.slot@1", label: Some("Filter"), slot: Some(Slot(
        role: "loom.role.filter@1",
        part: Some(3),
        held: {
            Module("loom.filter.svf@1"): Held(params: {"cutoff": 800.0, "resonance": 0.4}),
            SubPatch("comb"): Held(params: {"cutoff": 800.0}),
        },
    ))),
],
cables: [
    PatchCable(from: (2, "out"), to: (9, "in")),
    PatchCable(from: (9, "out"), to: (4, "in")),
],
```

- `role` is the role's `id@version`. `part` is the level id of the module or instance that fills the slot now, or `None` for an empty slot, which shows the role's ports and its note and plays as [Empty slots](#empty-slots-agreed-2026-11-14) says: a pulled cable, or a thru for a Filter or Effect slot. The part is never cabled directly: every cable, assignment, mapping, and exposure on it goes through the slot, by the role's names.
- `held` keeps what every other part the slot has held would need to come back: its parameter values and control ranges by its own names, and later its data. The key says what the part is: a module kind, or a definition key. A definition a slot holds but does not play stays in `subpatches`, used by no instance, so a sub-patch part comes back whole, inner edits included. Swapping back takes the entry out of `held` and makes a module from it. The size is bounded by the number of distinct parts.
- The slot's own `params`, `ranges`, and `subpatch` are empty; `label` is the tile's name. A file that cables the part directly, names a `part` the level does not have, names a part that does not declare the role, or holds a `held` key whose definition is missing, is refused with a message naming the slot.
- *Why the part is a plain module:* module ids, flat ids, undo, module data, MIDI, and modulation all work on it unchanged. The slot adds only the overlay: the role, which module is behind the tile, and the held values. A new part is a new module with a new id, so flat ids stay stable (flattening rule 4).

- Module IDs are unique within one definition, not across the patch. *Why:* a definition copied into another patch keeps its own IDs.
- Definition keys are made from the name and made unique ("voice", "voice 2"). They are not shown to the player. Each instance refers to its own key; no two instances share one.
- An instance's `params` holds the values of its exposed parameters, by exposed name.
- A definition that contains itself within one file is refused when loading or inserting, with a message naming the loop. A loop through library links (A links B, B links A) leaves the sub-patch where the chain comes back its stored copy, with a note naming the loop, so the patch still opens (rule 5). Inserting, into a library file opened on its own, a link to that file or to a file whose links reach it is refused with the loop message, since it would make the file contain itself. Settled with the maintainer on 2026-11-08.

### Flattening

These rules are for implementers. The player never sees the flat graph.

1. The control side expands each instance into its definition's modules, recursively, before every compile.
2. A Patch input or output is removed. A cable into it continues from its other side.
3. An exposed parameter's instance value is written to the inner parameter. A modulation assignment or MIDI mapping on the exposed parameter is attached to the inner parameter.
4. Each flat module gets a `ModuleId` from a table keyed by its path (instance IDs, then the inner ID). A path keeps its ID across edits, so its state survives. A removed path's ID is not reused in the session.
5. Stack copies, layer and select members, and slot parts expand into ordinary modules, with the mixing modules the flattener adds for modes ([The modules the flattener adds](#the-modules-the-flattener-adds-settled-2026-11-09)). Added modules are not shown and not saved. Stack copies 1 to N-1 and the added modules are implied: the editor's `sync` rebuilds them from the flattener's output (ADR-0040). A slot's part is already an ordinary module of its level: the flattener routes each cable, assignment, mapping, and exposure on the slot to the part's mapped port or parameter and adds nothing. The crossfade modules of a swap are added by the swap, not the flattener, and are never in the file (corrected 2026-11-08: a fade is a moment in a session, not a fact about the patch).
6. Flattening is deterministic: the same patch gives the same flat graph and the same IDs.
7. The flattener never opens a file. Linked library items are read through a `Library` trait that the caller implements, before a patch is flattened: when it loads, every linked definition takes a fresh copy from the library (`Patch::refresh_linked`), and the flattener then sees only definitions. The desktop app reads the library folder, a test passes a map, and the command line and the web runtime pass `NoLibrary`, so a missing item falls back to the stored copy (rule 5). A file's own linked items are refreshed first, so a chain of links is followed to its end, and a chain that comes back to a file on the way is the loop the [patch file](#patch-file) refuses. *Why:* `loom-patch` must work on WebAssembly, which has no files (hard rule 6), and the loop check needs to read links while it follows them.
8. **Ports with nothing behind them.** A cable or assignment from an instance output whose Patch output has no cable behind it is dropped; the outer input reads its default. A cable into an input port that feeds nothing inside is dropped too.
9. **Pass-through ports.** A Patch output fed straight from a Patch input is a pass-through: the outer cable continues from whatever feeds the instance's input, through any number of levels. A pass-through fed by its own output carries nothing and is dropped.
10. **An assignment never starts at a port.** A modulation assignment inside a definition whose source is a Patch input's `out` is refused. Collapsing never makes one (point 4 turns an outside-to-inside assignment into an exposed parameter).
11. **Assignment order.** Each instance's inner assignments are added before the level's own, instances in id order, depth first, then the level's own in file order. The engine sums assignments on one parameter in that order. The editor keeps its live order equal to this one ([Editor model](#editor-model)), so a patch plays the same after save and reload. A collapse may reorder once (rule 2).
12. **Exposed through levels.** `Exposed.module` may name a nested instance; `param` is then one of that instance's exposed names. The value is clamped to each level's range, outer first. An exposed value is written into the inner module before it is resolved, and a saved control range on that parameter is dropped, so the loader picks a range that holds the value (control-ranges.md, rule 7).
13. **Port names are not made unique by the flattener.** Two Patch inputs or two Patch outputs of one definition with one name are an error. The editor makes names unique when collapsing (point 3). A Patch input or output with no label is called `in` or `out`.
14. **An instance's `label`, `position`, and `size` are the editor's.** The flattener ignores them. An instance's `ranges` is refused, as any parameter the definition does not expose is.
15. **At the top level, Patch input and output are ordinary modules**, as when a library file is opened on its own: the input outputs silence, the output reads its input and does nothing.
16. **No two instances share a key.** A file whose instances name one definition twice, at one level or at two, is refused, naming the key and both instances (`modules 1 and 2 › 5`). *Why:* rule 3. The editor never writes one; a merge or a hand edit can.
17. **Two cables into one input inside a definition are refused.** A flat patch keeps the later cable; flattening cannot keep that order across a boundary, so the file says which it means. The editor never makes two.
18. **Errors and notes from inside a definition carry its name** and the inner module's own id, such as ``in sub-patch "Voice": module #3 has no input port `inn` ``. Unknown modules at every level are reported in one note.

#### How a swap crossfades (settled 2026-11-08)

The crossfade is made of ordinary modules and ordinary graph edits, so the engine learns nothing new and the audio thread allocates nothing.

1. **A hidden module, `loom.struct.fade@1`** ("Swap fade"): inputs `a` (the leaving part) and `b` (the new part), output `out`, one parameter `law` (Equal power, the default, or Linear; a choice the swap sets, not modulatable), one display value `done`. From its first processed frame it counts samples to `N = round(0.010 × sample rate)` and outputs `a·cos(θ) + b·sin(θ)` with `θ = (π/2)·n/N` under Equal power, or `a·(1 − p) + b·p` with `p = n/N` under Linear. After `N` samples it outputs exactly `b` and publishes `done = 1`. It counts per sample, so it is block-size invariant and deterministic, and it holds no buffer. It is registered as internal: not in the browser or the help pages, but checked by every test that runs over all modules.
2. **The swap**, on the control thread, as one undo step: the new part module is added with its values (role parameters from the leaving part by role name, clamped; its other parameters from its held entry, else defaults); one fade module per role output is added; assignments and mappings on role parameters move to the new part; the leaving part and its fades are recorded on the slot as **fading**. `sync` then wires each slot input to both parts' mapped inputs, each part's mapped output to its fade's `a` or `b`, and each fade's `out` to what the slot's output fed. A modulation assignment whose source is the slot's output goes through the fade's `out` too, so a Modulator or Envelope slot swaps click-free as a source; finishing points it at the new part. One commit, one schedule swap at a buffer boundary. The new part starts from reset at its values (live-patching rule 7) and the fade starts at 0, so the first frame after the swap is the leaving part alone.
3. **Finishing.** On each UI tick the app reads every fading slot's `done`. When all of a slot's fades read 1, the leaving part and its fades are removed and the graph is committed again. This is bookkeeping, not an edit: no undo step and no change to the saved file. The removal is click-free, because a finished fade outputs exactly `b`, which is what the new part's output then feeds directly. *Why a display value and not a clock:* the control thread cannot read how many frames the engine has processed, and wall-clock time is wrong when the engine is idle or a test steps it by hand; a display value is read on every tick and tells the truth in both.
4. **The file never holds a fade.** A save during the fade writes the new part only.
5. **A swap during a fade** finishes the first fade at once and starts the new one. What the first fade fed moves off its unfinished blend, so it fades over 5 ms to the part the fade was heading for ([live patching](live-patching.md), rule 9), which softens the step the fade had left. *Why:* two fades at once would need fades of fades, and a second swap within 10 ms is a slip of the hand.
6. **The law follows the signal type** of each role output (changed 2026-11-14, [#698](https://github.com/brylie/LoomDSP/issues/698)), as the select fade's does:
   - **Audio:** equal power, since two parts are different sounds.
   - **Pitch and modulation:** linear, so a value both parts share stays put. *Why:* equal power would lift a shared level of 0.5 to 0.707 in the middle of the fade, and a shared pitch would go sharp for a few ms.
   - **Gate, trigger, and phase:** not faded. They switch to the new part on the swap frame, for the reason live-patching rule 9 gives (a ramped gate fires late, and a ramped phase looks like wraps). A trigger is a one-sample gate, so the rule reads the port's signal type: gate or phase. No initial role has one.
7. **A swap into or out of an empty thru slot** uses the same fade with the thru source in place of the missing part, and the linear law ([Empty slots](#empty-slots-agreed-2026-11-14)).

*Why equal power for audio:* the two parts are different sounds, so they add in power, not in amplitude. Gains on a quarter circle keep the sum of their squares at 1, so loudness holds through the fade, where a linear fade dips by 3 dB in the middle (C. Roads, *The Computer Music Tutorial*, MIT Press, 1996, on crossfading; U. Zölzer (ed.), *DAFX: Digital Audio Effects*, 2nd ed., Wiley, 2011, on constant-power laws). *Why 10 ms:* above the few milliseconds where a gain change is heard as a click, below the tens of milliseconds where a change is heard as a fade. The explainer [Abstraction: roles, slots, and swapping](../learn/abstraction.md) has the sample values.

## Rules

1. **Encapsulation has no run-time cost.** The engine runs only flat graphs. *Why:* FR-ENCAP-07. The player should not trade clarity for CPU.
2. **Collapsing and dissolving never change the sound.** *Why:* organising a patch is not editing it. One exception, settled with the maintainer on 2026-11-07: when a collapse moves some of three or more assignments on one parameter inside, the engine sums them in the file's order from then on (flattening rule 11). The result can differ in its last bits once, at the collapse. It never changes on save and reload. *Why accepted:* the only way to keep both bit for bit is to refuse the collapse, which blocks a real use for a difference nobody can hear.
3. **No action at a distance.** Every tile has its own definition, and no edit, save, or paste changes another tile during a session. A link only refreshes a tile's own copy when the patch loads. *Why:* settled with the maintainer. Each sub-patch is a distinct part of the project in the player's mind, and propagation would make them track every copy before any local edit.
4. **Nothing crosses a level except through ports and exposed items.** Assignments, cables, and mappings stay inside their level. *Why:* a sub-patch must work the same wherever it is used (modulation rule 10).
5. **A patch always opens.** Linked items carry a stored copy. *Why:* a missing library file on another computer must not lose the set.
6. **Swaps and select changes are click-free.** *Why:* they happen during performance.
7. **One edit is one undo step, at any level.** *Why:* the [undo](undo.md) rules apply unchanged.
8. **Patch-wide modules live only at the top level.** A sub-patch never holds the Transport or the Scale and tuning module, so stack copies, layer and select members, slot parts, and linked items refreshed on load never repeat them. Inside a sub-patch, adding one is refused, and a paste leaves them out: their cables are dropped, since a cable cannot reach the top level (rule 4), and the status bar says so. *Why:* settled with the maintainer. A patch has one transport and one scale ([clock](clock.md), [scale and tuning](scale-and-tuning.md)), and one place to set them. The same reason keeps a paste from adding a transport ([clipboard](clipboard.md), rule 6).

## Acceptance criteria

Collapse and navigation:

- [x] Collapsing three modules with two cables in and one out gives a tile with two inputs and one output, and the rendered output is bit-identical before and after (tests: `crates/loom-patch/src/hierarchy/tests.rs`, `crates/loom-modules/tests/subpatches.rs`).
- [x] Dissolving that tile gives back the same modules and cables (test: `crates/loom-patch/src/hierarchy/tests.rs`, `crates/loom-app/src/levels/tests.rs`).
- [x] An outside assignment to an inner parameter becomes an assignment to the exposed parameter (test: `crates/loom-patch/src/hierarchy/tests.rs`).
- [x] Opening a sub-patch shows its inside and the breadcrumb; the breadcrumb and Escape go back; each level keeps its camera (`Ui` harness: `crates/loom-app/src/levels/tests.rs`).
- [x] Undo of an edit made inside a sub-patch, while the canvas shows the top level, moves the canvas inside and reverts it (test: `crates/loom-app/src/levels/tests.rs`).

Exposing:

- [x] Expose from the inspector gives the surface item the parameter's label, stores the inner value for the file, and the saved file loads with it; Unexpose on one surface leaves the exposures below it (tests: `crates/loom-patch/src/hierarchy/tests.rs`, `exposing_and_unexposing_a_parameter_round_trip_through_the_file`, `unexposing_at_one_level_leaves_the_exposures_below`; `crates/loom-app/src/levels/tests.rs`, `exposing_a_fader_puts_a_fader_on_the_surface`, `the_inspector_offers_expose_and_shows_surface_rows`).
- [x] A modulation from a level reaches an inner parameter only through a surface control of that level, from a drop, the inspector, or a matrix retarget; onto an unexposed parameter, or from a port tile, it is refused (test: `crates/loom-app/src/levels/tests.rs`, `a_modulation_reaches_inside_only_through_a_surface`).
- [x] Unexpose removes the assignments that no surface reaches any more in the same step, keeps one through a surface below, and undo brings the exposure and the assignments back (tests: `crates/loom-app/src/levels/tests.rs`, `unexposing_removes_the_assignment_from_outside_in_one_step`, `unexposing_on_an_outer_surface_keeps_assignments_still_exposed_below`).
- [x] An exposed range of 200 to 2000 Hz narrows the surface control and clamps a value typed on the surface, typed on the inner knob, dragged inside, set from an inspector row, or reset, and the file saves what played (test: `crates/loom-app/src/levels/tests.rs`, `an_exposed_range_clamps_writes_from_both_sides`; `crates/loom-patch/src/hierarchy/tests.rs`, `exposed_ranges_overlap_outer_then_inner`).
- [x] MIDI learn on a surface control maps the inner parameter with its whole travel, the engine sweeps the exposed range with it, and the file writes the mapping inside the definition (test: `crates/loom-app/src/levels/tests.rs`, `midi_learn_on_a_surface_control_maps_the_inner_parameter_within_its_range`).
- [x] An Absolute, a Pickup, and a Relative mapping on a parameter exposed at 200 to 2000 Hz sweep that range and never leave it; CC 0, 64, and 127 give 200, about 638, and 2000 Hz (tests: `crates/loom-modules/tests/subpatches.rs`, `an_absolute_mapping_sweeps_the_exposed_range`, `a_pickup_mapping_catches_and_stays_in_the_exposed_range`, `a_relative_mapping_steps_inside_the_exposed_range`).
- [x] A control change at 127 renders bit-identical to the value typed at 2000 Hz; with the span turned off in the compiler the renders differ (test: `crates/loom-modules/tests/subpatches.rs`, `a_control_change_at_the_top_renders_as_the_top_of_the_range`).
- [x] The mapping row of a parameter a surface narrows, inside and on the sub-patch tile, says Low and High are a share of the exposed range and names the surface; with no surface left the note goes. It names the span MIDI sweeps and the surface whose range takes part in it, as `fold_ranges` composes them (tests: `crates/loom-app/src/levels/tests.rs`, `a_mapping_row_says_its_range_is_a_share_of_the_exposed_range`, `the_range_note_names_the_span_midi_sweeps`, `the_range_note_names_the_surface_that_sets_the_span`).
- [x] The span follows a new range at the next commit with no new learn, and a control change from before the change is not set again outside it and the engine plays what the graph holds after the change (tests: `crates/loom-modules/tests/subpatches.rs`, `the_span_follows_a_new_range`; `crates/loom-modules/tests/ranges.rs`, `a_renewed_mapping_plays_what_the_graph_holds`).
- [x] The range MIDI sweeps is the outermost surface's, for overlapping and disjoint ranges, and an unexpose leaves the range below it or none (test: `crates/loom-patch/src/hierarchy/tests.rs`, `the_midi_limit_follows_the_outermost_surface_range`).
- [x] A waiting pickup's ghost mark sits at the hardware's value on each control's own scale: at the ends of the surface knob and the sub-patch inspector row for CC 0 and 127, a third and two thirds of the way along the inner knob, and on its own span on each nested surface (tests: `crates/loom-app/src/levels/tests.rs`, `a_ghost_mark_sits_on_each_controls_own_scale`, `each_nested_surface_marks_the_ghost_on_its_own_span`; `crates/loom-modules/tests/ranges.rs`, `a_waiting_pickup_reports_the_value_in_the_swept_span`), and control changes on limited mappings in every mode, with the range changing under them, allocate nothing on the audio thread (test: `crates/loom-modules/tests/no_alloc.rs`).
- [ ] Narrowing an exposed range in the panel editor moves a value outside it to the nearest end, and widening it keeps the value ([#76](https://github.com/brylie/LoomDSP/issues/76); test).
- [x] Three assignments on one parameter from both sides of a boundary sum after save and reload as they played, because the live order follows the file's after each assignment; without the reorder the render differs (tests: `crates/loom-patch/src/hierarchy/tests.rs`, `sync_puts_assignments_in_file_order_and_renumbers_only_the_movers`; `crates/loom-modules/tests/subpatches.rs`, `assignments_from_both_sides_sum_as_the_file_loads_them`).
- [x] A collapse that moves one of three sources inside reloads bit-identical to what plays after the collapse (test: `crates/loom-modules/tests/subpatches.rs`, `a_collapse_with_three_sources_reloads_as_it_plays`).
- [x] Renaming a sub-patch in its inspector changes the tile and the breadcrumb as one undo step and keeps the definition's key (tests: `crates/loom-app/src/levels/tests.rs`, `renaming_a_sub_patch_in_the_inspector`; `crates/loom-patch/src/hierarchy/tests.rs`, `renaming_a_sub_patch_changes_its_name_and_keeps_its_key`).

Copies, library, and linking:

- [x] Pasting a sub-patch tile and editing inside the copy does not change the original (tests: `crates/loom-app/src/library_tests.rs`, `pasting_a_sub_patch_tile_and_editing_inside_the_copy_leaves_the_original`, `duplicate_and_cut_carry_a_sub_patch_tile_and_paste_leaves_the_transport_out`; `crates/loom-app/src/clipboard/tests.rs`, `a_sub_patch_tile_copies_with_its_definition_and_pastes_back_whole`).
- [x] Saving a sub-patch to the library writes a patch file that opens on its own and plays the same (tests: `crates/loom-app/src/library_tests.rs`, `save_to_library_inside_a_sub_patch_writes_a_file_that_opens_on_its_own`; `crates/loom-modules/tests/subpatches.rs`, `a_sub_patch_saved_to_the_library_plays_the_same_inserted_again`; `crates/loom-patch/src/hierarchy/tests.rs`, `a_sub_patch_written_as_a_file_opens_on_its_own_and_inserts_back_the_same`).
- [x] Collapsing a selection that includes the Transport leaves it outside, and its cables into the selection become input ports (test: `crates/loom-patch/src/hierarchy/tests.rs`).
- [x] Inserting a patch file that has a Transport and a Scale and tuning module drops both, turns their cables into ports on the tile, and says so in the status bar; the patch still has one Transport (tests: `crates/loom-app/src/library_tests.rs`, `inserting_a_file_with_patch_wide_modules_leaves_them_out_with_a_note`; `crates/loom-patch/src/library/tests.rs`, `from_patch_leaves_patch_wide_modules_out_and_turns_their_cables_into_ports`). The Scale and tuning module is not built yet, so the test uses its reserved kind, and "one of each" is checked for the Transport only.
- [x] Two linked instances of one library file have separate definitions in the saved patch (tests: `crates/loom-app/src/library_tests.rs`, `two_linked_instances_have_separate_definitions`; `crates/loom-patch/src/hierarchy/tests.rs`, `two_linked_instances_have_separate_definitions_and_unlink_apart`).
- [x] Editing inside one of them unlinks it and leaves the other unchanged and linked (test: `crates/loom-app/src/library_tests.rs`, `editing_inside_a_linked_instance_unlinks_it_and_leaves_the_other`).
- [x] Save to library from one linked instance leaves the other instance unchanged in the open patch; reloading the patch updates it (test: `crates/loom-app/src/library_tests.rs`, `save_to_library_leaves_the_other_instance_until_the_patch_reloads`).
- [x] A linked instance picks up a changed library file on load, keeps exposed values by name, and the status bar names it (tests: `crates/loom-app/src/library_tests.rs`, `save_to_library_leaves_the_other_instance_until_the_patch_reloads`; `crates/loom-patch/src/library/tests.rs`, `refresh_replaces_a_differing_copy_and_keeps_exposed_values_by_name`).
- [x] A missing library file loads the stored copy with a status message (tests: `crates/loom-app/src/library_tests.rs`, `a_missing_library_file_loads_the_stored_copy_with_a_status`; `crates/loom-patch/src/library/tests.rs`, `a_missing_or_unreadable_library_file_keeps_the_stored_copy_with_a_note`).
- [x] A definition that contains itself through a library link keeps its stored copy on load with a note naming the loop, and a link that would make the open library file contain itself is refused with the loop message (tests: `crates/loom-app/src/library_tests.rs`, `a_link_loop_keeps_the_stored_copy_and_a_self_link_is_refused_on_insert`; `crates/loom-patch/src/library/tests.rs`, `a_link_loop_keeps_the_stored_copy_with_a_note_naming_the_loop`).

Engine:

- [x] A patch with three levels of nesting renders bit-identical to the same patch built flat, at block sizes 1, 77, and 512 (test: `crates/loom-modules/tests/subpatches.rs`).
- [x] A feedback loop that crosses a sub-patch boundary gets a unit delay, as a flat one does (test: `crates/loom-modules/tests/subpatches.rs`).
- [x] Editing inside a sub-patch keeps the state of the other inner modules (test: `crates/loom-modules/tests/subpatches.rs`, a module added inside while a note plays leaves the render unchanged).
- [x] No allocation on the audio thread during a slot swap or a select change, from a set Member value, MIDI, or audio-rate modulation (`crates/loom-modules/tests/no_alloc.rs`, `a_slot_swap_never_allocates`, `a_select_change_never_allocates`; `crates/loom-dsp/tests/no_alloc.rs`, `blends_never_allocate`).

Roles, slots, and modes:

- [x] The registry refuses a role declaration that misses a role parameter, naming it (tests: `crates/loom-core/src/registry/tests.rs`, `refuses_a_role_declaration_missing_a_parameter`; `crates/loom-core/src/role/tests.rs`).
- [x] Swapping a slot keeps cables and role parameter values; swapping back restores the first part's own values (tests: `crates/loom-patch/src/hierarchy/slots/tests.rs`, `swap_keeps_cables_carries_role_values_and_swapping_back_restores_the_rest`, `swapping_to_a_held_sub_patch_restores_its_definition`; `crates/loom-app/src/slots/tests.rs`, `swap_from_the_tile_menu_and_undo_fades_back`). Data waits for module data, which is not built; the held entry has room for it.
- [x] A slot swap during playback has no step larger than the crossfade allows: the largest step in the swap window is at most the steady audio's largest step plus the fade's largest gain change per sample times the peak, the same form as the output-safety click check (test: `crates/loom-app/src/qa_tests.rs`, `a_slot_swap_during_playback_has_no_click`; engine: `crates/loom-modules/tests/slots.rs`, the first frame is the leaving part alone, finishing the fade is bit-identical to leaving it, and the render is the same at block sizes 1, 77, and 512).
- [x] Layer mode sums members; select mode crossfades over 10 ms; stack mode with 4 copies and spread on a parameter gives each copy its offset value, at block sizes 1, 77, and 512 (tests: `crates/loom-modules/tests/modes.rs`, `a_layer_plays_the_sum_of_its_members`, `a_select_change_crossfades_over_10_ms`, `a_stack_of_four_gives_each_copy_its_spread_value`; the laws: `crates/loom-dsp/src/blend/tests.rs`; flattening: `crates/loom-patch/src/flatten/tests/modes.rs`).
- [x] Stack mode with stereo spread gives a stereo output (test: `crates/loom-modules/tests/modes.rs`, `stereo_spread_gives_a_stereo_output`).
- [x] A select change during playback has no step larger than the equal-power fade allows (test: `crates/loom-app/src/qa_tests.rs`, `a_select_change_during_playback_has_no_click`); turning back mid-fade reverses with no step, and a third member waits (test: `crates/loom-dsp/src/blend/tests.rs`).
- [x] A mode change alone never changes the sound: a stack of one plays as the single sub-patch, bit for bit; single to layer and back keeps every flat id and the file (tests: `crates/loom-modules/tests/modes.rs`, `a_stack_of_one_plays_as_the_single_sub_patch`; `crates/loom-patch/src/hierarchy/modes/tests.rs`, `single_to_layer_and_back_keeps_ids_and_the_file`).
- [x] Every edit inside a stack reaches every copy and undoes as one step: a knob turn during the drag, a cable, Copies, spread, and stereo spread; a mapping on a stack moves every copy (tests: `crates/loom-app/src/modes/tests.rs`; `crates/loom-patch/src/hierarchy/modes/tests.rs`, `copies_follow_the_mode_the_count_and_edits_inside`; `crates/loom-modules/tests/modes.rs`, `a_mapping_on_a_stack_moves_every_copy`).
- [x] Files with every mode round trip, a select's Member value, assignment, and mapping on the instance included (tests: `crates/loom-patch/src/hierarchy/modes/tests.rs`, `a_stack_round_trips_and_its_copies_are_not_tiles`, `select_member_round_trips_on_the_instance`; `crates/loom-patch/src/modes/tests.rs`).
- [x] The refusals of [How modes meet the rest](#how-modes-meet-the-rest-settled-2026-11-09) say their texts (tests: `crates/loom-app/src/modes/tests.rs`, `select_members_and_the_member_control`, `a_stack_refuses_dissolve_library_and_slots`; `crates/loom-patch/src/hierarchy/modes/tests.rs`, `mode_changes_refuse_what_the_spec_refuses`).

Built-in abstractions, empty slots, and slots inside a stack ([#700](https://github.com/brylie/LoomDSP/issues/700), [#698](https://github.com/brylie/LoomDSP/issues/698)):

- [x] Inserting the Synth voice plays a note that peaks between −12 and −1 dBFS; swapping its Filter slot from the tile menu keeps the cables and the cutoff, with no step larger than the swap fade allows (audio QA test: `crates/loom-app/src/qa_tests.rs`, `the_synth_voice_plays_and_swaps_its_filter_with_no_click`; inserting from the browser: `crates/loom-app/src/builtin/tests.rs`, `the_browser_inserts_a_builtin_as_a_copy`).
- [x] The Synth voice stacks for unison: Mode Stack, Copies 4, and a `fine tune` spread of 0.1 give the copies ±20 and ±6.7 cents, and a filter swap in that state has no click (audio QA test: `crates/loom-app/src/qa_tests.rs`, `the_synth_voice_stacks_for_unison`).
- [x] An empty slot added from the browser shows the role's ports and its note, drawn in the panel and read by a screen reader; an empty Filter slot passes its input through, an empty Sound source slot is silent, and an empty Envelope slot into an Amplifier's `level` plays its normal (`Ui` harness: `crates/loom-app/src/slots/tests/empty.rs`; the browser rows: `crates/loom-app/src/browser/tests.rs`; the audio: `crates/loom-modules/tests/slots.rs`, `an_empty_filter_slot_passes_its_input_through`, `a_part_into_an_empty_pulled_cable_slot_steps_until_rule_9`; flattening: `crates/loom-patch/src/flatten/tests/empty_slots.rs`).
- [x] A swap into an empty Filter slot, Remove part, and undo of the swap-in each fade with no step larger than the fade allows and leave no stray module; a swap into an Envelope slot steps until rule 9 (tests: `crates/loom-modules/tests/slots.rs`, `swaps_into_and_out_of_an_empty_thru_slot_fade`, `a_part_into_an_empty_pulled_cable_slot_steps_until_rule_9`; `crates/loom-app/src/slots/tests/empty.rs`, `remove_part_and_undo_back_to_empty_leave_no_stray_module`).
- [x] The Effect chain passes audio through every slot in order, each slot's mix works, including a middle value on the linear law, and an emptied slot is a thru (test: `crates/loom-modules/tests/effect_chain.rs`).
- [x] A Modulator slot swapped between two parts that both output 0.5 stays at 0.5 through the fade, at block sizes 1, 77, and 512 ([#698](https://github.com/brylie/LoomDSP/issues/698); test: `crates/loom-modules/tests/slots.rs`, `a_modulation_output_fades_linearly_so_a_shared_value_stays_put`).
- [x] A swap between two parts inside a stack of 4 fades in every copy, keeps each leaving copy's spread until the fade ends, holds no transient module in a save, is refused whole past the module limit, and allocates nothing on the audio thread (tests: `crates/loom-patch/src/hierarchy/slots/tests.rs`, `a_swap_inside_a_stack_fades_every_copy`; `crates/loom-modules/tests/slots.rs`, `a_swap_inside_a_stack_has_no_step_in_any_copy`; `crates/loom-modules/tests/no_alloc.rs`, `a_slot_swap_inside_a_stack_never_allocates`; `crates/loom-app/src/slots/tests/empty.rs`, `a_swap_past_the_module_limit_is_refused_whole`).
- [x] No module plays something other than its declared default on an unconnected fixed-value input, except the switched inputs ADR-0043 lists (`crates/loom-modules/tests/normals.rs`).
- [x] A long chain of empty thru slots flattens (10,000) and loads into the hierarchy (2,000) without overflowing a small stack (tests: `crates/loom-patch/src/flatten/tests/empty_slots.rs`, `a_long_chain_of_empty_thru_slots_flattens`; `crates/loom-patch/src/hierarchy/slots/tests.rs`, `the_hierarchy_follows_a_long_chain_of_empty_thru_slots`).

## Notes from building

Points the [Library](#library) and [Linked library items](#linked-library-items) sections left open, settled while building slice 4 (2026-11-08). Each is small; say so in review if one should change.

- **Save to library asks where for a copy, and not for a linked tile.** A tile that follows a library file writes that file, with no dialog. Any other tile opens the Save dialog in the library folder, under a free name made from the sub-patch's name, so the player can choose a subfolder ("Delays/Tape delay.ron"). Choosing a file that exists asks to replace it, as Save As does, so an edited copy of a library item can be saved back over its file. Saving does not link the tile: linking is a choice made with Insert linked. With the control channel on, the dialog case is refused, as Save As is.
- **What "an edit inside" is.** An edit unlinks a tile when it changes what the tile's definition, or any definition nested in it, would be saved as: a value that is not exposed, a module, a cable, an assignment, a MIDI mapping, an exposure, the name, or a tile's place inside. Moving a tile inside counts, because the file keeps positions and the next load would put them back. Turning an exposed parameter, from either control, does not count: its value belongs to the instance and is saved on it. The check compares the definitions' text before and after each undo step, so the unlink is part of that step, and a nested linked tile inside a linked tile unlinks both, since the outer refresh would replace the inner edit too. A MIDI control that moves a value inside which is not exposed counts too, although it makes no undo step: the same check runs when control changes arrive, so what plays is what saves (settled with the maintainer on 2026-11-08).
- **Unlink says the same as the first edit:** `Voice is now a copy. It no longer follows the library.`
- **Messages the spec left to the status bar.** A missing file: `Voice: the library file "Voices/Voice.ron" is missing, so its stored copy is used.` A file that does not load says `does not load (reason)` in its place. A patch-wide module with no cable: `Arp: its Transport was left out.`, since there is no port to connect. The Scale and tuning module says `its Scale and tuning was left out; connect its pitch ports.` An inserted tile: `Inserted Voice`, with any of those notes after it. A pasted text that held a patch-wide module: `Pasted 3 modules. The Transport was left out: a patch has one, at the top level.` (rule 8).
- **A linked tile looks like any other.** Nothing on the canvas marks a link; the tile menu's Unlink says it. A mark can come with the panel editor if players ask.
- **A loop is also refused at Save to library and at Insert:** a sub-patch that holds a linked tile of the very file being written would make that file contain itself, and so would inserting, into a library file opened on its own, a link to that file or to a file whose links reach it. A loop met while loading is not refused: the stored copy plays, with a note (rule 5).
- **Saving over a former library file does not re-link the tile.** An edited copy saved back over the file it came from stays a copy; Insert linked is the one way to link (settled with the maintainer on 2026-11-08).
- **The Scale and tuning module's kind is reserved** (`loom.pitch.scale`, [scale and tuning](scale-and-tuning.md)) in `loom-patch`, so a file that holds one is left out of a sub-patch before the module exists.
- **Not built here:** patch info's "Includes parts by" line ([credit for parts](patch-metadata.md#credit-for-parts)), and a way to edit a sub-patch's own metadata. The definition keeps the authors a library item came with, and Save to library writes them.

### Notes from building: slots

Points settled while building slice 5 (2026-11-08), beyond what the proposal put to the maintainer. Each is small; say so in review if one should change.

- **A patch file carries `roles` too**, as it carries `exposed`: a library item declares the roles it fills, and Save to library and Insert copy the list between the file and the definition. Omitted when empty.
- **Collapsing a selection that holds a slot, and dissolving a sub-patch that holds one, are refused for now**, with `Cannot make a sub-patch from a slot yet; dissolve the slot first` and `Cannot dissolve a sub-patch that holds a slot yet; dissolve the slot first`. Collapse and dissolve move modules by path, and a slot's part must move with its slot. A follow-up moves them together.
- **The tile menu's items.** `Make Filter slot` on a module or sub-patch tile, one per role it declares. On a slot: `Swap to Ladder` for every part but the current one, then `Dissolve slot`. On a sub-patch tile whose surface has a control bound to a slot's role parameter: `Swap Filter to Ladder`. Menu labels are built from names, so the menu's rows are text, not fixed strings.
- **The slot tile's hover text** says `Slot for Filter. Holds Ladder. Swap from its menu.`, with the role's kind as the name. An empty slot's panel draws its note as text; [Empty slots](#empty-slots-agreed-2026-11-14) has the wording (changed 2026-11-14: it was hover text only).
- **The inspector for a slot** shows the name, `Slot for Filter`, a **Part** list with the current part selected, the role's rows with the part's own labels, then `Ladder only` over the part's other rows, then **Dissolve slot**. A sub-patch part's surface items are its role rows.
- **Swapping to the part the slot holds is refused** with `Cannot swap Filter: it holds that part already`.
- **A role parameter the new part lacks counts as one only the leaving part had**: an assignment or mapping on it is removed and named, as for a parameter outside the role. Undo brings them back with their ids, along with the part.
- **A swap asked for during a fade, or a refused swap, finishes the fade first** on the live session, so a refusal leaves the graph and the hierarchy in step and makes no undo step. A swap that fails part-way takes back the modules it added and puts back the assignments and mappings it moved, with their ids.
- **An exposure of an optional role parameter the current part lacks stays on the slot** with nothing behind it, so a part that has it fills it again; the file writes no value for it on the part. Dissolve slot drops such an exposure, since the plain module has no parameter to carry it.
- **A save during a fade writes a modulation from the slot's output as coming from the new part**, which is what finishing the fade makes it, so the file holds it whatever the engine has run; an undo snapshot records it the same way.
- **An undo snapshot leaves the fading modules out**, and undo or redo first finishes any fade in progress. A step that changes a slot's part is applied as a swap at the id the part had, so the step's assignments and mappings on it still apply, and the leaving part keeps its id and path across the restored snapshot.
- **A sub-patch part is stored whole when it leaves**, with the definitions nested in it, under its key in the `subpatches` table. If a new definition takes a nested key meanwhile, the stored nested definitions are rekeyed when the file is written.
- **A library item swapped in is inserted as a copy**, and its own linked items are refreshed first, as Insert does.
- **Not built here:** a slot tile on the clipboard, a menu on a control, moving a slot through collapse or dissolve, and data on held parts (module data is not built). An empty slot from the browser came with [#700](https://github.com/brylie/LoomDSP/issues/700).

### Notes from building: modes

Points settled while building slice 6 (2026-11-09), beyond what the proposal put to the maintainer. Each is small; say so in review if one should change.

- **A control has one MIDI mapping, so a stack's copies follow it.** The engine keeps one mapping per control (MIDI learn, the feedback table), so copies 1 to N-1 cannot each have one. A mapping on copy 0 carries the same parameter of every other copy as **followers** (`Controller::set_midi_followers`), and the engine sets them to the value the mapping sets, on the audio thread, with no allocation. The flattener turns the copies' mappings, and a mapping into a stack from outside, into followers. What the player sees is what the proposal said: MIDI on a stack moves every copy.
- **The Member control is an exposed item of the Member module.** It sits first in the select definition's exposed list, named `member`, with the Member module's reserved level id and a range of 1 to the member count, so every surface path (the tile control, the inspector row, modulation, MIDI learn, exposing it again on an outer surface) works on it unchanged. The file never holds that entry; the editor makes it again on load and after every sync. It has no Unexpose button.
- **A layer or a select of one member, and a stack of one copy with no stereo spread, add nothing**, so they are the single sub-patch, module for module. A select of one member still has its Member module and Selects, which play the one member exactly.
- **Within a member, ports come in name order**, after the member order, since a member's own ports have no other order the flattener keeps.
- **The added modules' inputs are named `in1` to `in16`**, as the Mixer's are, and the Select also has a `members` parameter: how many of its inputs count, so a Member past the last plays the last.
- **A seed offset through nested stacks** is copy k times 16 to the power of the depth, so copies of copies never share a seed.
- **A new Select starts on its member with no fade**, so a select added by a paste or a mode change does not fade in from member 1.
- **Add member places the new member to the right of the others**; removing one is the member tile's delete, which keeps the member list in step.
- **A spread of 0 is not in the file**, and the inspector's slider shows every exposed item with its amount, 0 included.
- **The clipboard carries a sub-patch with a mode whole**, with its members' definitions and its Member value, by inserting its definition rather than a patch file, which has no place for a mode.
- **Collapse and cables at a layer or select level** are refused with the same text as other modules: `A layer holds only its members; open a member to add modules`.
- **Fills role has no gesture in the app yet**, so a mode change on a sub-patch that declares a role (from a file or the library) is the one place the role refusal shows.
- **Not built here:** saving a sub-patch with a mode to the library, a sub-patch with a mode filling a role, one surface control for a parameter in every member, and pausing unselected members. Slots inside a stack came with [#700](https://github.com/brylie/LoomDSP/issues/700).

## Out of scope

- Polyphony and voice allocation (FR-VOICE-*): the [polyphony](polyphony.md) specification. Stack mode is not voices.
- The panel editor's tools: placing, aligning, and grouping widgets ([#76](https://github.com/brylie/LoomDSP/issues/76), [panels](panels.md#the-panel-editor)).
- Embedding a sub-patch's panel inside its parent's panel (FR-PANEL-08). After the panel editor.
- Watching library files while a patch is open.
- Converting data between parts on a swap (OQ-21).
- A slot tile on the clipboard, and a menu on a control (so Swap can sit on the bound widget itself): follow-ups of slice 5.
- Saving a sub-patch with a mode to the library, a sub-patch with a mode filling a role, one surface control across members, and pausing unselected members: follow-ups of slice 6.
- Follow-ups of [#700](https://github.com/brylie/LoomDSP/issues/700): swapping a nested slot from the outer tile's menu ([#765](https://github.com/brylie/LoomDSP/issues/765)); a stack filling the roles its inside fills, so the Synth voice can declare Voice ([#766](https://github.com/brylie/LoomDSP/issues/766)); Shaper as a thru role ([#767](https://github.com/brylie/LoomDSP/issues/767)); and swaps into and out of an empty slot fading per copy inside a stack ([#768](https://github.com/brylie/LoomDSP/issues/768)).
- Sharing the library online (M4, FR-SHARE).

## Open points

- **CPU in select mode.** Every member runs, so a select of 8 members costs 8 members. Measured on 2026-11-09 ([benchmark](../benchmarks/2026-11-09-sub-patch-modes.md)): a select of 8 saw and SVF voices takes 19.9 µs per 128-frame block, a layer of 8 20.9 µs, and one voice alone 4.2 µs, under 1% of a core at 48 kHz. Pausing is not built. Leaning, if a heavier abstraction needs it: pause a member that has been silent and unselected for a while, decided at a fixed sample count, not a block, so output stays block-size invariant.
