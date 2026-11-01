# MIDI Learn Specification

How a performer links a hardware knob or fader to any parameter in a few seconds, and how those links are saved, shown, and removed.

- **Kind:** living specification.
- **Status:** Agreed. [Device mappings](#device-mappings-fr-midi-06) Agreed, M2 (2026-11-01, [#166](https://github.com/brylie/LoomDSP/issues/166)); not built. [Mapping modes](#mapping-modes-fr-midi-09) Agreed, M2 (2026-11-01, [#165](https://github.com/brylie/LoomDSP/issues/165)); not built.
- **Decision behind it:** none of its own. Builds on [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (control and audio threads), [ADR-0004](../adr/0004-export-as-runtime-plus-patch-data.md) (export as runtime plus patch data), [ADR-0008](../adr/0008-patch-format-ron.md) (patch format), and [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (the inspector shows and removes mappings; learn mode extends the gesture to tile controls).
- **Use case and requirements:** [UC3, perform live](../product/personas-and-use-cases.md#uc3-perform-live-tomas-highest-priority-use-case), FR-MIDI-02. FR-MIDI-06 (per-device mappings, [own section](#device-mappings-fr-midi-06)). FR-MIDI-09 (pickup, relative encoders, scaling, [own section](#mapping-modes-fr-midi-09)).
- **Background:** [parameter mapping](modulation.md#parameter-mapping) (value to position and back).
- **Prior art:** most DAWs and software modular tools have a "learn" gesture: arm a control, move a hardware control, and the two are linked. We use the idea, in our own words.
- **Issues:** [#24](https://github.com/brylie/LoomDSP/issues/24), [#166](https://github.com/brylie/LoomDSP/issues/166) (device mappings).
- **Code:** `crates/loom-core/src/midi_map.rs` (mappings and feedback), `Engine::apply_midi_maps`, the `Controller` MIDI methods, the `midi` list in `loom-patch`, `crates/loom-app/src/learn.rs` (learn mode), and `LearnView` in `loom-canvas`. Tests: `crates/loom-modules/tests/midi_learn.rs`, and the learn mode tests in `loom-canvas`'s `widget/tests.rs`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

> I touch the filter cutoff, turn knob 3 on my controller, and from then on knob 3 moves the cutoff. When I save the patch and open it tomorrow, knob 3 still moves the cutoff. If I change my mind, I remove the link in the inspector, or learn knob 3 onto another control.

## Terms

| Term | Meaning |
|---|---|
| **Mapping** | One link from a MIDI control change (a channel and a controller number) to one parameter. |
| **Learn mode** | A mode of the app where touching a control picks it for mapping instead of changing its value. |
| **Armed parameter** | The one parameter that the next control change will be mapped to. |

## Behaviour

### Making a mapping

The user touches the control on screen, then moves the hardware control. Learn mode makes the touch safe: it arms the control and does not change its value.

| Action | Result |
|---|---|
| Press **MIDI learn** in the toolbar | Learn mode starts. The button stays lit. Every control bound to a parameter, on tiles and in the inspector, gets a learn outline. Mapped controls show their mapping as a small label, for example `CC 74`. |
| In learn mode, click a knob, toggle, or radio button on a tile, or a parameter row in the inspector | That parameter is armed. The control is highlighted and the status bar says **Move a MIDI control to map it to <module> <parameter>**. The value does not change. Any parameter armed before is disarmed. |
| Move a hardware knob or fader while a parameter is armed | A mapping is made from that control change (channel and controller number) to the armed parameter. The status bar says which control was mapped. Nothing is armed now, and learn mode stays on, so the next control can be mapped at once. |
| In learn mode, click an armed control again | It is disarmed. |
| In learn mode, press Space or Enter on a control focused with Tab | The same as a click: it arms or disarms the control. The arrow keys do nothing. With no control focused, Space still starts and stops the transport, because that is not a parameter. |
| Press **MIDI learn** again, or press Escape | Learn mode ends. An armed parameter is disarmed with no change. |

- **Touch first, then the hardware.** *Why:* the user points at what they want to control and then shows which hardware control does it. This is direct manipulation, and it is the order the user thinks in. Learning from a list in the inspector alone is harder to follow.
- **In learn mode, nothing on screen changes a value.** Not a click, a drag, or a key. Only the hardware control that makes or uses a mapping does. *Why:* the mode exists so that touching a control on screen is safe.
- **Learn mode stays on after a mapping.** *Why:* the common task is to map several knobs in a row (the M1 goal maps four). Leaving the mode after each one would add a click per knob.
- **Outside learn mode, touching a control only changes its value,** as today. *Why:* moving a hardware control after touching a screen knob is normal in play. It must never remap anything by accident.
- **Only one parameter is armed at a time.** *Why:* the next control change must have exactly one target, so the result is never a surprise.
- **One hardware control drives one parameter, and a parameter has one hardware control.** Learning a parameter that already has a mapping replaces it. Learning a hardware control that already has a mapping moves it to the new parameter, and the status bar names the parameter it left. *Why:* this is the least confusing rule. What a hardware knob does is always one thing, and the label on screen tells the whole story. Two hardware controls on one parameter would fight and make the value jump. One knob that drives many parameters is a job for the Macro module (MT-UI-02, M2): map the hardware to the macro, and the macro drives many parameters through modulation, where depth and the modulation views show the fan-out.
- **The control change that makes a mapping also sets the value.** *Why:* the user sees and hears at once that the link works.

### Playing

- A mapped control change sets the parameter's **base value**, the same value the knob on the tile sets. Modulation still adds on top of it ([modulation](modulation.md), rule 2).
- The value 0 to 127 becomes a position from 0 to 1 (`value / 127`), and then a value with `ParamSpec::denormalize`. So 0 is the minimum, 127 is the maximum, and a logarithmic parameter feels the same on hardware as on screen. Whole-number parameters round, as `denormalize` already does.
- The tile knob and the inspector follow the hardware, so the screen always shows the value that plays.
- Moving the knob on screen still works. The last change wins, from either side. A mapped parameter can jump when the hardware control moves after a screen change. Pickup modes that prevent the jump are FR-MIDI-09, M2.
- A parameter with smoothing (`smooth_seconds`) glides to the new value as it does for a screen knob. *Why:* a 7-bit control change moves in steps of about 0.8 %. Without smoothing, a step on a cutoff or a level can be heard as a click.

### Seeing and removing mappings

- The inspector shows a parameter's mapping on its row as `CC <number> · ch <channel>`, with channels counted from 1 as on hardware. A small **×** button next to it deletes the mapping ([removing an item](panels.md#removing-an-item)). This works in and out of learn mode.
- In learn mode, the labels on the tiles show every mapping in view at a glance. *Why:* with one parameter per hardware control, the labels are a full picture of the mappings. A separate mapping list is not needed in M1.
- Deleting a module deletes its mappings.

## Rules

1. **Mappings run in the engine.** The controller compiles the patch's mappings into a table and sends it to the audio thread with the schedule. The engine reads each incoming control change, looks it up, and sets the base value. *Why:* the same patch must respond to the same controller in the desktop app, in `loom play`, and in an exported plugin (ADR-0004), none of which can rely on the app's UI thread. It also adds no latency beyond the audio buffer.
2. **The engine reports values back to the controller.** When a mapped control change arrives, the engine writes it into a latest-value table: one entry per control (16 channels × 128 controller numbers), holding the mapping's id, a sequence number, and the 7-bit value. The controller reads the table, turns the value into a parameter value with the same mapping, and updates its graph. Three ordering rules keep the graph and the engine in step:
   - The controller reads the table before it compiles, because compiled base values come from the graph.
   - A screen edit on a mapped parameter carries a token. When the engine applies it, it stores the token with a sequence number that it shares with control changes. Until then, the controller ignores control changes for that parameter, and after it, applies only those the engine applied later. So an older control change never overwrites a newer edit, and a newer one always wins.
   - A control change that reaches the engine between a compile and the schedule swap is applied again to the new schedule. Screen edits made in that time are queued after the swap, so they still win.

   *Why:* the graph is what the UI draws and what is saved. A latest-value table loses nothing that matters if the UI is slow, and never makes the audio thread wait. The id says which mapping wrote an entry, so a write from a mapping that no longer exists is ignored. The sequence number shows the same value sent twice.
3. **Learn reads control changes from the engine, too.** The engine records the last control change it received (channel, controller number, and a counter that goes up with each one). The controller reads it while a parameter is armed. *Why:* every host sends MIDI to the engine already. Learn then works the same way in every host, with no second MIDI path.
4. **A mapped control change applies on a fixed 32-frame grid** (decided in [#167](https://github.com/brylie/LoomDSP/issues/167), 2026-11-01; built when a render path or a host feeds control changes at frame offsets). It applies at the first multiple of 32 frames, counted from engine start, at or after its own frame, the grid the Transport's run and reset inputs use ([clock](clock.md#rules), rule 11). The engine splits a block only where a control change lands on the grid. *Why:* the same control change stream then gives the same output at any block size (AGENTS.md rules 2 and 3). Splitting at each change's own frame would also be exact, but a fast sweep would split a block many times. Live MIDI, stamped at frame 0 of the buffer by the desktop host, waits at most 31 frames (0.7 ms at 48 kHz).
5. **A mapping matches one channel and one controller number,** the ones the user moved while learning. *Why:* what the user moved is what works. Most controllers send each knob on one channel, so this needs no setting. Two controllers that send the same controller number on different channels stay apart.
6. **Mapped control changes still reach modules.** A MIDI input module (#30) still sees every control change. *Why:* hiding messages that a mapping uses is a surprise that is hard to debug. If this causes trouble in use, a later change can add a "consume" option.
7. **Mappings are saved with the patch.** A new list in the patch file, empty when missing. The patch format stays at version 1. *Why:* the issue asks for it, and a patch should play the same way when opened again. Per-device mappings that live outside patches are [device mappings](#device-mappings-fr-midi-06).
8. **Real-time rules hold.** The mapping table is built on the control thread and sent with the schedule. The latest-value table is a fixed table of atomics, allocated once when the engine is created. The engine reads the mapping table and stores into the latest-value table with no allocation, lock, or wait (hard rule 1).

### Patch format

```ron
midi: [
    (target: (3, "cutoff"), channel: 0, controller: 74),
    (target: (3, "resonance"), channel: 0, controller: 71),
],
```

- `channel` is 0 to 15, as in MIDI. The inspector shows 1 to 16.
- A mapping whose module or parameter does not exist is a load error, the same as a cable to a port that does not exist.
- Two mappings on the same parameter, or two on the same channel and controller number, are a load error. *Why:* learning never makes them (one hardware control, one parameter), so a file with them was edited by hand by mistake.

## Acceptance criteria

- [x] Pressing **MIDI learn** starts learn mode. Clicking a tile control or an inspector row in learn mode arms its parameter and does not change its value.
- [x] Given an armed parameter, when a control change arrives, then one mapping is made, the parameter takes the control's value, nothing is armed, and learn mode stays on.
- [ ] Outside learn mode, clicking a control and then moving a hardware control makes no mapping.
- [x] Given a mapping, when its control change arrives, then the parameter's base value is `denormalize(value / 127)`: the minimum at 0, the maximum at 127, and the middle of a logarithmic range at 64 (within one step).
- [x] A control change on another channel or controller number does not move the parameter.
- [x] Learning a mapped parameter replaces its mapping. Learning a hardware control that is already mapped moves the mapping to the new parameter, and the status bar names the old one.
- [x] Clicking an armed control disarms it. Escape or a second press of **MIDI learn** ends learn mode with no change.
- [ ] In learn mode, every mapped control in view shows its `CC` label.
- [ ] The tile knob and the inspector show the value set by the hardware within one UI frame.
- [x] A mapped control change still reaches a MIDI input module.
- [x] A patch with mappings saves and loads with the same mappings. A patch without a `midi` list loads with none. A patch with two mappings on one parameter, or on one channel and controller number, fails to load with a clear error.
- [x] Removing a mapping in the inspector stops the control from moving the parameter. Deleting a module removes its mappings.
- [ ] `loom play` with a patch that has mappings responds to the controller with no UI.
- [x] Processing a mapped control change does not allocate (`tests/no_alloc.rs`).

## Device mappings (FR-MIDI-06)

**Status:** Agreed, M2 ([#166](https://github.com/brylie/LoomDSP/issues/166)). The choices marked "Settled" were made with the maintainer on 2026-11-01. Depends on the [front panel](panels.md#the-panel-editor) and on MIDI hot-plugging ([device changes](device-changes.md), built).

> I set up my 8-knob controller once: knob 1 is slot 1, knob 2 is slot 2, and so on. Every patch I open, the knobs move the first eight controls on its front panel. On one patch I want knob 3 on the filter drive, so I pin Drive to slot 3 in the panel editor.

### Terms

| Term | Meaning |
|---|---|
| **Device mapping** | A link from a control change on one MIDI input (channel and controller number) to a **slot**. Saved with the rig, not the patch. |
| **Slot** | A number from 1 to 16. Each patch decides which front-panel control each slot drives. |
| **Device file** | The device mappings of one MIDI input. |

### Slots

**Settled:** a device mapping points at a numbered slot, and each patch fills its slots from its front panel. *Why:* the device file knows the hardware and the patch knows the music, so neither needs the other's details, and a patch works on a rig with another controller.

- A patch's slots are filled from its [front panel](panels.md#the-panel-editor) controls bound to parameters. Displays and labels take no slot.
- **Pinned first:** a front-panel control can be pinned to a slot in the panel editor (widget menu, **Controller slot**: Auto, or 1 to 16). A slot holds one pinned control; pinning a control to a taken slot moves the other one back to Auto.
- **Then reading order:** the other controls take the free slots in the panel's focus order (reading order). Controls beyond the free slots have no slot.
- A widget that binds two parameters (XY pad, range) takes two slots in a row, x then y, or low then high.
- A patch with no front panel has no slots, so device mappings do nothing in it. *Why:* only the player knows which parameters are worth a knob. The status bar says once, when a mapped device is open: `"Keys" has slot mappings, but this patch has no front panel.`
- Slots resolve through the front panel to the inner parameter, as exposing does ([sub-patches](sub-patches.md#flattening)), so a slot reaches a parameter inside a sub-patch.

### Making a device mapping

**Settled:** in learn mode, on front-panel controls.

- In learn mode, front-panel controls in the Panel pane show their slot number next to their mapping label, for example `3 · CC 73`.
- Arming a front-panel control and moving a hardware control maps that control, on the input it came from, to the control's slot in that input's device file. The status bar says: `"Keys" CC 73 → slot 3 (Drive). Works in every patch.`
- A front-panel control with no slot cannot be armed for a device mapping. The status bar says: `Drive has no controller slot. Pin it to a slot in the panel editor.`
- Arming a tile control or an inspector row still makes a patch mapping, as above.
- The one-to-one rule holds per device: a slot has one hardware control, and a hardware control drives one slot. Learning moves or replaces, as for patch mappings.
- Device mappings are rig settings, like preferences. They are written at once and are not in the patch's undo history. *Why:* undo belongs to the patch; a rig change must not be undone by undoing a patch edit.

### Precedence

**Settled:** the patch wins. If a patch mapping and a device mapping use the same channel and controller number on the same input, the patch mapping applies and the device mapping is ignored while that patch plays. The inspector row of the parameter the slot would drive says `Slot 3 is overridden by this patch's CC 73.` *Why:* the patch mapping was made on purpose for this patch.

### Devices

- A device is a MIDI input, identified as in [device changes](device-changes.md#midi-inputs): by its port name, with `(2)` for a second input of the same name. A device that shows several ports has one device file per port.
- When an input connects, its device file is loaded and its mappings apply. When it disconnects, they stop. Replugging restores them. Opening, creating, or switching patches re-resolves slots.
- A patch mapping matches any open input, as today. A device mapping matches only its own input. Each MIDI event already carries its input index ([device changes](device-changes.md#midi-inputs)).

### Storage

**Settled:** one file per device.

- `controllers/<device name>.ron` in the folder that holds `preferences.ron` ([preferences](preferences.md)). Characters a file name cannot hold are replaced with `_`.
- The file follows the preferences write rules: written only on a change, through a temporary file and a rename, and a broken file is renamed `<name>.broken.ron` on the next write ([preferences](preferences.md)).
- A file that cannot be read gives no mappings and a status bar message. The app keeps running.

```ron
Controller(
    device: "Keys",
    slots: [
        (slot: 1, channel: 0, controller: 74),
        (slot: 2, channel: 0, controller: 71),
    ],
)
```

- **Preferences:** the MIDI section lists each device's slot mappings, as `Slot 1 · CC 74 · ch 1`, each with a **×** to remove it ([removing an item](panels.md#removing-an-item)).

### Engine

- The controller resolves device mappings to parameters through the slots, and compiles them with the patch mappings into the one table the engine already reads (rule 1). An entry from a device mapping also carries its input index. *Why:* the engine path, the feedback table, and the real-time rules (rules 2 and 8) stay as they are.
- `loom play` and plugin export do not read device files. *Why:* `loom play` runs one patch on any rig, and a plugin gets its controls from the host's automation. Patch mappings still work in both.

### Acceptance criteria: device mappings

- [ ] A device mapping made on a front-panel control in one patch moves the control in the same slot of another patch (test, with the fake host).
- [ ] Pinned controls take their slots; the rest fill free slots in reading order; an XY pad takes two slots (test).
- [ ] A patch mapping on the same control wins, and the inspector names the override (test).
- [ ] A device mapping moves only from its own input; a second input sending the same control change does nothing (test).
- [ ] Unplugging and replugging the device keeps its mappings (test with the fake host, and a manual check with a hardware controller).
- [ ] A slot reaches a parameter inside a sub-patch through the front panel (test).
- [ ] A patch with no front panel ignores device mappings and shows the status message once (test).
- [ ] A broken device file gives no mappings and a message; the next write keeps it as `.broken.ron` (test).
- [ ] Removing a slot mapping in Preferences stops it at once (test).
- [ ] Processing a device-mapped control change does not allocate (`tests/no_alloc.rs`).

### Mapping modes (FR-MIDI-09)

**Status:** Agreed, M2 ([#165](https://github.com/brylie/LoomDSP/issues/165), 2026-11-01).

> After loading a patch, my filter knob on the controller is at the top but the cutoff is low. In pickup mode, turning the knob does nothing until it passes the cutoff, then it follows, with no jump. My endless encoder moves the delay time in small steps. A fader covers only the middle of the reverb mix, and another runs backwards.

| Mode | What a control change does |
|---|---|
| **Absolute** (default) | Sets the position, as in [Playing](#playing). |
| **Pickup** | Ignored until the control's position crosses the parameter's current position, or comes within one step (1/127) of it. Then it follows, as Absolute, until the parameter changes from somewhere else. |
| **Relative** | Reads the value as a step up or down, and moves the position by 1/128 per step. |

- **The default stays Absolute** (settled). The mode is chosen on the mapping's row in the inspector, in a small menu with Mode, Range, Invert, and Encoding. *Why:* a new learner sees the knob work at once; a player who wants pickup sets it once per mapping, and device mappings keep it in their file.
- **Range and invert** (Absolute and Pickup): `low` and `high` from 0 to 1 of the parameter's position, default 0 and 1. The control's 0 to 127 maps onto that span, through `denormalize` as before. **Invert** swaps the ends.
- **Encoding** (Relative): **Offset 64** (default; 65 and up is up by value − 64, 63 and below is down by 64 − value), **Two's complement** (1 to 63 up, 127 down to 65 is −1 to −63), or **Sign bit** (1 to 63 up, 65 to 127 down by value − 64). A relative move is clamped to the parameter's range.
- **Pickup lives in the engine.** The engine already holds each parameter's base value and orders screen edits and control changes (rule 2), so it knows the current position. A screen edit, a preset change, an undo, or a patch load makes a pickup mapping wait again. *Why in the engine:* the comparison must use the value that plays, in the order rule 2 already defines.
- **Seeing a waiting pickup.** While a pickup mapping waits and its hardware moves, a hollow ghost mark at the hardware's position is drawn on the control, on the tile and in the inspector, for one second after the last move. The engine reports the waiting position in the latest-value table (rule 2). *Why:* the player sees which way to turn, without a mode to enter.
- **Patch format:** optional fields on each mapping, written only when not the default:

  ```ron
  (target: (3, "cutoff"), channel: 0, controller: 74, mode: Pickup, range: (0.2, 0.8), invert: true),
  (target: (5, "time"), channel: 0, controller: 20, mode: Relative(Offset64)),
  ```

  Device files use the same fields. The patch format stays at version 1.

### Acceptance criteria: mapping modes

- [ ] Absolute with range 0.2 to 0.8 maps 0 to position 0.2 and 127 to 0.8; with Invert, 0 maps to 0.8 (engine test at block sizes 1, 77, and 512).
- [ ] Pickup ignores a control above the current position until it comes down past it, then follows; a screen edit makes it wait again (engine test).
- [ ] Relative in each encoding moves by the right number of steps in each direction, and stops at the range ends (engine test).
- [ ] A waiting pickup shows its ghost mark while the hardware moves, and not after it catches (UI snapshot).
- [ ] Modes, ranges, invert, and encodings save and load in patches and device files; a mapping with none writes no new fields (test).
- [ ] Manual check with a hardware knob and an endless encoder.

## Out of scope: device mappings

- Pages or banks of slots (more than 16, or switching sets). Leaning: add when a controller with bank buttons needs it.
- Sharing device files online, or a library of factory files for known controllers.
- Nothing here limits [mapping modes](#mapping-modes-fr-midi-09): they apply to device mappings too.
- Feedback to LED rings or motorised faders (MIDI out).

## Out of scope

- One hardware control driving many parameters. Use the Macro module (MT-UI-02, M2) with modulation.
- A mapping list or matrix. In M1 the labels in learn mode and the inspector rows are enough, because each hardware control has one parameter.
- Learning notes, pitch bend, aftertouch, program change, 14-bit control changes, and NRPN. Pitch bend and aftertouch reach a patch through the MIDI input module (#30).
- Mapping on any channel ("omni").
- Hardware moves in undo. Mapping edits are undoable ([undo.md](undo.md)); moves of the mapped hardware control are not.
- Mapping to modulation depth, transport controls, or app actions.
- MIDI out and feedback to motorised faders or LED rings.

## Open points

None. Answered in review: mappings run in the engine; learning starts by touching the control in learn mode; one hardware control maps to one parameter, with the Macro module for one to many; mapped control changes still reach modules; changes apply at the start of the block; a mapping matches one channel.
