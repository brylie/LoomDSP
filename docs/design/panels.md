# Panels Specification

How modules, sub-patches, and patches show controls and displays, and how users build them.

- **Kind:** living specification. It changes as panels are built and used.
- **Status:** Draft, except [Module panels on tiles](#module-panels-on-tiles-p1), which is Agreed.
- **Decision behind it:** [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (modules carry their own panel).
- **Requirements:** FR-PANEL-*, FR-CANVAS-10, FR-CANVAS-11, FR-LIB-18, FR-VIS-01 to 04, FR-ENCAP-03, NFR-PERF-04, NFR-UX-01.
- **Builds on:** [Telemetry](telemetry.md) for live data, [ADR-0010](../adr/0010-own-patch-canvas.md) for the canvas, [parameter mapping](modulation.md#parameter-mapping) for control values.
- **Issues:** [#7](https://github.com/brylie/LoomDSP/issues/7) (umbrella), [#67](https://github.com/brylie/LoomDSP/issues/67) (panel declarations), [#68](https://github.com/brylie/LoomDSP/issues/68) (controls on tiles), [#69](https://github.com/brylie/LoomDSP/issues/69) (display values).

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

- **Control to parameter.** A control takes its range, unit, and mapping (linear, logarithmic, whole numbers) from the parameter's `ParamSpec`, through `ParamSpec::normalize` and `denormalize` ([parameter mapping](modulation.md#parameter-mapping), Agreed). *Why:* the slider mapping work in the inspector (logarithmic Hz, snapping integers) then applies everywhere for free.
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

## Module panels on tiles (P1)

**Status:** Agreed ([#7](https://github.com/brylie/LoomDSP/issues/7)). This section turns the general rules above into what P1 builds for built-in modules. Where the two differ, this section wins for P1.

### Panel declaration ([#67](https://github.com/brylie/LoomDSP/issues/67))

**Status:** Implemented. Code: `Panel`, `Widget`, and `ModuleDescriptor::check_panel` in `loom-core/src/descriptor.rs`.

- A `ModuleDescriptor` has a `panel`: its size in grid units (width and height), and a list of widgets. A module with nothing on its tile has an empty panel.
- Each widget has a kind, a grid position inside the panel, and a binding by name: a parameter for controls, a display value for lights. *Why names:* parameter names are stable IDs (AGENTS.md rule 5), and the P2 panel editor will save bindings the same way.
- P1 widgets, each 3 grid units wide:

| Widget | Size (grid units) | Binds to | Behaviour |
|---|---|---|---|
| Knob | 3 × 3 | Any parameter | A dial with its label below. Uses the parameter's mapping (`ParamSpec::normalize`). |
| Toggle | 3 × 2 | A whole-number parameter from 0 to 1 | Off or on, with its label below. |
| Radio buttons | 3 × (options + 1) | A whole-number parameter with exactly one value per option | One row per option, top option = minimum value. Label above. |
| Light | 3 × 2 | A display value | Brightness is the value clamped to 0 to 1. Label below. |

- **The registry refuses a panel that does not fit its module:** a binding to a parameter or display value the module does not have, a toggle or radio on a parameter that does not match it, a widget outside the panel, or two widgets that overlap. *Why:* a third-party module gets the same checks as a built-in one, at registration, not as a broken tile later.
- **Built-in panels:** a knob for every parameter except the noise seed and MIDI Note's channel and bend range, which stay in the inspector. MIDI Note has a light for its gate, and ADSR a light for its level. Toggle and radio buttons are built and tested, but no P1 module uses them yet.

### Display values ([#69](https://github.com/brylie/LoomDSP/issues/69))

**Status:** Implemented. Code: `loom-core/src/display.rs`, `Module::displays`, `Controller::display`.

- A descriptor lists its display values by name, at most 4. A module writes them in `Module::displays`, which the engine calls once after each block for every module that has any.
- They travel in a **latest-value table**: 4 atomic floats per module slot, allocated once when the engine is created. The engine stores, the UI loads whenever it draws. Nothing waits, nothing allocates, and the table can never fill up. *Why not the meter queue:* only the latest value matters for a light or a step position, as [telemetry.md](telemetry.md) notes for this kind of data.
- When a module is added to a slot, its values are set to 0 first, so a new module never shows the values of the one that was there before.
- The UI reads them by module ID and index through the `Controller`.
- Display values never change the audio. *Why:* determinism (AGENTS.md rule 3).

### Tile layout ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `ModuleView::width`, `widget_bounds`, and `tile_size` in `loom-canvas/src/scene.rs`.

- **The panel sits between the port columns.** Each side has a port label column 4 grid units wide. A label longer than its column is cut with "…", and hovering the port shows the full name.
- Tile width is 4 + panel width + 4 grid units, and at least 11 (today's width), so titles still fit. Tile height is the header plus the taller of the port rows and the panel.
- A tile with an empty panel keeps the old label layout: each side gets half the tile. *Why:* nothing sits between the columns, so cutting labels would hide text for no reason.
- *Why:* tiles stay compact, and ports and cables keep their place at the tile edges.

### Interaction ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `loom-canvas/src/widget.rs`, `controls.rs`.

| Gesture | Knob | Toggle, radio | Light, empty panel area | Header |
|---|---|---|---|---|
| Drag | Up raises, down lowers. The full range is 200 screen pixels at any zoom. | Moves the tile | Moves the tile | Moves the tile |
| Shift-drag | 10 times finer | | | |
| Click | Selects the module | Toggles, or picks the option | Selects the module | Selects the module |
| Double-click | Resets to the default | Resets to the default | | |
| Mouse wheel | Zooms the canvas | Zooms the canvas | Zooms the canvas | Zooms the canvas |

- Knob drags work in the control's 0 to 1 range, through the parameter's mapping, so a logarithmic cutoff moves by equal intervals.
- Editing a control selects its module, so the inspector follows.
- **Keyboard:** with a module selected, Tab moves focus to its next control and Shift-Tab to the previous one. Up and Right raise the value by 1% of the range, Down and Left lower it; with Shift, 0.1%. Whole-number parameters step by 1. Space or Enter toggles or picks the next option. Escape clears the focus. The focused control has a visible ring (NFR-UX-01).
- While a knob is dragged or focused, its label shows the value with its unit.
- *Why the wheel always zooms:* scrolling over a tile to zoom must never change the sound by accident.

### Level of detail ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `detail` in `loom-canvas/src/widget.rs`.

The canvas already hides port labels and titles below a size on screen. Panels use the same two thresholds:

| Zoom | Controls and lights |
|---|---|
| Port labels shown | Full widgets, with labels and values |
| Titles shown, port labels hidden | Shapes only: knob and value arc, toggle and light states. No text. |
| Titles hidden | Not drawn |

### Redraw ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `ControlGeometry` in `loom-canvas/src/controls.rs`, `loom-app/src/lights.rs`.

- **Control values have their own GPU buffer.** Changing a value rewrites only that buffer, never the tiles, ports, or cables. *Why:* ADR-0010's rule that GPU data for the patch is rebuilt only when the patch changes.
- Lights read the display table on a timer of 30 Hz, but only while a module with lights is on screen at a zoom where lights are drawn. A frame is drawn only when a shown brightness changes by at least 1/256.
- The timer runs on its own thread and sends the UI a message only when a light changes. *Why:* in Iced every message redraws the window, so a plain 30 Hz timer message would draw 30 frames a second even when nothing moves. The canvas reports which modules with lights are on screen, so the app watches only those.
- Nothing visible changing means no frame.

### Acceptance criteria: module panels

#67, panel declarations:

- [x] Every built-in module declares a panel. A test checks that every widget binds to something its module has, fits inside the panel, and overlaps no other widget.
- [x] The registry refuses each kind of bad panel listed above, with a message that names the widget.

#69, display values:

- [x] A module's display values reach the `Controller` after the block that wrote them.
- [x] A module added to a reused slot reads 0 until it writes, never the old module's values.
- [x] Display values allocate nothing on the audio thread (`no_alloc` test).
- [x] Output audio is bit-identical with and without display values being read.

#68, controls on tiles:

- [ ] Dragging a knob on a tile changes the parameter. The inspector shows the same value, and saving the patch keeps it.
- [ ] Dragging a control never moves the tile. Dragging the header always does.
- [x] Shift-drag is 10 times finer. Double-click resets to the default.
- [x] A logarithmic knob moves by equal ratios: halfway up a 20 Hz to 20 kHz cutoff knob is about 632 Hz.
- [x] Every control is reachable by keyboard: Tab, Shift-Tab, arrows, Space, Escape, with a visible focus ring.
- [x] Changing a control value does not rebuild the GPU data for tiles, ports, or cables.
- [ ] The MIDI Note gate light is lit while a key is held. The ADSR light follows its level.
- [x] Zoomed far out, controls and lights are not drawn. At middle zoom, they have no text.
- [x] Lights add no frames while their brightness stays the same: the light watcher sends nothing until a shown level changes, and runs only while lights are on screen.
- [ ] Manual, on the reference laptop: knobs, the toggle, and lights look right at close and middle zoom, and a cut port label shows in full on hover.

The whole app still redraws twice a second for the stats bar's audio numbers. "No redraw when idle" for the whole app stays a P1 criterion below.

## P1 slice

What P1 builds. The rest of this spec is P2.

### Acceptance criteria

- [ ] Module panels on tiles: see [their acceptance criteria](#acceptance-criteria-module-panels) above.
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

- Can users restyle a built-in module's panel, or only their own sub-patches?
- How third-party modules add display kinds through the SDK.
- The exact format of display values: how many per module, and whether they share the meter queue or use a latest-value buffer.
- How a sub-patch tile looks before its panel has been designed. Leaning: its exposed parameters as knobs, in order.
