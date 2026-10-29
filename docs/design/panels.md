# Panels Specification

How modules, sub-patches, and patches show controls and displays, and how users build them.

- **Kind:** living specification. It changes as panels are built and used.
- **Status:** Draft, except [Module panels on tiles](#module-panels-on-tiles-p1) and [Level meters and audio load](#level-meters-and-audio-load-p1), which are Implemented. [Labels and hover text](#labels-and-hover-text) is Implemented. [Scope module](#scope-module) is Implemented, with its trace ([#62](https://github.com/brylie/LoomDSP/issues/62)). [Default panels](#default-panels) and [Exposed controls in a sub-patch](#exposed-controls-in-a-sub-patch) are Draft.
- **Decision behind it:** [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (modules carry their own panel), [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) (one design language, and panels derived from the declaration).
- **Requirements:** FR-PANEL-*, FR-CANVAS-10, FR-CANVAS-11, FR-LIB-18, FR-VIS-01 to 04, FR-ENCAP-03, NFR-PERF-04, NFR-UX-01.
- **Builds on:** [Telemetry](telemetry.md) for live data, [ADR-0010](../adr/0010-own-patch-canvas.md) for the canvas, [parameter mapping](modulation.md#parameter-mapping) for control values.
- **Issues:** [#7](https://github.com/brylie/LoomDSP/issues/7) (umbrella), [#67](https://github.com/brylie/LoomDSP/issues/67) (panel declarations), [#68](https://github.com/brylie/LoomDSP/issues/68) (controls on tiles), [#69](https://github.com/brylie/LoomDSP/issues/69) (display values), [#9](https://github.com/brylie/LoomDSP/issues/9) (level meter, audio load).

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

The widgets after P1, with their bindings, modulation display, keyboard use, and the catalogue entries that need them, are planned in [Panel widgets](panel-widgets.md) (Draft, [ADR-0024](../adr/0024-widgets-follow-the-kind-of-value.md)). It names the slider "fader", the selector "select list", and the graph "curve", and adds a number, a readout, an XY scope, and a waveform.

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
- modulation ranges, MIDI mappings, and other settings that do not fit on a control.

MIDI learn grew past the inspector. ADR-0013 places it there, and that ADR is frozen, so the change is recorded here: learn mode arms controls on tiles as well as inspector rows, and the inspector shows each parameter's mapping and removes it ([MIDI learn](midi-learn.md#seeing-and-removing-mappings)). *Why:* touching the control you want, then the hardware, is the order the user thinks in.

### Removing an item

**Status:** Agreed.

Lists in the inspector, such as a parameter's MIDI mapping or its modulation assignments, share one remove control.

- A small **×** button at the end of the item's row removes that item. Hovering it shows what it removes, for example "Remove MIDI mapping". *Why:* one small, familiar mark in the same place on every row is quick to find and takes little room in a narrow inspector. The hover text says exactly what goes, because a bare × does not.
- It acts at once, with no confirmation. *Why:* removing a mapping or an assignment is quick to redo, and undo ([#12](https://github.com/brylie/LoomDSP/issues/12)) will bring it back.
- Used by [MIDI learn](midi-learn.md#seeing-and-removing-mappings) and by modulation's [remove button](modulation.md#seeing-assignments).

## Scope module

**Status:** Implemented ([#60](https://github.com/brylie/LoomDSP/issues/60)). Drawing the trace is [#62](https://github.com/brylie/LoomDSP/issues/62), and a resizable tile is [#61](https://github.com/brylie/LoomDSP/issues/61). Catalogue: MT-VIEW-01 Probe, scope mode.

The first display module that shows a signal over time.

### Module

- **`loom.display.scope`**, category Display. One audio input `in` and one output `out` that passes `in` through, sample for sample, as the level meter does. *Why a pass-through output:* probes read outputs, so the app probes the scope's own `out`. The scope can sit inline in a signal path or hang off an output, and an unconnected scope needs no special case.
- Parameters:

| Name | Label | Range | Default | Notes |
|---|---|---|---|---|
| `window` | Window | 0.001 to 1 s | 0.02 s | Logarithmic. The time shown across the display. One second is the history `ScopeTrace` keeps. |
| `auto` | Auto | 0 or 1 | 1 | Whole number. On: the trigger level is halfway between the lowest and highest sample, as in [telemetry](telemetry.md). |
| `level` | Level | -1 to 1 | 0 | The trigger level while `auto` is off. |

- Its panel: a knob for `window`, a toggle for `auto`, and a knob for `level`, and below them the scope widget (9 by 5 grid units, bound to `out`, no caption), added in #62. Code: `SCOPE` in `loom-modules/src/display.rs`, `Trigger` in `loom-app/src/live.rs`, and `loom-app/src/probe_menu.rs`.
- The parameters never change the audio. They tell the app how to draw the trace.
- *Why an auto toggle:* most signals trigger well at their midpoint, including envelopes and pitch with an offset, which never cross 0. A fixed level is for picking one edge of a complex wave. A toggle keeps both without a knob that means two things.

### Trigger

- `ScopeTrace` takes the level from the module: automatic, or the fixed `level`. The rest of the trigger is unchanged: a rising edge, with hysteresis of a tenth of the history's swing, found to a fraction of a sample. The hysteresis never reaches below the lowest sample, so a fixed level near the bottom of the signal still triggers.
- If no edge crosses a fixed level, the trace runs free, as for a flat signal.

### Add scope and Add meter

A one-step gesture on a cable or an output places a scope or a level meter next to it, already connected. *Why:* a probe should be as quick as a glance, but the result is an ordinary module you can keep, move into a sub-patch, or put on a front panel.

- **Right-click** an output port or a cable to open a small menu with **Add scope** and **Add meter**. A right-drag still pans the canvas: the menu opens only if the pointer moved no more than 4 screen pixels between press and release, the same allowance as a double-click. Right-clicking anything else opens no menu.
- The menu closes when an item is chosen, on Escape, or on a click outside it. While it is open, the canvas takes no pointer or key input, as with the module browser, so it cannot pan or zoom away from the menu.
- **What it connects:** a new cable from the output to the new module's `in`. On a cable, the output is the cable's source. The cable stays as it is. *Why:* the sound path does not change, and outputs can feed many inputs.
- **Where it goes:** with its top-left corner 3 grid units right of the output port, or of the point clicked on the cable, and its `in` port as close to the same height as the grid allows. If that overlaps a tile, it moves down in steps of 2 grid units until it does not.
- The new module is selected. Adding it and its cable is one undo step. If the output is gone when an item is chosen, nothing is added and the status bar says why.
- While the menu is open, the clipboard shortcuts (copy, cut, duplicate) do nothing.
- The control channel offers the same action, so it can be checked in the running app.

### Acceptance criteria: scope module

- [x] The scope module passes its input through, bit for bit. It has a reference test and a block-size check.
- [x] Its parameters are `window`, `auto`, and `level`, with the ranges above, and its panel has a knob, a toggle, and a knob for them.
- [x] `ScopeTrace` with a fixed level triggers on that level, and runs free when nothing crosses it. With `auto` on it behaves as before.
- [x] Right-clicking an output or a cable opens the menu. A right-drag pans and opens no menu. Right-clicking a tile, an input, or empty canvas opens none.
- [x] Add scope and Add meter, on an output and on a cable, place a module whose `in` is connected to that output, next to it and overlapping no tile. The cable that was clicked stays. The new module is selected.
- [x] Undo removes the new module and its cable in one step.
- [x] Escape and a click outside close the menu and add nothing. While it is open, keys do not reach the canvas.

## Module panels on tiles (P1)

**Status:** Agreed ([#7](https://github.com/brylie/LoomDSP/issues/7)). This section turns the general rules above into what P1 builds for built-in modules. Where the two differ, this section wins for P1.

### Panel declaration ([#67](https://github.com/brylie/LoomDSP/issues/67))

**Status:** Implemented. Code: `Panel`, `Widget`, and `ModuleDescriptor::check_panel` in `loom-core/src/descriptor.rs`.

- A `ModuleDescriptor` has a `panel`: its size in grid units (width and height), and a list of widgets. A module with nothing on its tile has an empty panel.
- Each widget has a kind, a grid position inside the panel, and a binding by name: a parameter for controls, a display value for lights. *Why names:* parameter names are stable IDs (AGENTS.md rule 5), and the P2 panel editor will save bindings the same way.
- P1 widgets. Most are 3 grid units wide. The button is wider, so a performance control can be large. Radio buttons are wider too, so their choice names fit ([#394](https://github.com/brylie/LoomDSP/issues/394)).

| Widget | Size (grid units) | Binds to | Behaviour |
|---|---|---|---|
| Knob | 3 × 3 | Any parameter | A dial with its label below. Uses the parameter's mapping (`ParamSpec::normalize`). |
| Toggle | 3 × 2 | A whole-number parameter from 0 to 1 | Off or on, with its label below. |
| Radio buttons | 6 × (choices + 1) | A whole-number parameter with [choice names](#choice-names) | One row per choice name, top = minimum value. Label above. |
| Button | 6 × 3 | A whole-number parameter from 0 to 1, and a different whole-number `mode` parameter from 0 to 1 | A large button. Its fill shows the bound parameter. Gestures are under [Interaction](#interaction-68). |
| Light | 3 × 2 | A display value | Brightness is the value clamped to 0 to 1. Label below. |

- **The registry refuses a panel that does not fit its module:** a binding to a parameter or display value the module does not have, a toggle, radio, or button on a parameter that does not match it (radio buttons need choice names), a button whose mode is missing, out of range, or the same parameter, a widget outside the panel, or two widgets that overlap. *Why:* a third-party module gets the same checks as a built-in one, at registration, not as a broken tile later.
- **Built-in panels:** a knob for every parameter except the noise seed, MIDI Note's channel and bend range, and Button's `mode`, which stay in the inspector. Button's `on` is the button, not a knob. MIDI Note has a light for its gate, and ADSR a light for its level. Clock Divide and Phase Trigger have a [trigger light](#lights-for-one-sample-triggers) on their output.

### Display values ([#69](https://github.com/brylie/LoomDSP/issues/69))

**Status:** Implemented. Code: `loom-core/src/display.rs`, `Module::displays`, `Controller::display`.

- A descriptor lists its display values as `DisplaySpec`s ([text](port-and-parameter-text.md#display-values)), at most 4. A module writes them in `Module::displays`, which the engine calls once after each block for every module that has any.
- They travel in a **latest-value table**: 4 atomic floats per module slot, allocated once when the engine is created. The engine stores, the UI loads whenever it draws. Nothing waits, nothing allocates, and the table can never fill up. *Why not the meter queue:* only the latest value matters for a light or a step position, as [telemetry.md](telemetry.md) notes for this kind of data.
- When a module is added to a slot, its values are set to 0 first, so a new module never shows the values of the one that was there before.
- The UI reads them by module ID and index through the `Controller`.
- Display values never change the audio. *Why:* determinism (AGENTS.md rule 3).

### Tile layout ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `ModuleView::width`, `widget_bounds`, and `tile_size` in `loom-canvas/src/scene.rs`.

- **The panel sits between the port columns.** Each side has a port label column 4 grid units wide. A label longer than its column is cut with "…", and the port's [hover text](#labels-and-hover-text) shows it in full.
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
- **Button** ([#75](https://github.com/brylie/LoomDSP/issues/75)). The canvas reports a press, a release, or an arrow. It does not read `mode`, and it does not move the tile. Drag the tile from its header. The editor turns the report into a value ([clock.md](clock.md#button-details)). In toggle mode a press flips the bound parameter, and a release does nothing. A double-click is two presses, not a reset. In momentary mode the parameter is 1 while the pointer or Space or Enter is held, and 0 on release, including when the pointer has left the button, when another mouse button is pressed, or when the window loses focus. Releasing a mouse button other than the left one does not end a hold the left button is still keeping. Space and Enter each keep their own hold. The gate stays high while the pointer or either key is still down. Arrow keys set 0 or 1 in toggle mode and do nothing in momentary mode. A repeated key event does nothing.
- **Keyboard:** with a module selected, Tab moves focus to its next control and Shift-Tab to the previous one. Up and Right raise the value by 1% of the range, Down and Left lower it; with Shift, 0.1%. Whole-number parameters step by 1. Space or Enter toggles or picks the next option. When a control is focused, Space and Enter act on it and do not toggle Run. Space toggles Run only when no panel control is focused ([clock.md](clock.md#controls-live-on-the-transport-module)). Escape clears the focus. A control focused with Tab has a visible ring (NFR-UX-01). The ring goes around the control itself, not its caption, so the value shown there stays clear. A click or drag also focuses the control, so the arrow keys work after it, but shows no ring, as `:focus-visible` works on the web. *Why:* after a mouse gesture the pointer already shows where the user is, and a ring around the whole cell cut through the value text ([#92](https://github.com/brylie/LoomDSP/issues/92)).
- While a knob is dragged or focused, its label shows the value with its unit.
- *Why the wheel always zooms:* scrolling over a tile to zoom must never change the sound by accident.

### Level of detail ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `detail` in `loom-canvas/src/widget.rs`, `tile_text.rs`.

The canvas already hides port labels and titles below a size on screen. Panels use the same two thresholds:

| Zoom | Controls and lights | Text |
|---|---|---|
| Port labels shown | Full widgets, with labels and values | All text |
| Titles shown, port labels hidden | Shapes only: knob and value arc, toggle, button, and light states | Titles. Port labels and captions are bars. |
| Titles hidden | Not drawn | Titles and port labels are bars |

- **Text too small to read leaves a faint bar.** A rounded bar, as wide as the text and half as tall as its letters, sits where the text would be. *Why:* a tile that loses all its text looks empty from far out, but a real panel seen from across a room still shows where things are written ([#92](https://github.com/brylie/LoomDSP/issues/92)). Text that small cannot be read anyway, and drawing it would mean thousands of glyph layouts in a large patch.
- Bars are shapes in the scene's instanced buffer. They are built when the scene changes, never per frame, so they cost almost nothing at any zoom.
- Text, bars, and hover on port labels all take their places from one list (`tile_text.rs`), so a bar always sits exactly where its text would be.

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

- [x] Dragging a knob on a tile changes the parameter. The inspector shows the same value, and saving the patch keeps it. (Checked by hand in [#92](https://github.com/brylie/LoomDSP/issues/92).)
- [x] Dragging a control never moves the tile. Dragging the header always does. (Checked by hand in [#92](https://github.com/brylie/LoomDSP/issues/92).)
- [x] Shift-drag is 10 times finer. Double-click resets to the default.
- [x] A logarithmic knob moves by equal ratios: halfway up a 20 Hz to 20 kHz cutoff knob is about 632 Hz.
- [x] Every control is reachable by keyboard: Tab, Shift-Tab, arrows, Space, Escape, with a visible focus ring. The ring shows for Tab focus only, and stays clear of the caption.
- [x] Changing a control value does not rebuild the GPU data for tiles, ports, or cables.
- [x] The MIDI Note gate light is lit while a key is held. The ADSR light follows its level. The tests in `loom-modules/tests/panels.rs` play a note through `subtractive.ron` and read the display values the lights draw, at block sizes 1, 77, and 512.
- [x] Zoomed far out, controls and lights are not drawn. At middle zoom, they have no text.
- [x] Text hidden by zoom is replaced by a bar at the place and width of its text: port labels and captions at middle zoom, titles and port labels far out.
- [ ] Manual, on the reference laptop: with `--generate 300 --bench`, the frame rate far out with bars matches the rate without them.
- [x] Lights add no frames while their brightness stays the same: the light watcher sends nothing until a shown level changes, and runs only while lights are on screen.
- [ ] Manual, on the reference laptop: knobs, the toggle, and lights look right at close and middle zoom, and a cut port label shows in full on hover.

The whole app redraws only when something visible changes ([#90](https://github.com/brylie/LoomDSP/issues/90)): see [Audio load and dropouts](#audio-load-and-dropouts-in-the-stats-bar) for the stats bar.

## Level meters and audio load (P1)

**Status:** Implemented ([#9](https://github.com/brylie/LoomDSP/issues/9)). Code: `WidgetKind::Meter`, `loom-modules/src/display.rs`, `loom-app/src/monitor.rs`, `live.rs`, `alerts.rs`. Requirements: FR-LIB-17, FR-VIS-03 (the level meter module, brought forward from P2), FR-PERF-04.

### Meter widget

- A fifth P1 widget, 3 × 6 grid units. It binds to one of its module's **outputs** by name. The registry refuses a meter bound to an output the module does not have.
- It shows the output's level from -60 dB (empty) to 0 dB (full), bottom to top: a bar for the RMS level and a thin line for the peak. Both rise at once and fall by the whole range in about a second. The bar is green below -12 dB, amber up to -3 dB, and red above. Its label is below it.
- A meter is a display: pressing or dragging it selects or moves the tile, as with a light.
- *Why levels fall over a second:* a short peak stays visible long enough to see, and the bar does not flicker.

### Level meter module

- **`loom.display.meter`**, category Display. One audio input `in` and one output `out` that passes `in` through, sample for sample. No parameters. Its panel is one meter bound to `out`.
- Mono. For stereo, use two.
- *Why a pass-through output:* probes read outputs, and the module can sit inline in a signal path or hang off an output, without changing the sound.

### How levels reach a meter

- While a meter is on screen at a zoom where it is drawn, the app keeps a `Meter` probe on its output ([telemetry](telemetry.md)). When the meter leaves the screen, the probe is removed at the next commit. *Why:* a probe costs engine work, and a meter nobody sees needs none.
- Telemetry is drained on a background thread every 33 ms, not on the UI thread. The thread keeps each probe's peak and RMS levels and sends the UI a message only when a level changes by at least 1 of 256 steps. *Why:* as for lights, every message redraws the window, so a silent patch with meters on screen must send none.
- Probes stay view state: not saved, and they never change the sound.

### Audio load and dropouts in the stats bar

- The stats bar always shows the audio load (the latest callback as a share of its time budget, and the highest so far) and the dropout count: callbacks that overran their buffer, plus errors reported by the audio device.
- An overload notice from the device that comes before the stream's first callback is not a dropout. No audio was rendered yet, so none was lost. macOS sends one as a stream starts with a very small buffer (32 frames), while the device switches to it. *Why:* counting it turned the count red each time a performer chose a small buffer, for a glitch that never happened. A count that cries wolf gets ignored. (Found on BlackHole 2ch, 2026-10-24, [#26](https://github.com/brylie/LoomDSP/issues/26).)
- Load is shown normally below 70%, in amber from 70%, and in red from 90%. The colour follows the highest load since the last tick, and the "max" is the highest in the last 10 seconds ([P1 additions](#audio-load-colour)).
- The dropout count is red for 10 seconds after it grows.
- **Redraw.** A thread reads the audio stats four times a second and wakes the UI only when the bar would change: the load or its maximum to the shown 1%, the dropout count, the end of the red time, or the telemetry drops. The load number changes at most twice a second. *Why:* in Iced every message redraws the window, so a timer that refreshed the bar drew two frames a second in an idle patch ([#90](https://github.com/brylie/LoomDSP/issues/90)).
- *Why:* a performer needs a warning before a problem is audible (FR-PERF-04), and must notice when one just happened. A count that stayed red forever would soon be ignored.

### Acceptance criteria: level meters and audio load

- [x] The registry refuses a meter bound to an output the module does not have.
- [x] The meter module passes its input through, bit for bit. It has a reference test and a block-size check.
- [x] A meter on screen gets a probe on its output. When it leaves the screen, or the canvas zooms far out, the probe is removed.
- [x] Levels map -60 dB and below to empty and 0 dB to full. They rise at once and fall by the full range in a second. The peak line is never below the RMS bar.
- [x] The telemetry thread sends nothing while the shown levels stay the same.
- [x] Load below 70% is normal, from 70% amber, from 90% red. The dropout count is red for 10 seconds after it grows.
- [x] An overload notice before a stream's first callback is not counted as a dropout (`an_overload_before_the_first_callback_is_not_a_stream_error`). Checked on macOS: BlackHole 2ch at 32 frames shows `dropouts 0`.
- [x] A meter module after the VCA in `subtractive.ron` rises when a note plays and falls after it stops. A meter on the filter's output stays up with no note, because the saw runs all the time and only the VCA is gated. Tested twice: as meter readings in `loom-modules/tests/panels.rs`, and as drawn levels and colour bands in `loom-app/src/live.rs`. (This was a manual check that said "after the filter"; [#92](https://github.com/brylie/LoomDSP/issues/92) found that wording wrong.)

### Out of scope

- Stereo meters. ("Add meter" came with "Add scope": see [Add scope and Add meter](#add-scope-and-add-meter).)
- Peak hold numbers and clip indicators.
- Spectrum and scope displays.

## Labels and hover text

**Status:** Implemented, except the manual check below. Code: `loom-app/src/scene.rs` (labels), `loom-canvas/src/hover.rs` (timing and placement), `draw_hover_text` in `loom-canvas/src/widget.rs`, and the inspector in `loom-app/src/main.rs`. Builds on [port and parameter text](port-and-parameter-text.md) ([ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md)): every port and parameter has a `label` and a one-sentence `description`. This section says where the label and description appear.

### Goal

A user zooms in on Clock Divide. The knob says "Division", not `n`. They rest the mouse on it and read "Fires on every Nth trigger." They never leave the canvas to learn what a control does (product principle 11).

### Where each text shows

| Place | Showed before | Shows now |
|---|---|---|
| Port label on a tile | name | label, cut with "…" to fit its column as now |
| Caption of a knob, toggle, button, or radio group | parameter name | parameter label, cut with "…" to fit the widget |
| Option of a radio group | choice name | choice name, cut with "…" to fit the widget ([#394](https://github.com/brylie/LoomDSP/issues/394)) |
| Caption of a meter | output name | output label |
| Caption of a light | display value name | display value label ([#118](https://github.com/brylie/LoomDSP/issues/118)) |
| Inspector parameter row | name | label |
| Hover text on a port or control | full name, only when the label is cut | label and description, always |
| Hover text on a light or meter | none | label, description, and value ([#118](https://github.com/brylie/LoomDSP/issues/118)) |

- **Names stay out of sight, except in hover text.** The hover text shows the stable name below the description, dimmed, when it differs from the label other than in letter case. So `n` shows under "Division", but `cutoff` does not repeat under "Cutoff". *Why:* names are what patch files and `loom modules` use, so a user who edits a RON file can find the port. Everywhere else, the label is the better text.
- **Captions and radio options are cut, never wrapped.** A label that does not fit is cut with "…", and the hover text shows it in full. Radio buttons are 6 units wide, so no built-in choice name is cut; a test checks this for every module. A longer name from a third-party module is cut and shows in full in the inspector. Widths are estimated per character, with narrow letters counting less, so "Division" fits under a knob, while "Resonance" is cut. *Why:* the grid layout of tiles does not change with text length, and estimating keeps text layout out of every frame.

### Hover text

- **When it shows.** After the pointer rests on a port or a control for 500 ms, every time. It hides as soon as the pointer leaves, a drag starts, or the canvas pans or zooms. *Why the delay:* while wiring, the pointer passes over many ports. Text that flashed up under every one would get in the way. 500 ms is what most desktop apps use.
- **During a drag it does not show.** That covers dragging a cable, a knob, or a tile. *Why:* the knob label already shows the value while it is dragged, and a cable drag needs a clear view of the target ports.
- **Keyboard focus shows it too.** When Tab moves focus to a control, its hover text shows after the same delay, and hides when focus moves on or Escape clears it. Right after a Tab press, the focused control's text wins over a port or control under a resting pointer, until the pointer moves. *Why:* the last thing the user did decides what they want to read. *Why:* keyboard users need the same help as mouse users (NFR-UX-01).
- **What it shows.** The label on the first line, in normal text colour. The description below it, in dim text, wrapped. The name last, dimmed, if it differs from the label (see above).
- **Size and place.** Fixed screen size, 12 px text, at most 260 px wide, whatever the zoom. On a canvas narrower than that, the box narrows to fit and the text wraps. It sits above the port or control, or below it when there is no room above, and always inside the canvas. *Why fixed size:* at middle zoom tile text is hidden because it is too small to read; the hover text is exactly what the user needs there, so it must stay readable.
- **A port's label hovers like the port.** Resting the pointer on a port's label, or on the bar that stands for it, shows the port's hover text. Pressing a label still selects or drags the tile, as before. *Why:* a cut label is what makes the user want the full text, so the label is where they point ([#92](https://github.com/brylie/LoomDSP/issues/92)).
- **At every zoom where the port or control can be hovered.** At middle zoom, labels are hidden, but hovering a port still shows its hover text.
- **Empty panel areas have no hover text.** Lights and meters have hover text since [#118](https://github.com/brylie/LoomDSP/issues/118): see [display values](port-and-parameter-text.md#display-values). They are not focused by Tab, because they are not controls.
- **It replaces the full-label overlay** from [Tile layout](#tile-layout-68): a cut label shows in full as the first line of the hover text.

### Inspector

- Each parameter row shows the label, and the value with its unit as now.
- Hovering the row's label shows the description and the name, in an Iced tooltip with the same content as the canvas hover text.

### Redraw and GPU data

- Showing or hiding hover text redraws once. While it is shown and nothing else changes, nothing redraws. The 500 ms wait is a single timer, not a frame loop.
- Labels are part of the scene. They change only when the patch changes, so they follow the existing rule: GPU data is rebuilt only when the scene's generation changes.
- Hover text is drawn as Iced text and quads in the overlay layer, like the full-label overlay today. It adds nothing to the instanced GPU buffers.

### Acceptance criteria: labels and hover text

- [x] Port labels, control captions, and meter captions show labels. A test builds the scene for Clock Divide and finds "Division", not `n`.
- [ ] Manual: inspector rows show labels, and hovering one shows its description, and its name when it differs.
- [x] Resting the pointer on a port or control for 500 ms shows its label, description, and (when it differs) name. Leaving hides it at once.
- [x] Resting on a port's label shows the port's hover text. Resting on empty tile space shows none.
- [x] No hover text shows during a cable, knob, or tile drag.
- [x] A control focused with Tab shows its hover text after the delay.
- [x] Hover text stays inside the canvas for a port at each edge of the window.
- [x] With hover text shown and nothing changing, hover text asks for no frames (`HoverTimer` tests).
- [ ] Manual: with Measure on, the frame rate with hover text shown matches the rate without it. (The stats bar still redraws twice a second, see above.)
- [ ] Manual, on the reference laptop: hover text is readable at close and middle zoom, and does not get in the way while wiring.

### Out of scope

- Translation (NFR-UX-04). Text is looked up by module ID and name when it comes.
- Labels and descriptions for panel widgets. (Display values have them since [#118](https://github.com/brylie/LoomDSP/issues/118).)
- [Help pages](help-pages.md) (#43), and module descriptions in the module picker.
- Touch: long-press for hover text.
- A shorter delay for the next hover text once one has shown. Consider it if reading a column of ports feels slow in use.

## P1 slice

What P1 builds. The rest of this spec is P2.

### Acceptance criteria

- [ ] Module panels on tiles: see [their acceptance criteria](#acceptance-criteria-module-panels) above.
- [x] A scope module fed by a 440 Hz oscillator shows a stable trace (`ScopeTrace::offset`, `trigger_is_stable_across_frames`; the subtractive example's scope, `docs/images/readme-subtractive.png`).
- [x] "Add scope" on a cable places a connected scope module next to it. Undo removes it.
- [ ] Cable opacity never depends on the signal. A test checks that the cable geometry is the same with and without telemetry.
- [x] With no displays on screen and no input, the app does not redraw. Check: `--measure-idle --bench-seconds 10` on `subtractive.ron` prints `frames=` with the same count from the second line on, with a device, with `--fake-audio`, and with `--no-audio` ([#90](https://github.com/brylie/LoomDSP/issues/90)).
- [ ] Zoomed far out, controls and displays are not drawn.
- [ ] Reference laptop, full window: 60 fps with four scopes on screen. Record CPU and GPU in the benchmark doc.

### Out of scope for P1

- The panel editor, sub-patch panels, and front panels (P2, FR-PANEL-*).
- Step grid and graph editing on the tile. They need module data (OQ-17, P2).
- XY pad, themes, custom skins (P2).
- Restyling built-in panels.
- Docked views at the window edge. Displays are modules.

## P1 additions

**Status:** Agreed. Each subsection names its issue. These close the gaps that building the P1 panels found. Choice names, display value text, and lights for one-sample triggers are Implemented.

### Choice names

([#160](https://github.com/brylie/LoomDSP/issues/160).) **Implemented.** Code: `ParamSpec::choices` and `ParamSpec::check_choices` in `loom-core/src/descriptor.rs`, `ParamControl` in `loom-app/src/inspector.rs`. A whole-number parameter can name its values.

- `ParamOptions` gets `.choices(&[...])`: one name per value, from `min` to `max` in steps of 1, so `max - min + 1` names. Names are short Title Case words or numbers with a unit ("Toggle", "Momentary", "24 dB"), at most 16 characters. The registry refuses names on a parameter that is not whole-number, a count that does not match, and an empty name, with an error that names the parameter.
- **The inspector** shows a parameter with names as radio buttons when there are 4 or fewer, otherwise as a pick list (the [radio or select rule](panel-widgets.md#radio-buttons-or-a-select-list) with the inspector's width). It never shows the slider with a bare number. A parameter without names keeps the slider.
- **A radio widget on a tile reads the names from the parameter.** `Widget::radio` no longer carries its own labels. *Why:* two lists for one parameter can disagree, and a panel that swaps radio buttons for a select list must show the same words ([panel-widgets](panel-widgets.md#choice-names)).
- **Text for a value** (hover text of a modulated control, the UI snapshot, the status bar) is the name when there is one, otherwise the number and unit.
- The names are stored in the descriptor, not in patches. Reword them freely (ADR-0016).
- The tile does not need them for Button's `mode`, which stays in the inspector.

### Display value text

([#118](https://github.com/brylie/LoomDSP/issues/118).) **Implemented.** Display values get the same text as ports and parameters, in [port and parameter text](port-and-parameter-text.md#display-values).

### Lights for one-sample triggers

([#114](https://github.com/brylie/LoomDSP/issues/114).) **Implemented.** Code: `DisplayKind`, the latch in `loom-core/src/display.rs`, `TriggerFlag` for modules, and `Fade` in `loom-app/src/lights.rs`. A one-sample trigger is almost never the latest value, so a light that reads the latest value stays dark. The fix has two parts, so cables keep their one-sample triggers.

- **A display value has a kind:** `Level` (today) or `Trigger`. A module writes a `Trigger` value as 1.0 if any trigger arrived in the block it just ran, else 0.0. `TriggerFlag` does this: set it in `process` and take it in `displays`. *Why a flag:* in a feedback loop `process` runs once per frame but `displays` once per block, so clearing in `process` would lose triggers.
- **The table latches it.** For a `Trigger` value, the engine stores with `max` instead of replacing: a 1.0 stays until the UI reads it. The UI reads a `Trigger` cell with `swap(0)`. *Why:* many blocks pass between two drawn frames. The latch makes the light show "fired since the last read", which is the truth the player cares about. Nothing waits and nothing allocates (ADR-0013).
- **The light holds on screen.** A read of 1.0 lights the light at full, and it fades to dark over 120 ms of UI time. *Why:* a trigger at 4 per second must be a visible flash, not a one-frame flicker.
- **Redraw.** A fading light asks for frames for 120 ms after the last trigger, then none. An idle patch draws nothing.
- **Two readers are not supported.** The table has one reader, the UI. A second consumer (cable activity, scopes) uses its own mechanism ([activity](#signal-activity-on-ports)).
- Clock Divide and Phase Trigger each get a light on their trigger output. A test sends one one-sample trigger and sees the light lit.

### Clock mark on transport inputs

([#103](https://github.com/brylie/LoomDSP/issues/103).)

- An input that has no cable and a default of bar phase, beat phase, or playing shows a **clock mark**: a small ring with two short hands, drawn between the port marker and its label, in the module's accent colour at reduced strength. It is the same mark for all three; the hover text tells them apart ("Follows the bar phase while nothing is connected").
- It is hidden when a cable is connected, and returns when the cable is removed.
- It shows at the zoom where port labels show, and as a plain dot at the middle zoom. It is not drawn at far zoom.
- `PortView` gets `follows_transport: Option<TransportSignal>`, set by the app from the descriptor's `PortDefault` and the cable state. The mark is part of the scene's GPU data, which changes only when cables change (ADR-0010).
- It is in the UI snapshot as a port field, so the control channel can find it.

### Audio load colour

([#102](https://github.com/brylie/LoomDSP/issues/102).) This replaces the first and third bullets of [Audio load and dropouts](#audio-load-and-dropouts-in-the-stats-bar).

- `CallbackStats` keeps `peak_since_read`: the highest load of any callback since the last read. The stats bar reads and clears it once per tick, and **colours the load by that peak**, not by the latest callback. *Why:* a spike shorter than one tick is the warning of a coming dropout, and the latest sample almost never catches it.
- The number shown is the load of the latest callback, as now.
- **"Max" is the highest load in the last 10 seconds,** not since the app started. It is a ring of tick peaks. *Why:* after one spike, a since-start maximum says 95% for the rest of the night and stops being useful.
- Amber from 70%, red from 90%, as now. The dropout count stays red for 10 seconds after it grows.
- If [#90](https://github.com/brylie/LoomDSP/issues/90) changes how the bar updates, the window is "per tick", not "per 500 ms".
- Test: feed one callback at 95% among thousands at 20% and see the bar turn red for that tick; after 10 seconds of quiet, the max falls.

### Keyboard without a mouse

([#101](https://github.com/brylie/LoomDSP/issues/101).) Closes the gap that nothing selects a module by key. Keys have three owners, tried in this order, and the first one that has a use for the key takes it:

1. **A focused panel control** (knob, toggle, radio, button).
2. **The canvas** (selection, movement, clipboard, undo).
3. **Global keys** (Space for Run).

| Key | Panel control focused | Module selected | Nothing selected |
|---|---|---|---|
| Tab, Shift-Tab | Next or previous control of the module; after the last, the next module | Next or previous module in reading order | Selects the first or last module |
| Enter | Acts on the control (toggle, press) | Focuses the module's first control | Nothing |
| Space | Acts on a toggle, radio, or button. A knob does not take it | Toggles Run | Toggles Run |
| Arrow keys | Change the value ([Interaction](#interaction-68)) | Move the selected modules one grid unit; Shift moves 10 | Nothing |
| Escape | Clears control focus | Clears the selection | Nothing (after the other uses of Escape: a drag, then pinned lines) |

- **Reading order** is top to bottom by tile top, then left to right, then by module ID. It is the same order as Tab in the module browser's results.
- Tab selects the module, so the inspector and the highlight follow. Focus on a control also selects its module.
- `focus_keys` no longer swallows Space and Enter for a knob: a key with no use passes on to the next owner. *Why:* a swallowed key is a key that does nothing.
- Moving by keys is one undo step per key press group (repeats within 500 ms join one step).
- Test: select a module, focus a knob, change its value, and leave, by keys alone.

### Signal activity on ports

([#63](https://github.com/brylie/LoomDSP/issues/63).) **Partly implemented:** the measurement, the two states, the rate, and the redraw rule are built (`take_activity` in `loom-app/src/monitor.rs`), and the UI snapshot shows `carrying` on each port. The marker is not drawn yet: see Open question below. Cables stay still ([Cables do not animate](#cables-do-not-animate)). Ports show whether they carry a signal.

- **Two states:** *carrying* (a filled marker in the signal colour) and *silent* (the marker as now). Input ports show the state of the output feeding them.
- **Carrying** means the output's largest absolute value in the last tick was above 0.0001 (−80 dB). *Why:* DC offsets and constants count as signal, since a Constant of 0.5 is a live control. Exactly 0 and noise below −80 dB are silent.
- **Where it is measured.** With the activity display on (Preferences, default on), `Live::sync` keeps a `Meter` probe on every output that has a cable ([telemetry](telemetry.md#in-the-desktop-app)). The telemetry thread already keeps the peak since the last drain. No new engine path is needed. One-sample triggers count, because the peak covers every frame of every block. When the display is off, no probes are added. *Why reuse meters:* the cost is already measured, and the code is built and waiting (`Live`).
- **Rate.** The UI reads the peaks 5 times a second. A port becomes *carrying* at once, and *silent* after 2 reads (400 ms) with nothing. *Why:* gates and slow envelopes would flicker otherwise.
- **Redraw.** The markers are drawn from a small buffer of port states, like the panel values buffer, so a change never rebuilds tiles, ports, or cables (ADR-0010). A state change draws one frame. No change, no frames.
- Cables do not change brightness. Hover focus rules are unchanged.
- At middle and far zoom the markers are not drawn (they are port-sized).
- **Open question (found while building #63):** a port with a cable is already drawn as a filled marker in its signal colour, so *carrying* as written looks the same as *silent*. The look of the two states needs a decision before the canvas draws them. The Preferences switch is not built either: activity is always on.

### Drawing the scope trace

([#62](https://github.com/brylie/LoomDSP/issues/62).) **Implemented.** Code: `WidgetKind::Scope`, `loom-canvas/src/trace.rs` (`ScopeTraces`, the `vs_trace` and `fs_trace` shaders), `ScopeTrace::canvas_trace` in `loom-app/src/live.rs`, and the telemetry thread in `loom-app/src/monitor.rs`, which builds the traces and wakes the UI only when a drawn trace changes. Building it settled one reading: the scope widget fills the panel area below the scope module's controls, and its frame is drawn with the controls, not the tile, so it also costs nothing per frame.

- **Where.** The scope widget fills the panel area of the scope tile. Its frame is drawn with the tile; the trace is a line strip clipped to the frame.
- **Data.** Each frame the app copies the points that fit the module's `window` into one small vertex buffer (at most 1024 points: more are skipped by stride, keeping the min and max of each group). It uploads that buffer and nothing else. Tiles, ports, and cables are not rebuilt (ADR-0010).
- **Steady wave.** `ScopeTrace::offset` shifts the trace by the fraction of a sample between the trigger and the first point, so a periodic wave does not shimmer.
- **Follows the camera.** The trace is in tile space, so pan and zoom only change the camera uniform.
- **Level of detail.** Drawn at the zoom where controls are drawn. At the middle zoom a flat line at the frame's centre, brightness following the level. Not drawn at far zoom.
- **Redraw.** The scope draws a frame only when it is on screen, big enough to read, and its data changed. A silent or still signal draws none. An idle patch or an off-screen scope does not redraw.
- **Size.** The widget takes the space the panel gives it, and grows with a resized tile ([resizable tiles](#resizable-tiles)).
- Acceptance: four live scopes on screen keep 60 fps on the reference laptop (NFR-PERF-04, ADR-0010 gate 2).

### Resizable tiles

([#61](https://github.com/brylie/LoomDSP/issues/61).)

- **The handle.** A small grip at the bottom-right corner of a tile that has a **growable** widget (a scope, a meter, an XY scope, later a spectrum or curve). Other tiles have no handle. Dragging it changes the tile's size and snaps to the grid. The cursor shows a resize cursor over it.
- **Minimum and maximum.** The minimum is the size the panel declares (the tile cannot be smaller than its layout). The maximum is 64 by 64 grid units.
- **What grows.** Only growable widgets. They take all extra width and height. Other widgets keep their size and position, top left.
- **Ports.** Inputs stay on the left edge and outputs on the right, spread evenly over the new height, in port order. Cable ends, hit-testing, and hover follow the new size.
- **Saved.** The patch file gets an optional `size: (w, h)` next to the position, in grid units. A tile without one has its declared size. Files without the field load unchanged (ADR-0008).
- **Undo.** One resize drag is one undo step. The size is part of the clipboard: paste keeps it.
- **Cost.** A resize changes the scene once, when the drag ends, and shows a live outline while dragging. Nothing is rebuilt per frame (ADR-0010).
- **Keyboard.** With a growable tile selected, Alt-arrow keys resize by one grid unit. Shift-arrow still moves the tile by 10 ([Keyboard without a mouse](#keyboard-without-a-mouse)). *Why:* NFR-UX-01. A separate modifier means the two rules never both apply to one key.

### Acceptance criteria: P1 additions

- [x] Choice names: the registry refuses a wrong count, an empty name, and names on a continuous parameter. The inspector shows "Toggle" and "Momentary" for Button's `mode` and a slider for a parameter without names. No module hard-codes names in its panel.
- [x] A one-sample trigger into a Clock Divide or Phase Trigger output light lights it, and it fades in 120 ms. No frames are drawn after the fade.
- [ ] A transport-following input shows the clock mark with no cable, loses it when a cable is connected, and has it back when the cable is removed.
- [ ] A 95% callback spike turns the load amber or red for its tick. After 10 seconds the max falls.
- [ ] By keys alone: Tab to a module, Enter into its panel, change a knob, Escape out twice. Space on a focused toggle acts on the toggle, not on Run. Arrow keys move the selected module by one grid unit.
- [ ] An output with a cable shows *carrying* within 200 ms of a signal above −80 dB, and *silent* 400 ms after it stops. A one-sample trigger lights it. Cable geometry is the same with and without activity. (The state is tested, `activity_turns_on_at_once_and_off_after_two_quiet_reads`, and shows in the UI snapshot. Drawing it waits for the open question above.)
- [x] The scope draws its trace in the tile, holds still for a periodic wave, and four scopes keep 60 fps. Checked on a development Mac (not the reference laptop): four live scopes draw at 120 fps with `--measure`, and upload only the trace buffer (`trace_uploads`, no scene or control uploads). Recording CPU and GPU on the reference laptop is left for the benchmark doc.
- [ ] A growable tile resizes to the grid within its limits. Ports and cable ends follow. The size is saved and loads. Old patches load. One drag is one undo step.

## Default panels

**Status:** Draft, P2. Decision: [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md). Requirements: FR-PANEL-09, FR-SDK-01, NFR-UX-01.

A module that declares no panel gets one derived from its parameters, display values, and data slots. The author can change one parameter's widget with a hint, or write the whole panel. *Why:* most of a panel follows from the parameters. P1 panels are already "a knob for every parameter", with a few kept in the inspector. Deriving them means every module, built-in or third-party, gets a consistent, keyboard-reachable panel with no layout work.

### Which widget

Each declared item gets one widget, by the first rule that matches:

| Declared item | Widget |
|---|---|
| A parameter with a hint | The widget the hint names, or none for "inspector only" |
| A whole-number parameter from 0 to 1, with no choice names | Toggle |
| A whole-number parameter with choice names, 5 or fewer | Radio buttons |
| A whole-number parameter with choice names, more than 5 | Select list |
| Any other parameter | Knob |
| A display value | Light |
| A data slot | Its kind's editor: curve, step grid, or waveform |

- The choice between radio buttons and a select list follows [Radio buttons or a select list?](panel-widgets.md#radio-buttons-or-a-select-list). Choice names come from the parameter ([#160](https://github.com/brylie/LoomDSP/issues/160)).
- A widget that binds more than one parameter (button, XY pad) is never derived. *Why:* the rules cannot know which two parameters belong together. A module that wants one writes its panel by hand, as the Button module does today.
- A widget bound to an output (meter, scope, XY scope, spectrum) is never derived either. *Why:* most outputs carry signals that nobody needs to watch on the tile, so a rule would add displays almost everywhere. A display module writes its panel by hand, as the Level Meter does today.
- A whole-number parameter without names gets a knob, as in P1. Leaning: the number widget, once it exists ([panel-widgets.md, Number](panel-widgets.md#number)).

### Hints

A hint is an option on the parameter, in `ParamOptions`. It names one widget from the parameter's group in [the baseline set](panel-widgets.md#the-baseline-set), such as a fader instead of a knob, or "inspector only".

- The registry refuses a hint outside the parameter's group, such as a toggle on a continuous parameter. *Why:* a hint picks a way to show the same kind of value. It cannot change what the value is.
- "Inspector only" is for parameters that are set once and rarely played: a noise seed, a MIDI channel, a bend range.
- A hint is not a position or a size. *Why:* layout belongs to the panel. A hint lives on the parameter, so it also reaches a sub-patch surface that exposes it (see below).

### Layout

- Data editors come first, each on its own row, at the full panel width. Then the parameter widgets in the order the parameters are declared. Then the display values, in their declared order.
- Widgets flow left to right on the panel grid and wrap at 12 grid units, four knobs wide. Each row is as tall as its tallest widget. Leaning: check the width against real modules and the estimated label widths ([#120](https://github.com/brylie/LoomDSP/issues/120)).
- The panel's size is the space its widgets fill. A module with nothing to show gets an empty panel, as today.
- Focus order is the order above, so Tab follows what the user reads.
- *Why declaration order:* the author already puts parameters in a meaningful order for the inspector. Deriving from it keeps the tile and the inspector alike.

### Writing a panel by hand

- A module can still declare its panel in full. Then nothing is derived for it, and the registry checks it as today ([Panel declaration](#panel-declaration-67)).
- **A module's hand-written panel shows every parameter that is not "inspector only".** The registry refuses a module panel that leaves one out, and names it. *Why:* the hint is then the one place that says a parameter is not on the tile. A forgotten knob is caught at registration, and a reader of the descriptor sees the choice. This rule is for module panels only. A sub-patch panel and a front panel show what their user chose, because a performance panel with every parameter on it is not minimal. The inspector shows every parameter either way.
- *Why keep "inspector only" at all:* some parameters are set once and never played, such as a seed or a MIDI channel. On a tile they take space from the controls that are played, and every derived panel would show them.
- The derived panel is a pure function of the descriptor, computed once when the module type is registered. *Why:* the same module must always look the same, and nothing about it may be rebuilt per frame.

### Acceptance criteria: default panels

- [ ] Each built-in module except Button and Level Meter gets its panel derived, with "inspector only" hints where P1 keeps a parameter in the inspector. Its widgets, bindings, and focus order equal today's hand-written panel (test). Positions may change.
- [ ] Deriving the panel twice from one descriptor gives equal panels (test).
- [ ] The registry refuses a hint outside its parameter's group, with a message that names the parameter (test).
- [ ] The registry refuses a hand-written module panel that leaves out a parameter without an "inspector only" hint, with a message that names the parameter (test).
- [ ] A test module with a 2-choice named parameter gets radio buttons, a 0 to 1 parameter without names a toggle, and a 12-choice named parameter a select list (test, once the select list exists).

## Exposed controls in a sub-patch

**Status:** Draft, P2. Decision: [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md). Requirements: FR-ENCAP-03, FR-PANEL-03, FR-PANEL-09.

A sub-patch's surface is made of the same widgets as its inside. *Why:* a user who enters a sub-patch, or leaves it, must not have to learn a new set of widgets. The knob outside is the knob inside, and so is the light, the scope, and the curve.

**Any widget can be exposed:** a control, a light, a readout, a meter, a scope, a curve, a step grid, or a waveform. The surface widget binds to the same inner item: a parameter, a display value, an output's probe, or a data slot. Parameters follow the rules below. A display shows the same live value inside and out. An editor on the surface edits the inner module's data, and each edit is one undo step, whichever side it is made on.

- **Exposing copies the parameter's spec.** When an inner parameter is exposed (FR-ENCAP-03), the sub-patch's new parameter takes its label, description, unit, mapping, choice names, and hint. The user can give it a new name and a narrower range. The mapping and the kind of value stay. *Why:* a logarithmic cutoff must still turn like a cutoff, and a named choice must still show its names.
- **The same widget on both sides.** The surface shows the widget the inner tile shows for that parameter: a fader inside is a fader outside. In the panel editor, the user can switch it for another widget in the same group, as a hint does.
- **One parameter, two controls.** Both controls are bound to the same parameter, so they move together, show the same modulation, and an edit on either is one undo step (FR-PANEL-03).
- **A sub-patch with no designed panel** gets a panel derived from what it exposes. Each exposed item keeps the widget it has inside, and the widgets are placed by the [Default panels layout](#layout-1) rules, in the order the items were exposed. When the user opens the panel editor, it starts from that panel.
- **Seeing the link.** Inside the sub-patch, a widget that is exposed has a small mark, a shape and not only a colour (NFR-UX-03), and its hover text adds "Exposed as" and the surface label. On the surface, the hover text adds which inner module and parameter it controls. *Why:* entering a sub-patch, the user should find the knob they just turned outside without searching.

### Acceptance criteria: exposed controls

- [ ] Exposing a parameter shown as a fader puts a fader on the sub-patch surface (`Ui` harness).
- [ ] Turning the surface control moves the inner control, and the reverse. One undo step reverts either (`Ui` harness).
- [ ] A sub-patch with three exposed parameters and no designed panel shows three widgets in the order exposed.
- [ ] An exposed control inside the sub-patch has its mark and its "Exposed as" hover text (UI snapshot).
- [ ] A light and a curve exposed from inside a sub-patch show on its surface. The light follows the inner display value, and a point dragged on the surface curve moves on the inner curve. One undo step reverts it (`Ui` harness).

## Open points

- Can users change the layout of a built-in module's panel? Its look cannot change (ADR-0025).
- How third-party modules add display kinds through the SDK.
- A derived display value gets a light. Once display values carry a unit and range ([#118](https://github.com/brylie/LoomDSP/issues/118)), should those with a unit get a readout or a meter instead?
