# Unknown Modules Specification

How a patch loads when it uses modules this build of LoomDSP does not have: each one becomes a silent placeholder tile that keeps its data and saves it back unchanged.

- **Kind:** living specification.
- **Status:** Implemented ([#18](https://github.com/brylie/LoomDSP/issues/18)).
- **Decision behind it:** none. It builds on the patch format ([ADR-0008](../adr/0008-patch-format-ron.md)) and on the rule that the patch format stays at version 1 during the early alpha (AGENTS.md).
- **Use case and requirements:** FR-FILE-05, NFR-UX-05. Later: FR-SHARE-02 (M4).
- **Background:** none needed.
- **Prior art:** many modular and DAW tools keep a missing plugin or module as an empty slot, so a project survives a trip to a computer without it. We describe our own behaviour here.
- **Issues:** [#18](https://github.com/brylie/LoomDSP/issues/18). Related: [copy and paste](clipboard.md) (pasting an unknown module), [#101](https://github.com/brylie/LoomDSP/issues/101) (keyboard).
- **Code:** Loading: `crates/loom-patch`. Silent processing: `crates/loom-core`. The tile and inspector: `crates/loom-app` and `crates/loom-canvas`.

## Goal

"A friend sent me a patch made with a newer LoomDSP. It uses a module my version does not have. I still want to open it, play the rest, and send it back without losing their work."

The patch loads, the unknown module shows as a placeholder, and saving writes it back as it was.

## Behaviour

### Which modules are unknown

A module is unknown when its `kind` (`"id@version"`) is not in this build's registry. That includes a known module at a version this build does not have, such as `loom.osc.saw@2` when only `@1` exists.

| In the patch file | What happens |
|---|---|
| A `kind` the registry does not have | The module loads as a placeholder |
| A `kind` that is not `id@version` | The load fails, as today. No LoomDSP writes it, so the file was damaged or edited by hand |
| A known module with an unknown parameter or port | The load fails, as today (Out of scope) |

### The placeholder

- **Ports.** The placeholder shows the ports the patch names on it: an input for each port a cable goes into, and an output for each port a cable or a modulation assignment comes from. They are in the order the file first names them. Each port is labelled with its name from the file. Its hover text says "Not known to this version of LoomDSP". A port that the module has but the patch never names is not shown, because nothing in the file says it exists.
- **The tile.** Its title is the module's label if it has one, else its `kind`. Its body shows a question mark and "Unknown module". Its border is dashed. It has no controls, lights, or meters. It is sized to fit its ports, like any tile. *Why a dashed border and a mark:* the tile must look different by shape and text, not only by colour (NFR-UX-03).
- **The inspector.** It shows the `kind`, then "This version of LoomDSP does not have this module. It makes no sound. Its settings are kept and saved." An editable **Label** field changes the tile title. Pressing Enter or **Apply** saves the field as one undo step. An empty field clears the label and restores the kind as the title. Typing before applying is session-only. Below that, the saved parameters, by name, with their numbers as the file has them, are read only. Assignments and MIDI mappings that touch it are listed as for any module.
- **Sound.** Every output is 0. Inputs are read by nothing. The placeholder is scheduled like any other module, so a feedback loop through it is handled as any loop is.
- **On load,** the status bar says how many modules are unknown and what to do, for example: **2 modules are not in this version: `loom.fx.shimmer@1`, `loom.osc.saw@2`. They make no sound. Their settings are kept when you save. Open the patch in a newer LoomDSP to play them.** With more than three kinds, it names three and says how many more.

### What the user can do with it

| Action | Allowed | Notes |
|---|---|---|
| Move, select, label | Yes | Change the label in the inspector |
| Delete | Yes | Its cables, assignments, and MIDI mappings go with it, as for any module. One undo step |
| Copy, paste, duplicate | Yes | The copy is a placeholder with the same data ([clipboard](clipboard.md)) |
| Remove one of its cables or assignments | Yes | The port stays shown until the patch is saved and opened again, so the tile does not change shape under the pointer |
| Add a cable or an assignment to or from it | No | The status bar says **Cannot connect to an unknown module** |
| Change its parameters | No | There are no controls, and the inspector shows them read only |
| MIDI learn on its parameters | No | Its mappings in the file are kept |

*Why no new cables:* this build does not know the module's real ports. A cable to a port the module does not have, or with the wrong meaning, would load as an error or do the wrong thing in the version that has the module.

## Rules

1. **Nothing is lost.** Saving a patch that has placeholders writes each one back with the same `id`, `kind`, `label`, `params`, `position`, and `ranges`, and writes every cable, assignment, and MIDI mapping that touches it, unchanged, unless the user moved, labelled, or deleted something. *Why:* the patch belongs to the person who made it, and a trip through an older version must not damage it (FR-FILE-05).
2. **A placeholder is silent and inert.** It outputs 0 and has no state. *Why:* playing anything else would be a guess. Silence is the safe thing, and the output safety stage still runs after it.
3. **Its links are kept but do nothing.** Assignments it drives and MIDI mappings on its parameters are kept and saved. The engine skips MIDI mappings on its parameters, since it has none to set. An assignment from a placeholder output to a known module's parameter stays in the engine but moves nothing, even when it forces a polarity. *Why:* bipolar-to-unipolar remapping would turn a silent zero into 0.5. The user who opens the patch in the newer version gets the same routing back.
4. **The user is told once, on load.** The status bar message appears when the patch is opened, not after every edit. Autosave recovery shows it too. *Why:* NFR-UX-05, and a message that repeats on every edit gets ignored.
5. **Determinism holds.** A patch with placeholders renders the same at every block size and in every run (hard rules 2 and 3).

### On the command line

- `loom check` lists the unknown modules and exits with status 1. *Why:* `check` asks whether this build can play the patch. With unknown modules it cannot, fully, and a script or CI job should see that.
- `loom render` warns on stderr, names the unknown modules, and renders with the placeholders silent. *Why:* the user may want to hear the rest of the patch.

## Acceptance criteria

- [x] Given a patch with an unregistered `kind`, when it loads, then it loads with a placeholder that has an input for each cable into it and an output for each cable or assignment from it, in file order (patch test).
- [x] Given a known module at an unregistered version, it loads as a placeholder (patch test).
- [x] Given a `kind` that is not `id@version`, or a known module with an unknown parameter, the load still fails as today (patch test).
- [x] Load, then save with no edits: the saved patch equals the loaded one, including the placeholder's `params`, `ranges`, `label`, and `position`, and its cables, assignments, and MIDI mappings (patch test).
- [x] The placeholder's outputs are 0 at block sizes 1, 77, and 512, and a module fed by a cable from it reads 0 on that input (engine test).
- [x] On load, the status bar names the unknown kinds once, and an edit after that does not repeat it (app test).
- [x] Dragging a cable onto a placeholder port, or from one, adds nothing and the status bar says why. Delete, move, label, copy, and paste work, each with one undo step (app tests).
- [x] The inspector for a placeholder shows its `kind`, the message, and its saved parameters read only (app test).
- [x] `loom check` on a patch with an unknown module exits 1 and lists it. `loom render` renders it and warns (CLI tests).
- [x] Manual check: the placeholder tile looks different from a known tile by shape and text, in a window screenshot.

## Out of scope

- Unknown parameters or ports on a known module. A known module that gained a parameter in a newer build would keep its version (`@1`) only if the change is compatible; that rule belongs to the patch migration work that starts with the first release (FR-FILE-02, ADR-0008).
- Saying where a missing module comes from or how to get it (FR-SHARE-02, M4).
- Placeholders in exported plugins and the web runtime. The export spec decides what an export does with them.
- Showing unknown modules in the module browser. Only modules this build has are listed.

## Open points

- None.
