# Panels Specification

How modules, sub-patches, and patches show controls and displays, and how users build them.

- **Kind:** living specification. It changes as panels are built and used.
- **Status:** Draft. Review it before building. The P1 slice below becomes Agreed first.
- **Decision behind it:** [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (modules carry their own panel).
- **Requirements:** FR-PANEL-*, FR-CANVAS-10, FR-CANVAS-11, FR-LIB-18, FR-VIS-01 to 04, FR-ENCAP-03, NFR-PERF-04, NFR-UX-01.
- **Builds on:** [Telemetry](telemetry.md) for live data, [ADR-0010](../adr/0010-own-patch-canvas.md) for the canvas.

## One panel model, three places

| Place | Who builds the panel | Where it shows |
|---|---|---|
| Built-in module | The module author, as data in the module's descriptor | On the module's tile |
| Sub-patch | The user, in the panel editor | On the sub-patch's tile in the parent patch, and as the sub-patch's front panel |
| Patch | The user, in the same panel editor | As the patch's front panel: performance mode, and later the plugin GUI and standalone app |

The rule that ties them together: **a sub-patch's front panel is its tile.** When you finish a sub-patch, its tile in the parent looks and works like a built-in module. *Why:* abstraction should feel the same at every level. Hiding wiring behind good controls is the same job whether the result is one filter or a whole instrument.

## Widgets

Every panel uses the same widget set.

**Controls** (bound to a parameter):

| Widget | Use |
|---|---|
| Knob | Continuous values. Shows modulation depth as an arc (FR-MOD-04). |
| Slider, horizontal or vertical | Continuous values where a long throw helps. |
| Toggle | On or off. |
| Radio buttons | A few choices shown at once, such as filter slopes 6, 12, 18, 24 dB. |
| Selector | Many choices, as a menu. |
| Button | A momentary action or trigger. |
| XY pad | Two parameters at once. |

**Displays** (bound to live data or module data):

| Widget | Shows |
|---|---|
| Light | One value as brightness, or on and off. |
| Meter | A level, from a port probe or a display value. |
| Scope | A signal over time, from a port probe. |
| Step grid | A pattern, and the playing step. Editable when bound to module data. |
| Graph | A shape: envelope, curve, filter response. Editable when bound to module data. |
| Label | Static text. |

*Why one set:* users learn each control once. A new widget is added to the set, not drawn by one module, so every module and every panel can use it.

## Bindings

- **Control to parameter.** A control takes its range, unit, and mapping (linear, logarithmic, whole numbers) from the parameter's `ParamSpec`. *Why:* the slider mapping work in the inspector (logarithmic Hz, snapping integers) then applies everywhere for free.
- **Display to port.** A scope or meter reads a port probe through the telemetry queues.
- **Display to module value.** A light or step position reads a **display value**: a small, fixed set of numbers a module publishes once per block. *Why:* a step sequencer's position is internal state, not a port, and it should not need a cable to be seen.
- **Editor to module data.** A step grid or graph that edits a pattern writes module data (OQ-17), with undo.
- **One parameter, many controls.** A parameter can have a knob on its tile and another on the front panel (FR-PANEL-03). Both move together.

## Layout

- Panels are laid out on the same snap grid as the canvas. A tile's size is its panel's size, in grid units.
- Ports stay on the tile's left and right edges, outside the panel area. *Why:* cables must stay easy to read, whatever the panel holds.
- A tile can have a compact and an expanded size. The scope module, for example, can be resized to show more detail.
- A sub-patch panel can be placed inside its parent's panel (FR-PANEL-08), as one more widget.

## The panel editor

Used for sub-patches and patches alike.

- Open it from a patch or a sub-patch. The same tools appear in both cases.
- Place widgets on the grid; snap, align, group, resize (FR-PANEL-05).
- Bind a control by dragging a parameter from the inspector or from a module onto it. Binding a control inside a sub-patch exposes that parameter as the sub-patch's own (FR-ENCAP-03).
- Leaving the editor shows the result at once: on the tile in the parent, or as the front panel.

Built-in module panels are written as data in code, in the same format the editor saves.

## Interaction on the canvas

| Gesture | On a control | On a display or empty panel area | On the tile header |
|---|---|---|---|
| Drag | Changes the value | Moves the tile | Moves the tile |
| Click | Toggle, radio, button: acts | Selects the tile | Selects the tile |
| Double-click | Resets to the default | Opens the larger editor, if the module has one | Opens a sub-patch |
| Shift-drag | Fine adjustment | | |

- Every control is reachable by keyboard, with visible focus (NFR-UX-01).
- Editing a control on a tile also selects that module, so the inspector follows.

## Level of detail

| Zoom | Controls | Displays |
|---|---|---|
| Close | Full widget with label and value | Full detail |
| Middle | Widget shape only, no text | Simplified (a scope becomes its level) |
| Far | Not drawn; the tile is a plain shape | Not drawn |

*Why:* large patches stay readable and fast (FR-CANVAS-11). At far zoom you read the structure, not the values.

## Redraw

**Draw a frame when something visible changes.**

- Graph edits, camera moves, hover, and control drags redraw.
- A display redraws when its data changes and the tile is on screen at a zoom where the display is drawn.
- A scope on screen changes every frame, so it animates every frame. A step light changes a few times a second, so it redraws a few times a second.
- Nothing on screen changing means no redraw.

*Why:* GPU time is not the limit (the canvas holds 120 fps with 300 modules). Battery and heat are, and a laptop on stage often runs on battery. Drawing only what changed keeps the cost in line with what the user is looking at.

Measure on the reference laptop: idle CPU and GPU with nothing changing, and CPU, GPU, and power with four scopes on screen.

## Cables do not animate

Cables do not pulse or change brightness with the signal they carry. Signal activity (FR-VIS-01) is shown on ports, as a steady state such as "carrying signal" or "silent", updated a few times a second. *Why:* when everything moves, nothing stands out, and constant motion tires the eyes. Hover focus already brings the cables you care about to the front.

## The inspector

The inspector stays, as the place for:

- every parameter, including those not on the tile;
- typing exact values;
- modulation ranges, MIDI learn, and other settings that do not fit on a control.

## Scope module

The first display module.

- One audio input, and an optional pass-through output so it can sit inline.
- A resizable tile with a scope display. Parameters: time window and trigger level.
- **Add scope:** a one-step gesture on a cable or an output that places a scope module next to it, already connected. *Why:* a probe should be as quick as a glance, but the result is an ordinary module you can keep, move into a sub-patch, or put on a front panel.

## P1 slice

What P1 builds. The rest of this spec is P2.

### Acceptance criteria

- [ ] Every built-in module declares a panel in its descriptor. A test checks that every control binds to a parameter the module has.
- [ ] Dragging a knob on a tile changes the parameter. The inspector shows the same value, and saving the patch keeps it.
- [ ] Dragging a control never moves the tile. Dragging the tile header always does.
- [ ] Every control on a tile is reachable by keyboard: Tab moves focus, arrow keys change the value.
- [ ] A scope module fed by a 440 Hz oscillator shows a stable trace.
- [ ] "Add scope" on a cable places a connected scope module next to it. Undo removes it.
- [ ] Cable opacity never depends on the signal. A test checks that the cable geometry is the same with and without telemetry.
- [ ] With no displays on screen and no input, the app does not redraw. Check: the frame counter stays still for 10 seconds.
- [ ] Zoomed far out, controls and displays are not drawn.
- [ ] Reference laptop, full window: 60 fps with four scopes on screen. Record CPU and GPU in the benchmark doc.

### Out of scope for P1

- The panel editor, sub-patch panels, and front panels (P2, FR-PANEL-*).
- Step grid and graph editing on the tile. They need module data (OQ-17, P2).
- XY pad, themes, custom skins (P2).
- Restyling built-in panels.
- Docked views at the window edge. Displays are modules.

## Open points

- Mouse wheel over a control: change the value, or keep zooming the canvas? Leaning: the wheel always zooms; a modifier changes the value.
- Can users restyle a built-in module's panel, or only their own sub-patches?
- How third-party modules add display kinds through the SDK.
- The exact format of display values: how many per module, and whether they share the meter queue or use a latest-value buffer.
- How a sub-patch tile looks before its panel has been designed. Leaning: its exposed parameters as knobs, in order.
