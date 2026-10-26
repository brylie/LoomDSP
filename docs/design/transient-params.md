# Transient Parameters Specification

Parameters that carry a live performance gesture to a module without being saved in the patch: a held button, a reset press.

- **Kind:** living specification.
- **Status:** Implemented ([#161](https://github.com/brylie/LoomDSP/issues/161)) for the Button's `hold`. The Transport's `reset` comes with the Transport module ([#73](https://github.com/brylie/LoomDSP/issues/73)).
- **Decision behind it:** none. It extends the parameter model of [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) and the patch format of [ADR-0008](../adr/0008-patch-format-ron.md) without changing either decision.
- **Use case and requirements:** UC3, FR-LIB-27, FR-LIB-12.
- **Prior art:** none needed.
- **Issues:** [#161](https://github.com/brylie/LoomDSP/issues/161).
- **Code:** `ParamOptions::transient` and `ModuleDescriptor::saved_values` in `crates/loom-core/src/descriptor.rs`, the registry check, `Patch::from_graph`, `load_into`, and `transient_values` in `loom-patch`, the button in `loom-modules/src/ui.rs`, and undo, clipboard, inspector, and MIDI learn in `loom-app`.

## Goal

> I hold a pad to open a gate while I play. I save the patch in the middle of a phrase. Next week it opens with the gate closed, and the patch is not marked as changed because I pressed a pad.

A hold is performance state, not patch state.

## Behaviour

A parameter can be marked **transient** in its `ParamOptions` (`.transient()`). A transient parameter:

- is sent to the engine like any parameter, and read by the module like any parameter;
- is **not saved** in the patch file and is **not loaded** from it (a saved value for it is ignored, with a note from `loom check`);
- **does not mark the patch as changed**, and is **not an undo step**;
- **cannot be the target of a modulation assignment, MIDI learn, or the clipboard** (paste and duplicate copy the default);
- starts at its default when a module is created, loaded, pasted, or reset.

The inspector shows no row for a transient parameter. A panel widget may be bound to it (a button's hold, the Transport's reset).

## Rules

1. **Only whole-number parameters can be transient.** The registry refuses others. *Why:* the need is a hold or a press, which is 0 or 1. A continuous transient value has no use yet.
2. **A transient default must be 0.** *Why:* "nothing is happening" is the only safe state to start in.
3. **A transient parameter changes the audio only as the module chooses.** Like any parameter, it never changes the engine. *Why:* determinism (AGENTS.md hard rule 3) holds for a given sequence of commands.
4. **A press and a release that arrive before the same buffer are applied in order.** A module that wants to see a very short press needs its own latch. *Why:* this is how the command queue already works. It is not solved here.
5. **Transient parameters are in the live-value table and the UI snapshot,** so a widget can show them and the control channel can read them. *Why:* a held button must look held.
6. **Switching a button's mode does not touch the saved latch.** See [Button details](clock.md#button-details).

## Uses

| Module | Transient parameter | Meaning |
|---|---|---|
| Button (`loom.ui.button`) | `hold` | The momentary hold. The saved `on` stays the toggle latch. |
| Transport (`loom.clock.transport`) | `reset` | A press resets the transport to bar 1. |

## Acceptance criteria

- [ ] A patch saved while a momentary Button is held loads with the gate low. The patch file has no `hold`.
- [ ] Pressing and releasing a Button in momentary mode leaves the patch unchanged (no unsaved-changes mark, no undo step).
- [ ] The registry refuses a transient parameter that is not a whole number or whose default is not 0.
- [ ] A saved value for a transient parameter is ignored on load, and `loom check` reports it.
- [ ] A transient parameter cannot be chosen as a modulation target or a MIDI learn target.
- [ ] Copy and paste of a Button gives a released button.
- [ ] The hold and the reset are in the UI snapshot.

## Out of scope

- Continuous transient values.
- Short presses that land inside one buffer (rule 4).
- Letting other modules write a transient parameter by cable.

## Open points

- None.
