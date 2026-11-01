# Scale and Tuning Specification

How a patch holds one tuning and a set of scales, how pitch modules follow them without cables, and how the patch changes key by itself.

- **Kind:** living specification.
- **Status:** Agreed, P2 (2026-11-01). The choices marked "Settled" were made with the maintainer. Nothing is built yet.
- **Decision behind it:** [ADR-0033](../adr/0033-one-scale-and-tuning-per-patch.md) (one scale and tuning per patch). Settles OQ-22.
- **Use case and requirements:** FR-LIB-29, FR-SIG-05, FR-LIB-19. Trait TR-08 (scale awareness).
- **Builds on:** [Module data](module-data.md) (the scales are a pattern slot), [Panel widgets: step grid](panel-widgets.md#step-grid) (the keys layout), [Clock and transport](clock.md) (the same pattern of patch-wide state).
- **Background:** an explainer on equal divisions and just intonation belongs in `docs/learn/` with the first build.
- **Prior art:** the Scala tuning file format (`.scl`) is a published, plain-text format used by many tools. We read it; we do not copy any tool's behaviour or tuning list.
- **Issues:** [#309](https://github.com/brylie/LoomDSP/issues/309). Users: [#308](https://github.com/brylie/LoomDSP/issues/308) (Pitch quantiser), [#307](https://github.com/brylie/LoomDSP/issues/307) (Multi-lane sequencer).
- **Code:** none yet.

## Goal

> My patch plays in A minor. At the end of the chorus, a bar trigger switches every quantiser and sequencer to C major at once. Another day I try the same patch in 19 equal divisions of the octave, and it still sounds in key.

## Terms

| Term | Meaning |
|---|---|
| **Tuning** | How the octave (or another period) is divided: a list of pitch classes, each an offset in cents from the reference, and the period. |
| **Pitch class** | One step of the tuning, in any octave. Class 0 is the reference: pitch 0.0, middle C in a 12-note tuning (FR-SIG-05). |
| **Scale** | A root (a pitch class) and a set of pitch classes that are in. |
| **Active scale** | The one scale the patch follows now. |

## Tunings

**Settled:** built-in equal divisions and two just sets, plus Scala import.

| Built-in tuning | Classes |
|---|---|
| Equal divisions of the octave | 5, 7, 12 (default), 17, 19, 22, 24, 31, 41, 53 |
| Just, 5-limit, 12 notes | 1/1, 16/15, 9/8, 6/5, 5/4, 4/3, 45/32, 3/2, 8/5, 5/3, 9/5, 15/8 |
| Just, 7-limit, 12 notes | 1/1, 15/14, 8/7, 7/6, 5/4, 4/3, 7/5, 3/2, 8/5, 5/3, 7/4, 15/8 |

- **Import:** a Scala `.scl` file, read on the control thread. Its pitches become cents; its last pitch is the period. The tuning is stored in the patch, so it travels with it ([module data](module-data.md)). A file that does not parse, or with more than 64 classes, is refused with a message naming the line. *Why 64:* a scale is one pattern lane of 64 cells ([data kinds](panel-widgets.md#data-kinds)).
- **Pitch of a note:** class `k` in octave `o` has pitch `(o × period + cents_k) / 12000`, in LoomDSP pitch units (0.1 per octave of 1200 cents). The octave numbering puts class 0 of octave 0 at pitch 0.0.

## The Scale and tuning module (MT-PITCH-03)

`loom.pitch.scale`:

| | |
|---|---|
| Inputs | `next` (trigger, default 0), `select` (default 0), `reset` (trigger, default 0) |
| Outputs | `root` (pitch of the active root in octave 0), `changed` (trigger when the active scale changes) |
| Parameters | `tuning`: a choice of the built-in tunings and "Imported" (shown only when one is). `mode`: 0 Step (default), 1 Select. `scales`: whole number 1 to 8, default 1. `root1` to `root8`: whole number 0 to 63, default 0, clamped to the tuning's classes |
| Data | `scales`: a pattern slot of 8 gate lanes, one cell per pitch class. `tuning`: the imported tuning, when there is one |
| Panel | A select list for `tuning`; a step grid in the keys layout showing the active scale, with its root marked; radio buttons for `mode`; a knob for `scales`. The [larger view](panel-widgets.md#larger-view) shows every stored scale as a row. |

**Settled:** one tuning and up to eight scales.

- A new module has scale 1 = every class in (chromatic), root 0.
- **Step:** each `next` trigger moves to the next scale, wrapping after `scales`. **Select:** the active scale is `floor(select × scales)`, clamped. Mode is a setting, not chosen by a cable ([P1 modules](p1-modules.md#sample-and-hold-loomrandsample-hold)).
- `reset` goes back to scale 1.
- **A change applies on the trigger's sample.** *Why:* the patch chooses the moment. A bar trigger from the Transport changes key on the bar; a Chance in front makes it "sometimes".
- Changing the tuning or editing a scale is an edit, applied at a buffer boundary and covered by undo, like any module data.
- **One per patch, at the top level.** Adding a second is refused: `A patch has one Scale and tuning module.` Adding one inside a sub-patch is refused, even when the patch has none: `The Scale and tuning module lives at the top level.` A paste leaves it out when the patch already has one, or when pasting inside a sub-patch; the rest of the paste goes ahead, cables to it are dropped, and the status bar says so. Collapse leaves it outside, and inserting a file drops it ([sub-patches, rule 8](sub-patches.md#rules)). *Why:* "patch-wide" must mean one answer.

## Following the scale

- The engine gives every module the active scale and the tuning through `ProcessContext`, as fixed-size tables (64 classes). No allocation on the audio thread (AGENTS.md rule 1).
- A patch with no Scale and tuning module gives 12 equal divisions, every class in, root 0.
- **The hidden cable.** The compiler orders every module that follows the patch scale after the Scale and tuning module, as if a cable joined them. *Why:* a key change must reach them on the same sample. A loop through that hidden cable is broken with a unit delay and reported, as any loop is ([ADR-0002](../adr/0002-graph-execution-and-feedback.md)).
- **Scale awareness (TR-08).** A scale-aware module has a `scale` setting: 0 Patch (default) or 1 Own. Own uses a one-lane scale pattern and a `root` stored in the module, over the patch tuning. *Why the tuning is never the module's own:* every pitch module must agree on what a note is.

## Rules

1. **One tuning per patch.** *Why:* notes mean the same everywhere in a patch.
2. **Key changes are sample-accurate and block-size invariant.** *Why:* AGENTS.md rules 2 and 3.
3. **Imported tunings live in the patch.** *Why:* a patch must play the same on another computer without the `.scl` file.
4. **No module means 12-tone equal temperament, chromatic.** *Why:* patches without a scale behave as today.

## Acceptance criteria

- [ ] Each built-in tuning gives the listed cents for each class, and pitch 0.0 is class 0 of octave 0 (unit tests).
- [ ] A Scala file imports, saves in the patch, and reloads without the file; a broken file is refused with its line number (tests).
- [ ] A `next` trigger switches the scale, and a quantiser later in the patch uses the new scale on the same sample, at block sizes 1, 77, and 512 (reference test).
- [ ] A quantiser placed before the Scale module in the module list still switches on the same sample (test: the hidden cable orders them).
- [ ] A second Scale and tuning module is refused (test).
- [ ] Adding a Scale and tuning module inside a sub-patch is refused, even when the patch has none; pasting a group that holds one inside a sub-patch adds the rest without it (tests).
- [ ] A patch without the module quantises to 12 equal divisions, chromatic (test).
- [ ] A module set to Own ignores a patch key change, but follows a patch tuning change (test).
- [ ] Switching scales does not allocate on the audio thread (`no_alloc` test).

## Out of scope

- Tunings with more than 64 classes ([panel widgets, open points](panel-widgets.md#open-points)).
- Tunings that change on a trigger. Only the scale switches.
- Retuning MIDI input notes to the tuning (the note input stays 12-note until the note model is settled, OQ-16).
- Keyboard mapping files (Scala `.kbm`).

## Open points

None.
