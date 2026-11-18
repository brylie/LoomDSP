# Panels Specification

How modules, sub-patches, and patches show controls and displays, and how users build them.

- **Kind:** living specification. It changes as panels are built and used.
- **Status:** Draft, except [Module panels on tiles](#module-panels-on-tiles-m1) and [Level meters and audio load](#level-meters-and-audio-load-m1), which are Implemented. [Labels and hover text](#labels-and-hover-text) is Implemented. [Scope module](#scope-module) is Implemented, with its trace ([#62](https://github.com/brylie/LoomDSP/issues/62)). [The panel editor](#the-panel-editor) and [Exposed controls in a sub-patch](#exposed-controls-in-a-sub-patch) are Agreed (M2, 2026-11-01). [Default panels](#default-panels) is Draft, except its flow and wrap rules, which are Agreed (2026-11-07).
- **Decision behind it:** [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (modules carry their own panel), [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) (one design language, and panels derived from the declaration).
- **Requirements:** FR-PANEL-*, FR-CANVAS-10, FR-CANVAS-11, FR-LIB-18, FR-VIS-01 to 04, FR-ENCAP-03, NFR-PERF-04, NFR-UX-01.
- **Builds on:** [Telemetry](telemetry.md) for live data, [ADR-0010](../adr/0010-own-patch-canvas.md) for the canvas, [parameter mapping](modulation.md#parameter-mapping) for control values.
- **Issues:** [#7](https://github.com/brylie/LoomDSP/issues/7) (umbrella), [#67](https://github.com/brylie/LoomDSP/issues/67) (panel declarations), [#68](https://github.com/brylie/LoomDSP/issues/68) (controls on tiles), [#69](https://github.com/brylie/LoomDSP/issues/69) (display values), [#9](https://github.com/brylie/LoomDSP/issues/9) (level meter, audio load), [#619](https://github.com/brylie/LoomDSP/issues/619) (lights across a pan or zoom), [#637](https://github.com/brylie/LoomDSP/issues/637) (non-finite scope samples, far-edge growth, two meters on one output), [#730](https://github.com/brylie/LoomDSP/issues/730) (the scope's wake rule and drawn size).

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

The widgets after M1, with their bindings, modulation display, keyboard use, and the catalogue entries that need them, are specified in [Panel widgets](panel-widgets.md) (Agreed, [ADR-0024](../adr/0024-widgets-follow-the-kind-of-value.md)). It names the slider "fader", the selector "select list", and the graph "curve", and adds a number, a readout, an XY scope, and a waveform.

The [read-only shape preview](panel-widgets.md#read-only-shape-preview) is an agreed first slice of the curve widget. Envelope, LFO, and Ladder place it above their controls. Each puts its choices in a row under the full-width shape ([Panel layout](#panel-layout), principle 2); Envelope's Mode is a segmented control. The preview shows a schematic result from parameters, keeps the tile's drag gesture, and has no editing handles. Built-in tiles use knobs for single ranged parameters by default; a fader needs a specific task such as comparing mixer levels.

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

## Panel layout

**Status:** Agreed, M1 ([#453](https://github.com/brylie/LoomDSP/issues/453)). Decision on modes: [ADR-0030](../adr/0030-panels-hide-controls-a-mode-does-not-use.md).

A panel reads from top to bottom as **what it does, how it behaves, what you play, what it reports**. Exact numbers stay one hover away. Every built-in panel follows these principles, and so do derived panels ([Default panels](#default-panels)).

1. **Purpose first.** A result view (envelope shape, filter response, LFO wave, scope, meter) sits at the top at full panel width. *Why:* it answers "what does this module do right now" before any control is read.
2. **Behaviour before detail.** A choice that changes what other controls mean (envelope mode, filter slope, LFO sync, compare direction) sits directly under the view, or at the top when there is no view. *Why:* the mode is the context for every knob below it.
3. **Play row.** Continuous controls follow in one row, in the order the sound happens (Attack, Decay, Sustain, Release; Cutoff, then Resonance). Controls set less often go in a second row. *Why:* the hand finds the most-played controls in the same place on every tile.
4. **Report next to the cause.** A light sits beside the control or port it reports on, or at the right end of the mode row. It never floats alone. *Why:* a light only helps when the eye knows where to look.
5. **Show only what the mode uses.** A control its mode ignores is hidden ([ADR-0030](../adr/0030-panels-hide-controls-a-mode-does-not-use.md)). The tile keeps its width: a row with a hidden control closes up and is centred again, and the tile gets shorter only when whole rows empty, so ports and cables never move. The inspector still lists the hidden parameter and says which choice does not use it. *Why:* a knob that does nothing misleads a performer; a tile that changes width would move cables on stage.
6. **Smallest widget that keeps the choice visible.** On and off is a toggle. Two to four short named choices are a [segmented control](panel-widgets.md#segmented-control): one row, every option visible, one click. More choices, or long names, are a select list. Voice merge uses a wider segmented row for its five modes, as its polyphony specification requires. Radio buttons are for lists of five that need every name visible and have the height to spare. *Why:* seeing the options matters on stage, and a segmented control shows them in one row.
7. **One alignment grid.** Captions are centred under their widget. A group's caption sits above it. Groups are one grid unit apart, and a row of controls is centred in the panel. The panel is as wide as its widest row; there are no empty quarters.
8. **Words fit.** No built-in panel cuts a caption or port label at 100% zoom. If a word does not fit, the layout changes, never the stable ID ([Labels and hover text](#labels-and-hover-text)).
9. **Details on demand.** Exact values, ranges, units, and descriptions are in hover text, the inspector, and help. The tile shows only what is played or watched.
10. **Same thing, same place.** Repeated strips (mixer channels) are labelled once per strip. Numbered things use the numbers users see on hardware ("1-2", "3-4" for channel pairs).

### Acceptance criteria: panel layout

- [x] Every built-in panel puts its result view first, then its choices, then its continuous controls (test over the registry, by widget kind and row).
- [x] Every built-in module with 2 to 4 short named choices on its tile uses a segmented control (test over the registry).
- [x] A parameter can declare `shown_when`; its widget is hidden while the condition is false, the tile keeps its width, a row with a hidden widget is centred again, and the tile gets shorter only when whole rows empty (tests). Envelope Sustain and Release in AD mode, LFO Rate and Cycles by sync mode, MIDI Controller's controller number, and Scope's Level with Auto on use it.
- [x] The inspector marks a hidden parameter "Not used in" its current choice, and Tab skips hidden controls (tests).
- [x] No built-in tile is more than one grid unit wider than its widest row needs (test over the registry).
- [x] Audio In's pairs read "1-2", "3-4", "5-6", "7-8"; Mixer labels its strips once; Button shows its label on the button; Level Meter's meter caption is not a port name; Scope's zero line is visible at 100% zoom.
- [x] `examples/dev/all-modules.ron` holds every built-in module on a grid, and a test renders it.

## The panel editor

**Status:** Agreed, M2 ([#76](https://github.com/brylie/LoomDSP/issues/76)). The choices marked "Settled" were made with the maintainer on 2026-11-01. Requirements: FR-PANEL-01, 03, 05, 09, 10. Builds on [sub-patches](sub-patches.md) and [exposed controls](#exposed-controls-in-a-sub-patch).

> Inside my Voice sub-patch I open the Panel pane. I drag the filter's cutoff knob onto it, then resonance, then the envelope's shape. I switch the pane to Edit, put a "Filter" frame around the two knobs, and line them up. I go back up a level: the Voice tile now shows exactly that panel.

### The Panel pane

**Settled:** the editor is a pane docked beside the canvas.

- The **Panel** button in the toolbar shows or hides the pane. It opens between the canvas and the inspector column, so the inspector stays visible. *Why:* exposing is a drag from the inside to the surface, so both must be on screen at once.
- The pane shows the panel of the current level: the sub-patch's surface when inside one, or the patch's **front panel** at the top level. The same tools appear at every level. *Why:* FR-PANEL-09, one panel model.
- The pane has two modes, switched by an **Edit** toggle in its header:
  - **Play:** widgets work as on a tile. Turning a knob in the pane turns the exposed parameter.
  - **Edit:** a drag moves widgets instead of changing values. Values cannot change in Edit mode. *Why:* arranging must never nudge a setting by accident.
- Every change in the pane shows at once on the sub-patch's tile in the parent. There is no separate save or apply.
- The pane is as wide as the panel at the current canvas zoom, up to half the window, and scrolls beyond that.

### Adding and removing

**Settled:** every control and display on a panel is bound by exposing. Only labels and frames are unbound.

- **Expose by drag:** drag a control or display from an inner tile, or a row from the inspector, onto the pane. It is exposed ([exposed controls](#exposed-controls-in-a-sub-patch)) and placed where it is dropped, snapped to the grid. Works in both modes.
- **Expose from a menu:** **Expose** in an inspector row's menu places the widget at the first free place, in reading order.
- At the top level, exposing adds the item to the patch's front panel. The top patch exposes items as a sub-patch does. *Why:* performance mode and later plugin export show the front panel, and it should be built the same way.
- **Palette** (Edit mode only): **Label** (static text) and **Frame** (a group). Nothing else. *Why:* an unbound knob does nothing, and a panel must never hold broken widgets.
- **Unexpose** on a widget's menu removes it from the panel and removes the exposed item. If the same item has a second widget on the panel, only this widget goes. Delete removes the selected labels and frames, and unexposes the selected widgets.

### Arranging

In Edit mode:

| Gesture | Result |
|---|---|
| Drag a widget, label, or frame | Moves it, snapped to the panel grid. A frame moves with its widgets. |
| Click | Selects one item. Shift-click adds or removes. |
| Drag on empty space | Selects the items inside the rectangle. |
| Arrow keys | Move the selection one grid unit. Shift-arrow: 10. |
| Alt-arrow | Resizes a selected growable widget or frame by one grid unit, as on [resizable tiles](#resizable-tiles). |
| Drag the corner grip of a growable widget or a frame | Resizes it, snapped to the grid. |

- **Snap:** positions and sizes are whole panel-grid units, the same grid as tiles. *Why:* the same rule as built-in panels; tidy without pixel-pushing (FR-PANEL-05).
- **No overlap:** a widget dropped on another is moved to the nearest free place. Labels may sit inside frames.
- **Align and distribute:** with two or more items selected, the pane's **Arrange** menu has Align left, right, top, bottom, centre, and middle, and Distribute horizontally and vertically.
- **Widget menu:** **Show as** switches to another widget in the same group, such as knob and fader ([panel widgets](panel-widgets.md#the-baseline-set)). **Caption** sets the text under the widget for this panel only; empty means the exposed name. **Controller slot** (front panel only): Auto, or a slot from 1 to 16 for [device mappings](midi-learn.md#device-mappings-fr-midi-06). **Unexpose**.
- **Panel size:** the panel is as large as its items need. A grip at its bottom-right corner makes it larger, never smaller than its items. The sub-patch's tile takes that size, with ports on its edges as on every tile.
- One drag, one menu command, or one held arrow key is one undo step, in the patch history.

### Groups

**Settled:** a group is a titled frame.

- A **frame** is a visible border with an optional title, such as "Filter" or "Amp". Widgets inside its rectangle belong to it.
- Moving a frame moves its widgets. Resizing it never moves them; a widget left outside stops belonging to it.
- Frames do not nest. *Why:* one level is enough to read a panel, and nested frames are rules nobody can see.
- **Group** in the Arrange menu draws a frame around the selection; **Ungroup** removes the frame and keeps the widgets.
- A frame's title is read by screen readers as the group's name (NFR-UX-02).

### Focus order

- Keyboard focus visits the panel in reading order: top to bottom, then left to right. A frame's widgets are visited together, in reading order inside it. *Why:* focus follows what the eye reads, with no separate order for the player to keep up to date.

### Built-in module panels

**Settled:** a built-in module's panel cannot be changed in M2. *Why:* every Filter looks the same in every patch, tutorial, and help page. For a different layout, collapse the module into a sub-patch and design its panel.

### Saved format

A designed panel is saved with its definition (or, for the front panel, with the patch):

```ron
panel: Some(Panel(
    size: (8, 6),
    widgets: [
        Widget(kind: Knob, at: (0, 1), bind: "cutoff"),
        Widget(kind: Knob, at: (2, 1), bind: "resonance", caption: Some("Res"), slot: Some(3)),
        Widget(kind: Curve, at: (0, 3), bind: "shape", size: Some((8, 3))),
    ],
    labels: [Label(at: (4, 1), text: "Voice")],
    frames: [Frame(at: (0, 0), size: (4, 3), title: Some("Filter"))],
)),
```

- `bind` names an exposed item. A panel with no `panel` field gets the derived one ([exposed controls](#exposed-controls-in-a-sub-patch)).
- Built-in module panels in code use the same structure, with captions, labels, and frames available to them (FR-PANEL-09). Today's `Panel` and `Widget` in `loom-core` gain those fields.
- A widget whose exposed item is gone when the patch loads is dropped, and the status bar says so. *Why:* a patch must always open.

### Acceptance criteria: panel editor

- [ ] Dragging an inner knob onto the pane exposes the parameter and places a knob where it was dropped; the sub-patch tile in the parent shows it (`Ui` harness).
- [ ] Expose from the inspector menu places the widget at the first free place in reading order (test).
- [ ] In Play mode, turning a pane knob changes the parameter; in Edit mode, the same drag moves the knob and the value does not change (`Ui` harness).
- [ ] Moves and resizes snap to whole grid units; a drop onto another widget lands at the nearest free place (test).
- [ ] Align left on three widgets gives them one x; Distribute horizontally gives equal gaps (test).
- [ ] Moving a frame moves its widgets; frames cannot be placed inside frames (test).
- [ ] Focus order is reading order, with a frame's widgets together (test).
- [ ] Show as switches a knob to a fader and refuses a widget from another group (test).
- [ ] Each arrange gesture is one undo step (`Ui` harness).
- [ ] A designed panel round-trips through the patch file; a widget bound to a missing exposed item is dropped with a status message (test).
- [ ] At the top level, the pane edits the patch's front panel and saves it with the patch (test).
- [ ] The Panel pane works by keys alone: Tab into the pane, toggle Edit, move a widget with arrows (`Ui` harness, NFR-UX-01).

### Out of scope: panel editor

- Embedding a sub-patch's panel inside its parent's panel (FR-PANEL-08). Later, as one more widget kind.
- Performance mode itself: showing only the front panel, locking edits, large controls (FR-PERF-01). It shows the panel this editor builds, and has its own spec.
- Colours, images, and free styling. ADR-0025 gives one design language.
- Changing built-in module panels.

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
- A scope on screen animates while its trace moves, at most once a frame. A still smooth wave, or a trace that moves by less than can be seen, draws nothing. A wave with a corner sharper than a sample can still wake it, as [Drawing the scope trace](#drawing-the-scope-trace) explains. (Implemented, [#730](https://github.com/brylie/LoomDSP/issues/730).) A step light changes a few times a second, so it redraws a few times a second.
- Nothing on screen changing means no redraw.
- **A running transport is not idle** (Implemented, [#730](https://github.com/brylie/LoomDSP/issues/730)). The Transport's Beat light flashes on every beat while the transport runs, so a patch with that light on screen draws a few frames each beat. That is a visible change, and the frames are correct. *Why:* "idle" means nothing on screen changes. A running transport also flashes the lights it drives, such as Clock Divide's, Phase Trigger's, and step lights. So a check that a patch draws no frames at rest runs with the transport stopped, or with no light it drives on screen. `--measure-idle` counts these frames as `lights`, not `meters` ([frame meter](frame-meter.md#causes)). A new patch's transport starts running ([clock](clock.md)), so this applies to most patches. These are [one-sample trigger lights](#lights-for-one-sample-triggers).

*Why:* GPU time is not the limit (the canvas holds 120 fps with 300 modules). Battery and heat are, and a laptop on stage often runs on battery. Drawing only what changed keeps the cost in line with what the user is looking at.

Measure on the reference laptop: idle CPU and GPU with nothing changing, and CPU, GPU, and power with four scopes on screen.

## Cables do not animate

Cables do not pulse or change brightness with the signal they carry. Signal activity (FR-VIS-01) is shown on ports, as a steady state such as "carrying signal" or "silent", updated a few times a second. *Why:* when everything moves, nothing stands out, and constant motion tires the eyes. Hover focus already brings the cables you care about to the front.

## The inspector

The inspector stays, as the place for:

- every parameter, including those not on the tile;
- typing exact values;
- modulation ranges, MIDI mappings, and other settings that do not fit on a control.

### Layout

**Status:** Agreed, M1 ([#457](https://github.com/brylie/LoomDSP/issues/457)).

- **With nothing selected,** two short lines ("Select a module to edit it." and "Press A over the canvas to add one.") and a **Keyboard shortcuts** button that opens the shortcut list in the help overlay. *Why:* the full list of gestures was useful once and noise after; it was about 30 lines in a 280 px column.
- **A module** shows its name, the **Help** button, its description, then its parameters. The type ID is not shown here: it is in the header's hover text and in help.
- **Ports** come last, under small **Inputs** and **Outputs** headings, one line each: the label, then the signal kind and usual range ("audio, -1 to 1"). The description is in the line's hover text.
- **Modulation assignments** under a parameter use the matrix's card ([modulation.md](modulation.md#slice-3-the-modulation-matrix)) without the target line: on and source with ×, the polarity as a quiet pick list, then the depth slider and value. **+ Add modulation** is a button.
- The scrollbar takes its own space, so it never covers a value.

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
| `level` | Level | -10 to 10 | 0 | The trigger level while `auto` is off. [Control ranges](control-ranges.md) -1 to 1 (default) and -10 to 10, like the scope's vertical ranges ([#329](https://github.com/brylie/LoomDSP/issues/329)). |
| `slope` | Slope | 0 Rising, 1 Falling | 0 | Which edge starts the trace ([#329](https://github.com/brylie/LoomDSP/issues/329)). In the inspector only, and always shown, because it applies with `auto` on or off. *Why not on the tile:* most patches never change it, and the tile keeps its size. |

- Its panel: the scope widget at the top (9 by 5 grid units, bound to `out`, no caption), added in #62, and below it a knob for `window`, a toggle for `auto`, and a knob for `level`. Code: `SCOPE` in `loom-modules/src/display.rs`, `Trigger` in `loom-app/src/live.rs`, and `loom-app/src/probe_menu.rs`.
- The parameters never change the audio. They tell the app how to draw the trace.
- *Why an auto toggle:* most signals trigger well at their midpoint, including envelopes and pitch with an offset, which never cross 0. A fixed level is for picking one edge of a complex wave. A toggle keeps both without a knob that means two things.

### Trigger

- `ScopeTrace` takes the level and the slope from the module: automatic, or the fixed `level`; a rising or falling edge. The rest of the trigger is unchanged: the chosen edge, with hysteresis of a tenth of the history's swing, found to a fraction of a sample. The hysteresis never reaches below the lowest sample on a rising edge, or above the highest sample on a falling edge, so a fixed level near either end of the signal still triggers.
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

### Canvas context menu

**Status:** Agreed, M2 ([#330](https://github.com/brylie/LoomDSP/issues/330), 2026-11-01). It grows the probe menu above into the one canvas menu.

A right-click opens a menu for what is under the pointer. Right-clicking a tile that is not selected selects it first.

| Under the pointer | Items |
|---|---|
| One tile | Copy, Duplicate, Delete, Help. A sub-patch adds Open, Dissolve, and Save to library ([sub-patches](sub-patches.md)). |
| A tile in a selection of several | Copy, Duplicate, Delete, Make sub-patch |
| Empty canvas | Add module here (opens the module browser and places the module at the point), Paste here, Select all |
| An output | Add scope, Add meter, Disconnect all |
| An input with a cable | Disconnect |
| A cable | Add scope, Add meter, Delete cable |

- Each item shows its shortcut on the right. An item that cannot act now is shown dimmed, not hidden. *Why:* the menu teaches the shortcuts, and items that move around are hard to find.
- **Keyboard:** Shift+F10, or the Menu key, opens the menu for the focused control's module, else the selection, else the canvas centre. Up and Down move, Enter acts, Escape closes (NFR-UX-01).
- It stays a pop-up over the window: while it is open, the canvas takes no input, as for the probe menu.
- The control channel can open it and pick an item by name, as it does for the probe menu.

### Acceptance criteria: canvas context menu

- [ ] Each target gets the items in the table, dimmed when they cannot act (`Ui` harness).
- [ ] Paste here places the pasted modules at the pointer; Add module here places the new module there (`Ui` harness).
- [ ] Shift+F10 opens the menu for the selection, and the arrows and Enter pick an item (`Ui` harness).
- [ ] The Add scope and Add meter tests still pass.

### Acceptance criteria: scope module

- [x] The scope module passes its input through, bit for bit. It has a reference test and a block-size check.
- [x] A falling slope starts the trace on a falling edge, and a fixed level of 3 triggers on a signal between 2 and 5 (tests in `crates/loom-app/src/live/tests.rs`).
- [x] Level meter: a peak of 1.0 lights the clip light until it is clicked; the peak hold shows the highest peak in dB; a double-click resets both (`Ui` harness, `crates/loom-app/src/clip/app_tests.rs`).
- [x] A patch that makes infinity at the output counts silenced samples and shows the error message for 10 seconds after the last one; clipping with safety Off shows the warning (test).
- [x] Its parameters are `window`, `auto`, `level`, and `slope`, with the ranges above. Its panel has a knob, a toggle, and a knob for the first three, and `slope` is in the inspector.
- [x] `ScopeTrace` with a fixed level triggers on that level, and runs free when nothing crosses it. With `auto` on it behaves as before.
- [x] Right-clicking an output or a cable opens the menu. A right-drag pans and opens no menu. Right-clicking a tile, an input, or empty canvas opens none.
- [x] Add scope and Add meter, on an output and on a cable, place a module whose `in` is connected to that output, next to it and overlapping no tile. The cable that was clicked stays. The new module is selected.
- [x] Undo removes the new module and its cable in one step.
- [x] Escape and a click outside close the menu and add nothing. While it is open, keys do not reach the canvas.

## Module panels on tiles (M1)

**Status:** Agreed ([#7](https://github.com/brylie/LoomDSP/issues/7)). This section turns the general rules above into what M1 builds for built-in modules. Where the two differ, this section wins for M1.

### Panel declaration ([#67](https://github.com/brylie/LoomDSP/issues/67))

**Status:** Implemented. Code: `Panel`, `Widget`, and `ModuleDescriptor::check_panel` in `loom-core/src/descriptor/panel.rs`.

- A `ModuleDescriptor` has a `panel`: its size in grid units (width and height), and a list of widgets. A module with nothing on its tile has an empty panel.
- Each widget has a kind, a grid position inside the panel, and a binding by name: a parameter for controls, a display value for lights. *Why names:* parameter names are stable IDs (AGENTS.md rule 5), and the M2 panel editor will save bindings the same way.
- M1 widgets. Most are 3 grid units wide. The button is wider, so a performance control can be large. Radio buttons are wider too, so their choice names fit ([#394](https://github.com/brylie/LoomDSP/issues/394)).

| Widget | Size (grid units) | Binds to | Behaviour |
|---|---|---|---|
| Knob | 3 × 3 | Any parameter | A dial with its label below. Uses the parameter's mapping (`ParamSpec::normalize`). |
| Toggle | 3 × 2 | A whole-number parameter from 0 to 1 | Off or on, with its label below. |
| Radio buttons | 6 × (choices + 1) | A whole-number parameter with [choice names](#choice-names) | One row per choice name, top = minimum value. Label above. |
| Button | 6 × 3 | A whole-number parameter from 0 to 1, and a different whole-number `mode` parameter from 0 to 1 | A large button. Its fill shows the bound parameter. Gestures are under [Interaction](#interaction-68). |
| Light | 3 × 2 | A display value | Brightness is the value clamped to 0 to 1. Label below. |

- **The registry refuses a panel that does not fit its module:** a binding to a parameter or display value the module does not have, a toggle, radio, or button on a parameter that does not match it (radio buttons need choice names), a button whose mode is missing, out of range, or the same parameter, a widget outside the panel, or two widgets that overlap. *Why:* a third-party module gets the same checks as a built-in one, at registration, not as a broken tile later.
- **Built-in panels:** a knob for every parameter except the noise seed, MIDI Note's channel and bend range, and Button's `mode`, which stay in the inspector. Button's `on` is the button, not a knob. MIDI Note and Voice Assigner each have a light for their gate, and Envelope a light for its level. Clock Divide and Phase Trigger have a [trigger light](#lights-for-one-sample-triggers) on their output.

### Display values ([#69](https://github.com/brylie/LoomDSP/issues/69))

**Status:** Implemented. Code: `loom-core/src/display.rs`, `Module::displays`, `Controller::display`.

- A descriptor lists its display values as `DisplaySpec`s ([text](port-and-parameter-text.md#display-values)), at most 4. A module writes them in `Module::displays`, which the engine calls once after each block for every module that has any.
- They travel in a **latest-value table**: 4 atomic floats per module slot, allocated once when the engine is created. The engine stores, the UI loads whenever it draws. Nothing waits, nothing allocates, and the table can never fill up. *Why not the meter queue:* only the latest value matters for a light or a step position, as [telemetry.md](telemetry.md) notes for this kind of data.
- When a module is added to a slot, its values are set to 0 first, so a new module never shows the values of the one that was there before.
- The UI reads them by module ID and index through the `Controller`.
- Display values never change the audio. *Why:* determinism (AGENTS.md rule 3).

### Tile layout ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `ModuleView::width`, `widget_bounds`, and `tile_size` in `loom-canvas/src/scene.rs`.

- **The panel sits between the port columns.** Each side has a port label column 4 grid units wide. A longer label may grow inward into empty space on its row ([Labels and hover text](#labels-and-hover-text)). A label that still does not fit is cut with "…", and the port's hover text shows it in full.
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
- **Keyboard:** with a module selected, Tab moves focus to its next control and Shift-Tab to the previous one. Up and Right raise the value by 1% of the range, Down and Left lower it; with Shift, 0.1%. Whole-number parameters step by 1. Space or Enter toggles or picks the next option. A focused control that uses Space acts on it and does not toggle Run. A knob, a fader, a number, an XY pad, and a range from outside have no use for Space, so it passes on to Run ([Keyboard without a mouse](#keyboard-without-a-mouse), [clock.md](clock.md#controls-live-on-the-transport-module)). Escape clears the focus. A control focused with Tab has a visible ring (NFR-UX-01). The ring goes around the control itself, not its caption, so the value shown there stays clear. A click or drag also focuses the control, so the arrow keys work after it, but shows no ring, as `:focus-visible` works on the web. *Why:* after a mouse gesture the pointer already shows where the user is, and a ring around the whole cell cut through the value text ([#92](https://github.com/brylie/LoomDSP/issues/92)).
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
- **Framing keeps labels readable** ([#455](https://github.com/brylie/LoomDSP/issues/455)). Opening a patch, and framing it with F or Home, fit the whole patch with a 40 px margin, but never zoom out past the first zoom where port labels show (7.5 / 9, about 83%), and never zoom in past 100%. A patch that does not fit at that zoom shows its left edge at the margin, where automatic placement puts its sources, and its top edge too if it is also too tall; the user pans to the rest. *Why:* fitting a large patch made every label a bar, so a new user opened an example and saw no words on its controls. The signal starts on the left, so the left edge is where reading starts. `--screenshot` without `--zoom` still fits the whole patch, because its images are overviews.

### Redraw ([#68](https://github.com/brylie/LoomDSP/issues/68))

**Status:** Implemented. Code: `ControlGeometry` in `loom-canvas/src/controls.rs`, `loom-app/src/lights.rs`.

- **Control values have their own GPU buffer.** Changing a value rewrites only that buffer, never the tiles, ports, or cables. *Why:* ADR-0010's rule that GPU data for the patch is rebuilt only when the patch changes.
- Lights read the display table about 30 times a second, and at once when the lights on screen change, but only while a module with lights is on screen at a zoom where lights are drawn. A frame is drawn only when a shown brightness, or a meter's place in its range, changes by at least 1/256. Past the end of its range, a meter's bar stops but its text does not, so steps of the same size still count.
- The reads run on their own thread, which sends the UI a message only when a light changes. *Why:* in Iced every message redraws the window, so a plain 30 Hz timer message would draw 30 frames a second even when nothing moves. The canvas reports which modules with lights are on screen, so the app watches only those.
- **A pan or zoom does not disturb a light** (Implemented, M2, [#619](https://github.com/brylie/LoomDSP/issues/619)). A light that stays on screen keeps its fade and does not flash again. A light that comes into view is read at once, so it shows its current value without waiting for the next read. A trigger light that comes into view starts dark, and flashes at the next trigger. *Why:* the player moved the view, not the patch, so what a light shows must not change because of it. *Why start dark:* the latch holds any trigger since the light was last read, however long ago. A divider that fires every 8 seconds would flash as the player pans back, as if it had just fired, and mislead a player who uses it to find the bar. Starting dark can lose only a trigger in the instant between the pan and the read, which the player could not have seen.
  - **At most one thread per engine** (Implemented, M2, [#619](https://github.com/brylie/LoomDSP/issues/619)). Code: `lights.rs`, `watcher.rs`, `App::watched_lights` and `App::set_visible_displays` in `subscriptions.rs`, `App::sync_watchers` and `EngineWatchers::light_watch` in `watching.rs`. The light watch holds what its thread needs for its whole life: the engine's `DisplayReader`, the engine's generation, and the shared list of lights. Its key, the hash that `watcher::keep` compares, covers only the generation. Whether any light is shown is whether there is a watch at all: with no light on screen, `App::watched_lights` gives none, `keep` drops the handle, and the thread stops. When a light comes back, a new thread starts. *Why:* a pan or zoom then changes what the thread reads, not which thread runs, so it starts no thread. A watcher lives as long as what it watches ([UI layer](ui-layer.md), section 2, rule 7), as the telemetry monitor does. *Why the list travels with the reader:* `DisplayTable::read` indexes its slots with checked slice indexing (`loom-core/src/display.rs`), so a list built for another engine fails in one of two ways. A slot past the new table panics and ends the thread. A slot inside it passes the owner check when the new engine reused the module's number (numbers start at 1 again in each engine), and its `swap(0)` takes the wrong module's trigger. Keeping the list and the reader in one watch, which a new engine replaces whole, makes both impossible.
  - **The list is a watched value.** A small type in `watcher.rs` holds a value behind `Arc<(Mutex<_>, Condvar)>`: one mutex and one condition variable. Under the mutex it keeps the value and a version number, and nothing else. It outlives any one watcher (hiding every light and showing one again starts a new watcher on the same value), so it has no closed flag of its own. `set` replaces the value only when it differs, then bumps the version and notifies the condition variable. An equal `set` changes nothing, not even the version, so a test can tell from the version alone, with no timing. `get` clones the value out with the version it read. The values compared are finite, because loading a patch clamps or replaces a parameter that is not (`loom-patch/src/flatten.rs`), and every edit passes through `ParamSpec::clamp` in `Controller::set_param` (`loom-core/src/descriptor/param.rs`), so an equal value never looks like a change; the type's doc says so. Every lock uses `unwrap_or_else(PoisonError::into_inner)`, and is held only to clone or to replace the value, never across a read of the display table or a wait. The telemetry monitor's watch list (`Monitor::watch`, `Monitor::watched`) is held by the same type, so there is one copy of this code. The list is kept with the engine's watchers (`EngineWatchers::light_watch` in `watching.rs`), tagged with the engine generation it was made for. A device change builds a new engine but keeps the watchers, so the tag is what tells the list is stale: a new generation (New, Open, or a device change) gets a new, empty list, and no list is ever read with another engine's reader. It has one entry per light on screen: the light (module and widget), its display source, its range, and its readout range. After each update, `App::sync_watchers` sets the list the app built, then calls `keep`. Neither the UI thread nor the light thread is the audio thread.
  - **The thread waits on the value, not on a timer.** It waits on the value's condition variable for up to 33 ms, and only while the version is the one it last read and its own handle is not closed (the thread's own `Flow.closed`). Both are checked under the value's lock. So a `set` made while the thread is reading the table is not lost: the version has moved, and the next wait returns at once. A new watcher starts with no version seen, so its first wait returns at once and it reads the table straight away. `Watcher::start_on` registers the watched value with the handle. Dropping the handle stores `Flow.closed`, then locks the value's mutex, notifies its condition variable, and unlocks, and then notifies `Flow.room`. The closed flag belongs to the handle, so a new watcher on the same value is never stopped by an old one's drop, and the old thread always ends. **Lock order:** the drop takes the value's lock and `Flow.room`'s one after the other, never one inside the other, and the thread never holds the value's lock while it sends (a send takes the ledger's and `Flow.room`'s locks), so no lock-order cycle can form. Each turn of the loop goes: wait, check closed, read the table, compare, and send. A thread whose handle dropped therefore never reads the table again once its wait returns. A light that comes into view is read in the same instant, and a dropped handle ends the thread at once instead of after its interval. The monitor's thread waits the same way, for up to its own interval.
  - **Each light's state is kept by light, not by its place in the list.** The thread keeps each light's last shown step, its fade, and its readout hold in maps keyed by the light, and prunes the maps to the current list at every read. A light that stays on the list keeps all three, so its fade goes on and its readout does not change early. A light new to the list has no last step, so its first reading always counts as a change and is sent at once. A light that leaves the list is forgotten, so it starts again if it comes back. A light whose display source changed while it stayed on the list starts again too. When the canvas reports that a trigger light left the screen (`CanvasEvent::VisibleDisplays`, handled by `App::set_visible_displays`), the app sets the value it keeps for it to dark. It sets a trigger light new to the list dark too. So the light does not draw lit for a frame when it comes back, before its first reading. A reading the thread sent before the light left can still be in the queue, so the app ignores a reading for a trigger light that is not on the current list (`App::take_lights`). A level light takes every reading, and keeps the last value it was sent, the best guess until the next one. Neither is drawn while it is off screen.
  - **A trigger light new to the list starts dark.** Its first read takes the latch, as every read must, and the thread throws that value away: the light shows 0 and has no fade. Its next read, at most 33 ms later, shows only triggers since then. With the wake above, the only trigger lost is one that fired between the pan and that first read.
  - **One reader for trigger cells.** A trigger cell is read with `swap(0)`, and the table has one reader ([lights for one-sample triggers](#lights-for-one-sample-triggers), "Two readers are not supported"). A pan that started a new thread would leave the old one reading until its wait ended, so for a moment two threads would take the same cells, and one of them could swallow a flash. With one thread per engine, no pan starts a second reader. One overlap is left: when every light leaves the screen and one comes back at once, the old thread may still be in its last read while the new one starts. It is rare, short, and costs at most one flash.
  - **`--frames`** reads every light once, with no thread (`run_engine` in `snapshot.rs`). It reads the list the app built with the engine's reader, as one call. That read keeps the latch: a trigger light that fired during the run shows lit, unlike a light new to the list in the window, which starts dark. *Why:* the snapshot shows what the run left, and a trigger is the thing a check after `--frames` looks for.
  - *Tests:* in `lights/tests.rs`, with the time passed in, the per-light state across a list change: a light that stays keeps its fade, a new light's first reading is sent, a trigger light new to the list starts dark, a light that leaves is forgotten, a light whose source changes starts again (`a_light_whose_source_changes_starts_again`), and a late change for a light that left keeps it forgotten (`a_late_change_for_a_light_that_left_keeps_it_forgotten`). In `watcher/tests.rs`, the watched value: `set` with an equal value leaves the version as it was, a changed one bumps it and wakes a waiting thread at once, a `set` made between two waits makes the next wait return at once, and a dropped handle ends the wait at once. In `lights/tests.rs`, `--frames`'s one-shot read shows a trigger that fired during the run as lit. In `watching/tests.rs`, a zoom that changes which lights are visible keeps the same handle key; hiding every light drops the handle, and showing one starts a new one. With the real thread (`app.watching = true`, a waker on a queue): a light brought into view is sent by the same thread, and after Open, New, and a device change the new engine's thread sends the light's value. Hide and show: hide every light, then show one. The old thread has ended, since the shared list's `Arc` strong count falls back to one thread's handles, and the new thread sends a reading. On the UI side (`Ui` harness tests): a trigger light panned away mid-fade is dark when it comes back, before its first reading, and a level light keeps its last value; a reading delivered after its trigger light left the screen is ignored, so the light comes back dark (`a_late_reading_for_a_trigger_light_that_left_is_ignored`).
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
- [x] The MIDI Note gate light is lit while a key is held. The Envelope light follows its level. The tests in `loom-modules/tests/panels.rs` play a note through `subtractive.ron` and read the display values the lights draw, at block sizes 1, 77, and 512.
- [x] Zoomed far out, controls and lights are not drawn. At middle zoom, they have no text.
- [x] Text hidden by zoom is replaced by a bar at the place and width of its text: port labels and captions at middle zoom, titles and port labels far out.
- [ ] Manual, on the reference laptop: with `--generate 300 --bench`, the frame rate far out with bars matches the rate without them.
- [x] Lights add no frames while their brightness stays the same: the light watcher sends nothing until a shown level changes, and runs only while lights are on screen.
- [ ] Manual, on the reference laptop: knobs, the toggle, and lights look right at close and middle zoom, and a cut port label shows in full on hover.

The whole app redraws only when something visible changes ([#90](https://github.com/brylie/LoomDSP/issues/90)): see [Audio load and dropouts](#audio-load-and-dropouts-in-the-stats-bar) for the stats bar.

## Level meters and audio load (M1)

**Status:** Implemented ([#9](https://github.com/brylie/LoomDSP/issues/9)). Code: `WidgetKind::Meter`, `loom-modules/src/display.rs`, `loom-app/src/monitor.rs`, `live.rs`, `alerts.rs`. Requirements: FR-LIB-17, FR-VIS-03 (the level meter module, brought forward from M2), FR-PERF-04.

### Meter widget

- A fifth M1 widget, 3 × 6 grid units. It binds to one of its module's **outputs** by name. The registry refuses a meter bound to an output the module does not have.
- It shows the output's level from -60 dB (empty) to 0 dB (full), bottom to top: a bar for the RMS level and a thin line for the peak. Both rise at once and fall by the whole range in about a second. The bar is green below -12 dB, amber up to -3 dB, and red above. Its label is below it.
- A meter is a display: pressing or dragging it selects or moves the tile, as with a light.
- **Clip light and peak hold** (Implemented, M2, [#98](https://github.com/brylie/LoomDSP/issues/98)). Code: `crates/loom-app/src/clip.rs`. Above the meter, a clip light turns on when the peak reaches 1.0 and stays on until it is clicked. Below it, a readout shows the highest peak since the last reset, in dB to one decimal, and `--` before any signal. Double-clicking the meter resets both. *Why the light says "would clip":* cables are never clipped (ADR-0007). A peak of 1.0 or more on a cable only clips if it reaches the output, so the hover text says "Would clip at the output". The clip light also has a shape, not only a colour (NFR-UX-03): a hollow ring when off, a filled square when on.
  - **Which meters.** Only a meter bound to an output. A meter on a display value, such as gain reduction, has neither, because it does not show a signal's level. A stereo meter has one light and one readout, using the higher side.
  - **Layout.** Both fit inside the meter's 3 × 6, so the bar is shorter. The readout takes the caption's row, so such a meter has no caption, and hover text keeps its label. At middle zoom the light keeps its shape and the readout becomes a bar. Far out, neither is drawn.
  - **Click.** The light clears on release, if the pointer did not move. The press also selects the module, as on any display. A drag from the light moves the tile and leaves it lit. A click elsewhere on the meter changes nothing. At far zoom neither is drawn, so a click or double-click there only selects the module and resets nothing.
  - **Keyboard.** A display takes no focus, so the Level Meter's inspector has a **Reset clip** button, which resets both.
  - **Two meters on one output** (Implemented, M2, [#637](https://github.com/brylie/LoomDSP/issues/637)). Each meter has its own clip light and peak hold. Resetting one clears only that meter. A clip that arrives just before or after the reset still lights the other one, and still raises its readout. *Why:* the player resets the meter they are looking at. A reset there must not hide a clip from the other meter, because catching a short clip is what the light is for. No built-in module has two meters on one output, but the panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)) can place them. The inspector's **Reset clip** button resets every meter on the module. Resetting one meter by keyboard comes with the panel editor, because only the panel editor can place a second meter on an output.
    - **How** (Implemented, #637). Code: `Peaks` in `clip.rs`, `WatchList::peak_resets` in `monitor.rs`, `App::peak_resets` and `App::take_peaks` in `lib.rs`. Probes stay per output, so two meters on one output still share one probe and one stream of readings. The telemetry thread keeps its peak news per meter (module and widget), not per probe. `WatchList::peak_resets` maps each meter on screen to its probe and its own reset count (`ClipHolds::resets`), with no sum. Each reading goes into the news of every meter on its probe. A new count for one meter clears only that meter's news, and the news it sends from then carries that count (`PeakNews::after`). Each piece of news also names the probe it was read from (`PeakNews::probe`). A meter that leaves the list is forgotten, and a meter whose probe changed starts with no news.
    - `Update::peaks` carries each meter's news by meter. `App::take_peaks` drops news whose `after` is not the meter's own count now, because it left before that meter's latest reset. It drops news whose probe is not the meter's probe now, because it left before the probe changed (the meter left the screen and came back, for example), and news for a meter no longer on screen. It gives the rest to that meter's hold alone. So news for the other meter, on its way during the reset, still lights it. One meter on an output works as before. The `--frames` run uses the same `Peaks`.
    - *Tests:* in `clip/tests.rs`, two meters on one probe: a reset of one clears only its news, and the other's news keeps its count. In `clip/app_tests.rs`, two meters on one output, a reset of one while a clip is on its way: the other still lights and its readout rises, and the reset one stays dark. Also in `clip/app_tests.rs`, `a_peak_from_an_old_probe_is_dropped`: a meter leaves the screen and comes back while a clip from its old probe is on its way, and it stays dark.
  - **What counts.** A peak of 1.0 or more, or a peak or RMS that is not a finite number. A value that is not finite lights the light and never changes the readout.
  - **How long it lasts.** The UI thread owns the state, by module and widget. It survives panning away, zooming out, and a device change. It clears when a patch opens and on New. It is not saved. While a meter is off screen it has no probe, so a clip then is not caught, nor is one read just before it leaves whose news was still on its way when it came back. *Why:* a probe costs engine work, and a meter nobody sees needs none.
- *Why levels fall over a second:* a short peak stays visible long enough to see, and the bar does not flicker.

### Level meter module

- **`loom.display.meter`**, category Display. One audio input `in` and one output `out` that passes `in` through, sample for sample. Its parameters are `view` and `window`. Its panel holds a meter and a level history, both bound to `out`, and the history shows only with `view` on Meter and history.
- A stereo output shows both sides ([stereo](stereo.md), rule 22).
- M2 adds a level history view, with `view` and `window` settings in the inspector ([panel widgets, Level history](panel-widgets.md#level-history)), and a **Reset clip** button in the inspector.
- *Why a pass-through output:* probes read outputs, and the module can sit inline in a signal path or hang off an output, without changing the sound.

### How levels reach a meter

- While a meter is on screen at a zoom where it is drawn, the app keeps a `Meter` probe on its output ([telemetry](telemetry.md)). When the meter leaves the screen, the probe is removed at the next commit. *Why:* a probe costs engine work, and a meter nobody sees needs none.
- Telemetry is drained on a background thread every 33 ms, or every 16 ms while a scope, a spectrum, or an XY scope is on screen, not on the UI thread. The thread keeps each probe's peak and RMS levels and sends the UI a message only when a level changes by at least 1 of 256 steps, or when the highest raw peak since the last reset rises by at least 0.1 dB or reaches 1.0. *Why:* as for lights, every message redraws the window, so a silent patch with meters on screen must send none.
- Probes stay view state: not saved, and they never change the sound.

### Audio load and dropouts in the stats bar

- The stats bar always shows the audio load (the latest callback as a share of its time budget, and the highest so far) and the dropout count: callbacks that overran their buffer, plus errors reported by the audio device.
- An overload notice from the device that comes before the stream's first callback is not a dropout. No audio was rendered yet, so none was lost. macOS sends one as a stream starts with a very small buffer (32 frames), while the device switches to it. *Why:* counting it turned the count red each time a performer chose a small buffer, for a glitch that never happened. A count that cries wolf gets ignored. (Found on BlackHole 2ch, 2026-10-24, [#26](https://github.com/brylie/LoomDSP/issues/26).)
- Load is shown normally below 70%, in amber from 70%, and in red from 90%. The colour follows the highest load since the last tick, and the "max" is the highest in the last 10 seconds ([M1 additions](#audio-load-colour)).
- The dropout count is red for 10 seconds after it grows.
- **Redraw.** A thread reads the audio stats four times a second and wakes the UI only when the bar would change: the load or its maximum to the shown 1%, the dropout count, the end of the red time, the telemetry drops, or an output warning starting or ending. The load number changes at most twice a second. *Why:* in Iced every message redraws the window, so a timer that refreshed the bar drew two frames a second in an idle patch ([#90](https://github.com/brylie/LoomDSP/issues/90)).
- *Why:* a performer needs a warning before a problem is audible (FR-PERF-04), and must notice when one just happened. A count that stayed red forever would soon be ignored.
- **Silenced and clipped output** (Agreed, M2, [#130](https://github.com/brylie/LoomDSP/issues/130), 2026-11-01). The engine counts output samples it turned from NaN or infinity into silence, and samples it clipped, to -1 to 1 or at the safety stage's input limit, in two atomics in `EngineStats` (an atomic add on the audio thread, nothing else). The stats bar shows **Output: bad samples silenced** in the error style, and **Output clipped** in the warning style, each for 10 seconds after the last such sample, like the dropout count. Hover text gives the counts since the engine started. The watcher thread owns the timing and tells the UI when a warning starts or ends, and the warnings clear when the stream or engine changes. Each stream gets a new engine, so a count already there at the watcher's first read is new, and it shows. *Why the watcher:* it wakes the UI only when the bar would change, so a second timer in the UI would end the warning while bad samples continue. *Why:* since #123 a patch that makes NaN just goes quiet, and the player cannot tell why. With the output limiter on, clipping is rare, so the clip message mostly appears with output safety Off, or when a finite sample comes in above the safety stage's input limit, as a runaway feedback loop's does ([ADR-0037](../adr/0037-finite-inputs-hold-at-the-largest-finite-value.md)). *Why that counts:* the clamp at that limit is clipping, and the limiter would otherwise hide it, so the loop would go quiet with no warning.

### Acceptance criteria: level meters and audio load

- [x] The registry refuses a meter bound to an output the module does not have.
- [x] The meter module passes its input through, bit for bit. It has a reference test and a block-size check.
- [x] A meter on screen gets a probe on its output. When it leaves the screen, or the canvas zooms far out, the probe is removed.
- [x] Levels map -60 dB and below to empty and 0 dB to full. They rise at once and fall by the full range in a second. The peak line is never below the RMS bar.
- [x] The telemetry thread sends nothing while the shown levels stay the same.
- [x] Load below 70% is normal, from 70% amber, from 90% red. The dropout count is red for 10 seconds after it grows.
- [x] An overload notice before a stream's first callback is not counted as a dropout (`an_overload_before_the_first_callback_is_not_a_stream_error`). Checked on macOS: BlackHole 2ch at 32 frames shows `dropouts 0`.
- [x] A meter module after the VCA in `subtractive.ron` rises when a note plays and falls after it stops. A meter on the filter's output stays up with no note, because the saw runs all the time and only the VCA is gated. Tested twice: as meter readings in `loom-modules/tests/panels.rs`, and as drawn levels and colour bands in `loom-app/src/live.rs`. (This was a manual check that said "after the filter"; [#92](https://github.com/brylie/LoomDSP/issues/92) found that wording wrong.)
- [x] Two meters on one output keep their own clip light and peak hold: resetting one while a clip is on its way leaves the other lit, with its readout raised, and the reset one dark. The inspector's Reset clip resets both ([#637](https://github.com/brylie/LoomDSP/issues/637)). Covered by `two_meters_on_one_probe_keep_their_own_news` in `clip/tests.rs` and `two_meters_on_one_output_keep_their_own_clips` in `clip/app_tests.rs`.

### Out of scope

- Stereo meters, for M1. ("Add meter" came with "Add scope": see [Add scope and Add meter](#add-scope-and-add-meter).)
- Peak hold numbers and clip indicators, for M1. They are [in M2](#meter-widget).
- Spectrum and scope displays, for M1. See [Scope module](#scope-module) and [Spectrum](panel-widgets.md#spectrum).

## Labels and hover text

**Status:** Implemented, except the manual check below. Code: `loom-app/src/scene.rs` (labels), `loom-canvas/src/hover.rs` (timing and placement), `draw_hover_text` in `loom-canvas/src/widget/hover_text.rs`, and the inspector in `loom-app/src/inspector.rs`. Builds on [port and parameter text](port-and-parameter-text.md) ([ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md)): every port and parameter has a `label` and a one-sentence `description`. This section says where the label and description appear.

### Goal

A user zooms in on Clock Divide. The knob says "Division", not `n`. They rest the mouse on it and read "Fires on every Nth trigger." They never leave the canvas to learn what a control does (product principle 11).

### Where each text shows

| Place | Showed before | Shows now |
|---|---|---|
| Port label on a tile | name | label, grown into empty space on its row, then cut with "…" |
| Caption of a knob, toggle, button, or radio group | parameter name | parameter label, grown into empty space on its line, then cut with "…" |
| Option of a radio group | choice name | choice name, cut with "…" to fit the widget ([#394](https://github.com/brylie/LoomDSP/issues/394)) |
| Caption of a meter | output name | output label |
| Caption of a light | display value name | display value label ([#118](https://github.com/brylie/LoomDSP/issues/118)) |
| Inspector parameter row | name | label |
| Hover text on a port or control | full name, only when the label is cut | label and description, always |
| Hover text on a light or meter | none | label, description, and value ([#118](https://github.com/brylie/LoomDSP/issues/118)) |

- **Names stay out of sight, except in hover text.** The hover text shows the stable name below the description, dimmed, when it differs from the label other than in letter case. So `n` shows under "Division", but `cutoff` does not repeat under "Cutoff". *Why:* names are what patch files and `loom modules` use, so a user who edits a RON file can find the port. Everywhere else, the label is the better text.
- **Captions and radio options are cut, never wrapped.** A label that does not fit is cut with "…", and the hover text shows it in full. Radio buttons are 6 units wide, so no built-in choice name is cut; a test checks this for every module. A longer name from a third-party module is cut and shows in full in the inspector. Widths are estimated per character, with narrow letters counting less. *Why:* the grid layout of tiles does not change with text length, and estimating keeps text layout out of every frame.
- **Labels use empty space before they are cut** ([#455](https://github.com/brylie/LoomDSP/issues/455)). A port label may grow inward from its column, along its row, up to 3 units before anything the panel draws there, and never past the middle of the tile. A caption may grow on both sides along its line: up to 3 units before another widget's drawn part (a knob's ring and range arc, a radio's buttons), halfway into a gap it shares with other text less 1.5 units, up to 3 units before a port label, and no further than the port markers' inset. A caption that fits is centred on its widget. One that fits only off centre moves the least it needs toward the free side, and its centre stays over its widget. A shape that reaches 1 unit or less into a line of text does not count, because the line's box is taller than its letters. Labels grow only within the tile's grid size, so tiles never change size with text. A test checks every built-in module: no port label or caption is cut, and no two pieces of text are closer than 1 unit. *Why:* "Resonance" and "Highpass" were cut on the filters although empty space sat beside them. Where no rule finds room, the panel layout changes: SVF's panel is 8 units wide with centred knobs, and MIDI Controller's Smoothing knob sits under Channel.

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

## M1 slice

What M1 builds. The rest of this spec is M2.

### Acceptance criteria

- [ ] Module panels on tiles: see [their acceptance criteria](#acceptance-criteria-module-panels) above.
- [x] A scope module fed by a 440 Hz oscillator shows a stable trace (`ScopeTrace::offset`, `trigger_is_stable_across_frames`; the subtractive example's scope, `docs/images/readme-subtractive.png`).
- [x] "Add scope" on a cable places a connected scope module next to it. Undo removes it.
- [ ] Cable opacity never depends on the signal. A test checks that the cable geometry is the same with and without telemetry.
- [x] With no displays on screen and no input, the app does not redraw. An empty patch with `--no-audio --measure-idle --bench-seconds 3` showed `fps=0.0` after startup and an unchanged cumulative frame count from the second line on ([#90](https://github.com/brylie/LoomDSP/issues/90)). A patch with visible transport lights or scopes redraws as those displays change.
- [ ] Zoomed far out, controls and displays are not drawn.
- [ ] Reference laptop, full window: 60 fps with four scopes on screen. Record CPU and GPU in the benchmark doc.

### Out of scope for M1

- The panel editor, sub-patch panels, and front panels (M2, FR-PANEL-*).
- Step grid and graph editing on the tile. They need module data (OQ-17, M2).
- XY pad, themes, custom skins (M2).
- Restyling built-in panels.
- Docked views at the window edge. Displays are modules.

## M1 additions

**Status:** Agreed. Each subsection names its issue. These close the gaps that building the M1 panels found. Choice names, display value text, lights for one-sample triggers, the clock mark, and signal activity are Implemented.

### Choice names

([#160](https://github.com/brylie/LoomDSP/issues/160).) **Implemented.** Code: `ParamSpec::choices` and `ParamSpec::check_choices` in `loom-core/src/descriptor/param.rs`, `ParamControl` in `loom-app/src/inspector.rs`. A whole-number parameter can name its values.

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

([#103](https://github.com/brylie/LoomDSP/issues/103).) **Implemented.** Code: `TransportSignal`, `PortView::follows_transport`, and `ModuleView::clock_mark_centre` in `loom-canvas/src/scene.rs`, the shapes in `loom-canvas/src/gpu.rs`, and `transport_default` in `loom-app/src/scene.rs`.

- An input that has no cable and a default of bar phase, beat phase, or playing shows a **clock mark**: a small ring with two short hands, drawn between the port marker and its label, in the module's accent colour at reduced strength. It is the same mark for all three; the hover text tells them apart ("Follows the bar phase while nothing is connected").
- It is hidden when a cable is connected, and returns when the cable is removed.
- It shows at the zoom where port labels show, and as a plain dot at the middle zoom. It is not drawn at far zoom.
- `PortView` gets `follows_transport: Option<TransportSignal>`, set by the app from the descriptor's `PortDefault` and the cable state. The mark is part of the scene's GPU data, which changes only when cables change (ADR-0010).
- It is in the UI snapshot as a port field, so the control channel can find it.
- Settled while building it: the mark is a ring 7 world units across with a hand up and a shorter hand to the right. Its centre is 11 units from the tile edge, and the label moves 8 units right to make room, so a label moves when a cable is connected or removed. The colour is the accent mixed halfway towards the dim text colour, at 80 percent opacity. *Why:* the accents are muted for headers, and at reduced strength alone the mark did not read on the tile. The hover text adds the line after the description. The mark shows whatever the module's mode, for example on the LFO in free mode, because the default is part of the static descriptor (clock.md, Default inputs).

### Audio load colour

([#102](https://github.com/brylie/LoomDSP/issues/102).) This replaces the first and third bullets of [Audio load and dropouts](#audio-load-and-dropouts-in-the-stats-bar).

- `CallbackStats` keeps `peak_since_read`: the highest load of any callback since the last read. The stats bar reads and clears it once per tick, and **colours the load by that peak**, not by the latest callback. *Why:* a spike shorter than one tick is the warning of a coming dropout, and the latest sample almost never catches it.
- The number shown is the load of the latest callback, as now.
- **"Max" is the highest load in the last 10 seconds,** not since the app started. It is a ring of tick peaks. *Why:* after one spike, a since-start maximum says 95% for the rest of the night and stops being useful.
- Amber from 70%, red from 90%, as now. The dropout count stays red for 10 seconds after it grows.
- If [#90](https://github.com/brylie/LoomDSP/issues/90) changes how the bar updates, the window is "per tick", not "per 500 ms".
- Test: feed one callback at 95% among thousands at 20% and see the bar turn red for that tick; after 10 seconds of quiet, the max falls.

### Keyboard without a mouse

([#101](https://github.com/brylie/LoomDSP/issues/101).) Closes the gap that nothing selects a module by key. Keys have four owners, tried in this order, and the first one that has a use for the key takes it:

1. **A text field or text box with focus.** The owners below get no key press, wherever the pointer is, whether or not the field uses the key ([Keys in a text field](panel-widgets.md)). *Why:* a space in an author's name must not stop the music, and a person typing should never change the patch behind the field. Escape leaves the field, and the other owners then have the keys again. Where a view has several text fields, Tab and Shift-Tab move between them in reading order and wrap at the ends: Authors and Tags in the patch form, and search and Author in the patch browser. Focus never leaves the view, so it cannot land behind an overlay. In any other field, Tab closes the field and moves the canvas's focus.
2. **A focused panel control** (knob, toggle, radio, select list, button).
3. **The canvas** (selection, movement, clipboard, undo).
4. **Global keys** (Space for Run).

The table below applies when no text field has focus.

| Key | Panel control focused | Module selected | Nothing selected |
|---|---|---|---|
| Tab, Shift-Tab | Next or previous control of the module; after the last, the next module | Next or previous module in reading order | Selects the first or last module |
| Enter | Acts on the control (toggle, press). On a one-number control, opens the field for [typing a value](panel-widgets.md#typing-a-value). In MIDI learn mode, arms it | Focuses the module's first control | Nothing |
| Space | Acts on a toggle, radio buttons, a segmented control, a select list, or a button, and on a part inside a range. A one-number control, an XY pad, or a range from outside does not take it | Toggles Run | Toggles Run |
| Arrow keys | Change the value ([Interaction](#interaction-68)) | Move the selected modules one grid unit; Shift moves 10 | Nothing |
| Escape | Clears control focus | Clears the selection | Nothing (after the other uses of Escape: a drag, then pinned lines) |

- **Reading order** is top to bottom by tile top, then left to right, then by module ID. It is the same order as Tab in the module browser's results.
- Tab selects the module, so the inspector and the highlight follow. Focus on a control also selects its module.
- `focus_keys` no longer swallows Space and Enter for a knob: a key with no use passes on to the next owner. *Why:* a swallowed key is a key that does nothing.
- Moving by keys is one undo step per key press group: repeats while the key is held, and presses within 500 ms of the release, join one step.
- Test: select a module, focus a knob, change its value, and leave, by keys alone.

### Signal activity on ports

([#63](https://github.com/brylie/LoomDSP/issues/63).) **Implemented:** `take_activity` in `loom-app/src/monitor.rs` measures the state, the canvas draws the marker from its overlay buffer, and the UI snapshot shows `carrying` on each port. Cables stay still ([Cables do not animate](#cables-do-not-animate)). Ports show whether they carry a signal.

- **Two states:** *carrying* (a filled marker in the signal colour) and *silent* (an outlined marker). Input ports show the state of the output feeding them. Earlier connected ports were filled even while silent, so the outlined state makes activity visible.
- **Carrying** means the output's largest absolute value in the last tick was above 0.0001 (−80 dB). *Why:* DC offsets and constants count as signal, since a Constant of 0.5 is a live control. Exactly 0 and noise below −80 dB are silent.
- **Where it is measured.** With the activity display on (Preferences, default on), `Live::sync` keeps a `Meter` probe on every output that has a cable ([telemetry](telemetry.md#in-the-desktop-app)). The telemetry thread already keeps the peak since the last drain. No new engine path is needed. One-sample triggers count, because the peak covers every frame of every block. When the display is off, no probes are added. *Why reuse meters:* the cost is already measured, and the code is built and waiting (`Live`).
- **Rate.** The UI reads the peaks 5 times a second. A port becomes *carrying* at once, and *silent* after 2 reads (400 ms) with nothing. *Why:* gates and slow envelopes would flicker otherwise.
- **Redraw.** The markers are drawn from a small buffer of port states, like the panel values buffer, so a change never rebuilds tiles, ports, or cables (ADR-0010). A state change draws one frame. No change, no frames.
- Cables do not change brightness. Hover focus rules are unchanged.
- At middle and far zoom the markers are not drawn (they are port-sized).

### Drawing the scope trace

([#62](https://github.com/brylie/LoomDSP/issues/62).) **Implemented.** Code: `WidgetKind::Scope`, `loom-canvas/src/trace.rs` (`ScopeTraces`, the `vs_trace` and `fs_trace` shaders), `ScopeTrace::canvas_trace` in `loom-app/src/live.rs`, and the telemetry thread in `loom-app/src/monitor.rs`, which builds the traces and wakes the UI only when a drawn line moves by more than half a pixel. Building it settled one reading: the scope widget fills the panel area below the scope module's controls, and its frame is drawn with the controls, not the tile, so it also costs nothing per frame. The rules marked #730 and #637 are Implemented.

- **Where.** The scope widget fills the panel area of the scope tile. Its frame is drawn with the tile; the trace is a line strip clipped to the frame.
- **Data.** Each frame the app copies the points that fit the module's `window` into one small vertex buffer (at most 1024 points: more are skipped by stride, keeping the min and max of each group, and the first and last samples). It uploads that buffer and nothing else. Tiles, ports, and cables are not rebuilt (ADR-0010).
  - **The line reaches past both edges** (Implemented, #730). A triggered trace takes `window + 2` samples from the sample before the trigger (`find_trigger` looks for a start that leaves `window + 2`), so after the shift by `offset` its first point sits at or left of x = 0 and its last at or right of x = 1, and the frame clips both. A free-running trace has no shift, so `window + 1` samples span 0 to 1 exactly. *Why:* with `window + 1` samples a triggered trace stops short of the right edge by `offset`, up to one sample. That end slides on every read, which is a visible jitter at the right edge, and in the architecture review's model it woke the UI on 30% of reads at zoom 2. A long window reduced to groups keeps its first and last samples as well as each group's lowest and highest, with room left for them within the 1024 points, so the reduced line reaches past both edges too. *Why:* a group's lowest and highest can both lie inside the frame, as in a constant last group, and the line would then stop short of the edge. A NaN at either end adds no point, so the first or last sample with a value is kept instead.
- **Steady wave.** `ScopeTrace::offset` shifts the trace by the fraction of a sample between the trigger and the first point, so a periodic wave does not shimmer.
- **Values that are not finite** (Implemented, [#637](https://github.com/brylie/LoomDSP/issues/637)). A NaN sample is skipped. It draws no point, and the line joins the finite samples on each side of it. A window with only NaN draws no line. An infinite sample is pinned to the edge of the frame, as any value past the range is. The middle-zoom level ignores a NaN and shows infinity at full. *Why skip a NaN:* it has no value, so it has no place in the frame, and a point drawn at the centre would look like a real 0 and mislead the player. *Why pin infinity:* it has a sign, and it is the far end of a value that is already past the range. A runaway feedback loop then stays pinned at the edge, instead of vanishing once it reaches infinity. The XY scope does the same with both ([panel widgets, XY scope](panel-widgets.md#xy-scope)). The vertical range follows the peak, so an infinity in the window pushes it to its largest step (±10), and the finite signal looks small until the sample leaves. That is a visible sign that something is wrong. With Auto on, an infinity leaves no level to trigger at, so the trace runs free until it leaves the history. A fixed level still triggers when the infinity lies on the side the edge rises toward (above the level for a rising edge, below it for a falling one). An infinity on the side the edge starts from leaves the level with no arm point, so the trace runs free then too. Cables are never cleaned (ADR-0007), so a feedback patch can put either on a scope's input. A meter on the same output shows both with its clip light ([meter widget](#meter-widget)), and the stats bar warns when one reaches the output. A scope with no points, at the middle zoom or with a window of only NaN, has no accessibility description, as today before its first trace.
  - **How** (Implemented, #637). Every point the canvas gets has a finite x and a finite y. `ScopeTrace::canvas_trace` leaves a NaN out of the full trace. When a long window is reduced to groups, each group picks its lowest and highest sample among those that are not NaN, and a group with none adds no point. Each point keeps its own x, so the line joins the samples on each side of a gap. An infinite sample goes through the clamp to the frame's edge. The trigger search, the range, and the middle-zoom level already ignore a NaN: a comparison with NaN is false, so no crossing is found next to one, and `f32::max` and `f32::min` keep the other value. A trigger whose sub-sample fraction is not finite (an infinity next to the crossing) counts as no trigger: the trace runs free, with an offset of 0, so no point gets a NaN x. With Auto on, an infinite sample in the last second of history leaves no level to trigger at (the halfway level is infinite, or NaN with both signs), so the trace runs free until that sample has left the history. With a fixed level, the arm point is the level less a share of the distance to the lowest sample (`find_trigger` in `live.rs`, sign-adjusted for a falling edge). An infinity on the side the edge starts from makes that distance infinite, so the arm point is infinitely far from the level, nothing arms, and the trace runs free. An infinity on the other side leaves the arm point finite, and the level triggers.
  - The wake check below expects finite points, and says so with a `debug_assert`, so a trace that breaks this fails in tests rather than waking the UI on every read.
  - *Tests:* in `live/tests.rs`, a window with a NaN in it gives only finite points, in the full trace and in a reduced one, and a group made only of NaN adds no point. A reduced triggered step, whose first group's lowest and highest and whose constant last group all lie inside the frame, still has its first point at or left of x = 0 and its last at or right of x = 1, within 1024 points, and with a NaN at either end keeps the next sample with a value (`a_reduced_trace_reaches_both_edges`). An infinity is drawn at the edge, and the level is full. A window of only NaN draws no line. A fixed level with an infinity just before the crossing gives only finite points. A fixed level with an infinity on the far side still triggers.
- **Follows the camera.** The trace is in tile space, so pan and zoom only change the camera uniform.
- **Level of detail.** Drawn at the zoom where controls are drawn. At the middle zoom a flat line at the frame's centre, brightness following the level. Not drawn at far zoom.
- **Redraw.** The scope draws a frame only when it is on screen, big enough to read, and its drawn trace moved by more than the player can see, at the tile's size and zoom. A silent or still signal draws none, and so does a steady smooth wave (Implemented, [#730](https://github.com/brylie/LoomDSP/issues/730)). A wave with corners sharper than a sample, such as a saw's jump, may still draw on some reads, because the corner falls at a new place between samples each time. When it is drawn at about a pixel per sample or more, that move is a real change on screen. A NaN alone is never a change (Implemented, [#637](https://github.com/brylie/LoomDSP/issues/637)). A steady signal with a NaN in it draws once, then stops. The gap it leaves wakes the scope again only when the joined line visibly cuts a corner, which needs a window with fewer samples than pixels. An idle patch or an off-screen scope does not redraw. *Why:* every frame costs battery on stage, and a change nobody can see is not worth one (NFR-PERF-04).
  - **The drawn size** (Implemented, #730). Code: `ScopeWant` and `App::publish_watch`. The UI tells the telemetry thread each scope's size as drawn, in physical pixels: the widget's bounds on its tile (`ModuleView::widget_bounds`), times the camera's zoom, times the window's scale factor. Each side is rounded to a whole pixel, and is at least 1. It travels in the scope's `ScopeWant`, so a zoom step changes the watch list in place and restarts no thread. Rounding keeps a small zoom step from rewriting the list when no side changed by a pixel. The app works it out again after any change that moves the scope's bounds: a camera change (`CanvasEvent::Camera`, whose arm then calls `publish_watch`), a change of the window's scale, a tile resize (`CanvasEvent::Resize` calls `App::refresh`, which rebuilds the scene and then reaches `publish_watch` through `refresh_values`, so a resize republishes the drawn size), and a scene change that moves the widget, such as a control shown or hidden above it. The drawn size reads the window's scale from each update's context (`cx.window().scale`), and the app republishes the watch list when it differs from the scale last used. A scale change arrives as a message, so an update runs. The app keeps the scale it last used (`App::window_scale`). The zoom is the camera the canvas last reported for the level shown (`CanvasEvent::Camera`, which the canvas also sends when it frames a level). With no drawn size known yet, before that first camera, a trace is sent on any change.
  - **Two scopes on one probe** (Implemented, #730) share one `ScopeWant`, because the watch list keys scopes by probe. Their drawn size is the larger of the two, each side on its own, so neither lags by more than half a pixel. Only the panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)) can make this case.
  - **What counts as moved** (Implemented, #730). Code: `scope_moved` and `line_check` in `monitor.rs`, called from `Watcher::take`. With full traces, the thread places both traces, the one the UI has and the new one, in pixels at that size: x from 0 to 1 across the width, y from -1 to 1 up the height. The new trace is sent when one is empty and the other is not, or when a point of either trace with x from 0 to 1 lies more than half a pixel from the other trace's whole line, points outside the frame included. The check runs both ways. Points outside the frame are not tested themselves, because the frame clips them, but they still shape the line a point inside is measured against. Where a trace's line crosses the frame's left or right edge (x = 0 or x = 1), the point on that edge, found by linear interpolation along the segment that spans it, is tested too, like a point inside. *Why:* a segment from a point inside to a point outside is drawn up to the edge, so when only the outside point moves, the end of the line at the edge moves while no point inside does. At the middle zoom a trace is only its level, already in 256 steps, and any change of step counts.
  - **The level in the UI snapshot** (Implemented, #730). While the full trace is drawn, only the points are compared, so a level change alone sends nothing. The snapshot's `TraceShown.level` then keeps the level last sent. It is exact again at the middle zoom, where the level is all that is drawn.
  - **Level history and spectrum** ([#810](https://github.com/brylie/LoomDSP/issues/810)). The thread compares the heights at their fixed columns, in physical pixels inside the frame. A line appearing or disappearing, a change in the columns present, or a column moving more than half a pixel sends the trace. The lighter peak or hold line counts while the zoom draws it.
  - **XY scope** ([#810](https://github.com/brylie/LoomDSP/issues/810)). On the square inside the frame, compare the points in both traces against the other trace's segments. A point more than half a physical pixel away sends the trace. At middle zoom the dot is a segment of zero length. The check uses the shape and ignores where in the window a sample fell, including differences in the age shading along a still line. A search that reaches its work limit sends the trace.
  - These three displays use the same scale and zoom as the Scope. Until the size is known, changes to visible points count. When widgets share a probe their comparison uses the larger drawn size on each axis.
  - See [Why a triggered scope holds still](../learn/triggered-scope.md) for the crossing, sample offset, and the points just outside the frame.
  - *Why the distance to the line, and not point by point:* the shift that keeps a wave still (Steady wave) samples it at a new phase on each read, so its points slide along the same curve. In a model of `subtractive.ron`'s scope (a smooth 261.63 Hz wave in a 5 ms window at 48 kHz, drawn at 216 × 120 pixels, the scope's 9 × 5 at zoom 1 on a 2× screen), points compared one by one moved by up to 1.7 pixels between reads, while the line moved by less than 0.01 pixels. *Why not the height at fixed columns:* a steep edge that shifts by a fraction of a pixel then reads as a jump of many pixels. The distance to the line is small for both. *The limit:* the line between two samples is straight, so a corner sharper than a sample moves by up to a sample as the phase changes. Drawn at about a pixel per sample or more, that is a real change on screen, and the scope can still wake on some reads. In the same model with a saw that jumps in one sample, the line moved by up to 0.86 pixels.
  - *A NaN* (Implemented, #730, #637) leaves a gap that the line joins (Values that are not finite). That wakes the UI only if the joined line passes more than half a pixel from the left-out sample, which needs a window with fewer samples than pixels. A NaN is never a change merely because it never equals itself.
  - **Bounded cost** (Implemented, #730). Both traces run left to right in x. For each point, the check keeps a window on the other line: the segments whose x lies within half a pixel of the point's, which moves forward as the points do. Inside the window it tries the segment under the point first, then its neighbours outward, since on a line that held still the nearest segment is usually there. The whole check is close to linear in the number of points (at most 1024 per trace, and the two edge crossings, which add nothing to the worst case: a crossing needs a point past that edge, and such a point is not tested). The segments tried for one point are capped at 64, the window included, and a point that reaches the cap counts as moved. The check stops at the first point that moved. *Why a cap:* a trace bunched into a column of pixels (a tiny drawn size, a vertical edge) would otherwise try every segment for every point. The cap costs at worst one redraw that was not needed, never a stall of the telemetry thread, which also carries the meters and the clip news. The worst case, 1024 points at a drawn size of 1 × 1, is 1024 × 64 segment tests. *Why 64:* full traces start at about zoom 0.81 (`loom-canvas/src/input.rs`), where a scope 9 grid units wide is about 87 pixels on a 1× screen. A window longer than about 21 ms is reduced to about 1,022 points, so about 12 points a pixel, and the half-pixel window holds 12 to 14 segments. A cap of 8 would be reached at every point and bring the redraws of [#730](https://github.com/brylie/LoomDSP/issues/730) back for long windows. 64 leaves room above that.
  - *Tests:* in `monitor/tests.rs`, a steady sine with a period of 183.47 samples, drawn at 432 × 240 (zoom 2 on a 2× screen), sends its trace once, then nothing over 200 reads; it fails with `window + 1` samples. A wave that moves by more than half a pixel sends, and one that moves by less does not. A steep edge that holds still does not send. When only a point past the frame's edge moves, the end of the line at the edge moving by about a pixel sends, at either edge and both ways, and by a quarter pixel does not (`where_the_line_crosses_an_edge_is_tested`). A reduced trace that gains or loses one point with no visible change does not send. A level step sends at the middle zoom. The same move sends at a larger drawn size where it did not at the smaller. A steady signal with one NaN sends once, then stops, and what it sent has only finite points. With no drawn size, any change sends. 1024 points at a drawn size of 1 × 1 finish within the cap. A triggered 0.5 s window of a sine whose period is not a whole number of samples, with several cycles in the window, drawn at 87 × 48 pixels (the smallest full-trace size on a 1× screen), sends once, then nothing over 200 reads, which also shows the cap is never reached by a still trace. (A 1 s window would run free: the history keeps exactly one second, and a trigger needs `window + 2` samples after it.) A zoom step, a change of the window's scale, and a tile resize each republish the watch list with the new size and keep the same monitor thread.
- **Size.** The widget takes the space the panel gives it, and grows with a resized tile ([resizable tiles](#resizable-tiles)).
- Acceptance: four live scopes on screen keep 60 fps on the reference laptop (NFR-PERF-04, ADR-0010 gate 2).

### Resizable tiles

([#61](https://github.com/brylie/LoomDSP/issues/61).)

**Status:** Implemented, with the far-edge rule for several trace displays, in both directions, and the rule that a meter moved by a display's edge does not also grow that way ([#637](https://github.com/brylie/LoomDSP/issues/637)). No module in the registry changed with these two rules, but `test.panel` did.

- **The handle.** A small grip at the bottom-right corner of a tile that has a **growable** widget (a scope, a meter, a level history, a spectrum, an XY scope, later a curve). Other tiles have no handle. Dragging it changes the tile's size and snaps to the grid. The cursor shows a resize cursor over it.
- **Minimum and maximum.** The minimum is the size of the panel as its parameter values show it (the tile cannot be smaller than its layout). The maximum is 64 by 64 grid units.
- **What grows.** Only growable widgets. They take all extra width and height. Other widgets keep their size and position, top left, except that a widget below a trace display (a scope, a level history, a spectrum, an XY scope) moves down with that display's bottom edge, and a widget to its right moves right with the display's right edge. *Why:* the display sits at the top of its panel, so the controls under it would otherwise be covered, and a tall display is the reason to resize one. The rule for several trace displays on one panel (Implemented, #637) is the same in both directions. Every shown trace display whose bottom edge is level with the lowest one takes the extra height, and every widget at or below that edge moves down. Every shown trace display whose right edge is level with the furthest right one takes the extra width, and every widget at or past that edge moves right. The other displays keep their declared size in that direction. So displays stacked in a column all grow wider together, displays side by side with level bottoms all grow taller together, and of two displays side by side the left one keeps its width. No display ever grows over another or over a widget. *Why one rule for both directions:* it is easy to predict, and it needs no tie-break, so the result never depends on the order widgets were declared. *Why not stop a display at its neighbour,* as a meter stops (the direction [#639](https://github.com/brylie/LoomDSP/issues/639) suggested): the left one of two displays side by side, or a display with a control beside it, would then never widen. The far-edge rule shifts whole halves of the panel, so nothing can overlap by construction. *The cost:* on a wide tile, a row of controls below a narrower display splits at that display's right edge: the controls past it move right and the others stay. The height rule already splits a column of controls the same way. A panel with one trace display, as every module in the registry has, lays out as it did before this rule. In `test.panel`, the far right edge is 27 (the level history and the spectrum) and the lowest edge 18 (the XY scope), so only the history and the spectrum widen, and only the XY scope grows taller. The scope at (0, 8) keeps its width, which is the fix #639 asked for: before this rule it grew over the select, the fader, and the spectrum. The XY scope at (18, 12) keeps its width too, so its square frame cannot grow. `test.panel` keeps its layout and has this look. The XY scope's frame stays square in whatever room it gets. A meter does not grow over a widget. A widget below it, in the meter's columns, stops it growing taller. A widget to its right, in the meter's rows, stops it growing wider. With nothing below it to stop it growing taller, a widget past its lower right corner also stops it growing wider, so it grows taller only. A hidden widget counts, so a meter does not jump when a setting shows or hides one. The exception is a hidden widget past the panel's closed-up width, which no longer takes any room. *Why:* a panel can place a meter above a control, as Dynamics does, and a covered control cannot be read or reached.
  - **How the rule is built** (Implemented, #637). Code: `ModuleView::widget_bounds` in `loom-canvas/src/scene.rs`. `display_edges` finds two far edges among the shown trace displays, in grid units as declared: the lowest bottom edge (`y` plus height) and the furthest right edge (`x` plus width). There is no single chosen display and no tie-break: every shown trace display whose bottom edge equals the lowest takes the extra height, and every one whose right edge equals the furthest right takes the extra width. `display_push` returns a shift in both directions: right by the extra width when the widget's left edge is at or past the far right edge, whatever its rows, and down by the extra height when its top edge is at or below the lowest bottom edge, whatever its columns. A hidden trace display does not count for either edge. *Why no overlap is possible:* each shift moves a whole half of the panel by the same amount, so the widgets in that half keep their places among themselves. A display on a far edge grows only into the band that shift just emptied, and a display that grows both ways grows into the corner that both shifts emptied. Nothing else grows into those bands except a meter, which the next rule keeps out.
  - **A meter that moves with a display does not also grow that way** (Implemented, #637). A meter moved right keeps its declared width, and a meter moved down keeps its declared height. *Why:* the room it moved into is the room it would have grown into. Growing as well would run it past the tile's edge by the extra size. The height half also fixes the rule before it, where a meter below a trace display, with nothing below it, moved down and grew taller too. No built-in panel has a meter below or to the right of a trace display, so none changed.
  - *Tests:* in `loom-canvas/src/scene/tests.rs`, beside `stacked_displays_never_cover_each_other`: two trace displays side by side on a wide tile do not overlap, the right one grows wider, and the left one keeps its width. Two stacked displays of equal width both grow wider, and only the lower grows taller. Two displays side by side with level bottoms both grow taller. A widget past the far right edge moves right, including one that shares only some of a display's rows. A row of controls below a narrower display splits at its right edge, with no overlap. A meter moved right or down keeps that size and stays inside the tile. On a panel with displays both stacked and side by side, no two widgets overlap after a resize, and none leaves the tile. The move-right tests use a panel built in the test, since no panel in the registry has a widget past a trace display's right edge. In `loom-app/src/scene/tests.rs`, which can see the registry: every module in the registry, widened, made taller, or both, lays out where the rule before this change put it (none has two trace displays, or a widget to the right of one), and `test.panel` widened has the level history and the spectrum wider, the scope and the XY scope at their declared width, only the XY scope taller, and no two widgets overlapping. In `loom-app/src/scene/app_tests.rs`, on a panel built in the test and widened, a control moved right has accessibility node bounds that match `ui.control_bounds`, and Tab visits the same controls in the same order as before the resize.
- **Ports.** Inputs stay on the left edge and outputs on the right, spread evenly over the new height, in port order. Cable ends, hit-testing, and hover follow the new size.
- **Saved.** The patch file gets an optional `size: (w, h)` next to the position, in grid units. A tile without one has its declared size. Files without the field load unchanged (ADR-0008).
- **Undo.** One resize drag is one undo step. The size is part of the clipboard: paste keeps it.
- **Cost.** A resize changes the scene once, when the drag ends, and shows a live outline while dragging. Nothing is rebuilt per frame (ADR-0010).
- **Keyboard.** With a growable tile selected, Alt-arrow keys resize by one grid unit. Shift-arrow still moves the tile by 10 ([Keyboard without a mouse](#keyboard-without-a-mouse)). *Why:* NFR-UX-01. A separate modifier means the two rules never both apply to one key.

### Acceptance criteria: M1 additions

- [x] Tab and Shift-Tab move between the fields of the patch form and of the patch browser, wrap at the ends, and stay inside the overlay (`tab_moves_between_the_fields_of_one_view`).
- [x] With a text box in focus, Space, A, F, Tab, Backspace, Enter, Left, and Cmd+V reach no canvas action, with the pointer over the box or over the canvas (`keys_stay_out_of_the_canvas_while_typing`). After Escape, Space toggles Run again (`escape_from_a_text_box_gives_keys_back_to_the_canvas`).
- [x] Choice names: the registry refuses a wrong count, an empty name, and names on a continuous parameter. The inspector shows "Toggle" and "Momentary" for Button's `mode` and a slider for a parameter without names. No module hard-codes names in its panel.
- [x] A one-sample trigger into a Clock Divide or Phase Trigger output light lights it, and it fades in 120 ms. No frames are drawn after the fade.
- [x] A transport-following input shows the clock mark with no cable, loses it when a cable is connected, and has it back when the cable is removed.
- [x] A 95% callback spike turns the load amber or red for its tick. After 10 seconds the max falls. Covered by callback and watcher tests.
- [x] By keys alone: Tab to a module, Enter into its panel, change a knob, Escape out twice. Space on a focused toggle acts on the toggle, not on Run. Arrow keys move the selected module by one grid unit.
- [x] An output with a cable shows *carrying* within 200 ms of a signal above −80 dB, and *silent* 400 ms after it stops. A one-sample trigger lights it. Cable geometry is the same with and without activity. Covered by activity and overlay tests; the state also appears in the UI snapshot.
- [x] The scope draws its trace in the tile, holds still for a periodic wave, and four scopes keep 60 fps. Checked on a development Mac (not the reference laptop): four live scopes draw at 71.8 fps with `--measure`, with one initial scene upload and five trace uploads during the five-second run. Recording CPU and GPU on the reference laptop is left for the benchmark doc.
- [x] A growable tile resizes to the grid within its limits. Ports and cable ends follow. The size is saved and loads. Old patches load. One drag is one undo step. Covered by canvas geometry, patch, and headless UI tests.
- [x] A NaN on a scope's input draws no point: the trace has only finite points, and an infinity is drawn at the frame's edge. A steady signal with one NaN in it draws once, then stops ([#637](https://github.com/brylie/LoomDSP/issues/637)). Covered by `a_nan_in_the_window_draws_no_point` and `an_infinity_is_drawn_at_the_edge` in `live/tests.rs`, `a_steady_signal_with_one_nan_sends_once` in `monitor/tests.rs`, and `a_scope_describes_only_finite_numbers` in `live/app_tests.rs`.
- [x] A steady smooth wave on a scope draws its trace once, then no more frames while it stays still. A wave that moves by more than can be seen draws ([#730](https://github.com/brylie/LoomDSP/issues/730)). Covered by `a_steady_sine_sends_its_trace_once`, `a_long_steady_window_sends_nothing_once_settled`, and `half_a_pixel_is_the_line` in `monitor/tests.rs`.
- [x] Trace displays side by side on a widened tile never overlap each other or another widget. Displays stacked in a column with level right edges all grow wider together. Every module in the registry lays out as before, and `test.panel`'s scope no longer grows over its neighbours ([#637](https://github.com/brylie/LoomDSP/issues/637)). Covered by `displays_side_by_side_share_a_wide_tile`, `stacked_displays_of_equal_width_both_widen`, and `mixed_displays_never_overlap_after_a_resize` in `loom-canvas/src/scene/tests.rs`, and `every_built_in_panel_lays_out_as_before_when_resized` and `the_test_panel_widened_grows_only_its_far_edge_displays` in `loom-app/src/scene/tests.rs`.
- [x] A meter moved right or down by a display's edge keeps its size in that direction and stays inside the tile ([#637](https://github.com/brylie/LoomDSP/issues/637)). Covered by `a_meter_moved_by_a_display_keeps_that_size`.
- [x] A light that stays on screen across a pan or zoom keeps its fade and does not flash again. A light that comes into view shows its value at once, and a trigger light that comes into view starts dark ([#619](https://github.com/brylie/LoomDSP/issues/619)). Covered by `a_light_that_stays_keeps_its_fade` and `a_trigger_light_new_to_the_list_starts_dark` in `lights/tests.rs`, and `a_light_brought_into_view_is_read_by_the_same_thread` in `watching/tests.rs`.
- [ ] A patch whose transport runs, with its Beat light on screen, draws a few frames each beat and nothing between them once its other displays are still. With the transport running, `--measure-idle` shows no frames of cause meters once its other displays are still; the Beat light's frames count as lights ([frame meter](frame-meter.md#causes)). This is a manual check on a real audio device, not yet done. With `--fake-audio --measure-idle --bench-seconds 6` on `subtractive.ron` it passes: `meters:5`, all in the first second, and lights about 9 a second ([M1 canvas benchmarks](../benchmarks/p1-canvas.md)).
- [x] A pan or zoom that changes which lights are on screen keeps the same light thread (the same handle key). Hiding every light stops the thread. A dropped handle ends the thread's wait within 100 ms, and so does a `set` made while the thread was reading, so a light brought into view is read by the running thread at once. Hiding every light and showing one starts a new thread that sends a reading, and the old thread has ended ([#619](https://github.com/brylie/LoomDSP/issues/619)).
- [x] A scope's drawn size follows its tile, the zoom, and the window's scale: a move of more than half a pixel at that size is sent, and a zoom step or scale change updates the watch list without restarting the telemetry thread. The check costs a bounded time for 1024 points at any drawn size, and every point the canvas gets is finite ([#730](https://github.com/brylie/LoomDSP/issues/730), [#637](https://github.com/brylie/LoomDSP/issues/637)). Covered by `a_scope_drawn_size_follows_the_zoom_the_scale_and_a_resize` in `watching/tests.rs`, `half_a_pixel_is_the_line` and `the_check_is_bounded_at_any_size` in `monitor/tests.rs`, and the finite-point tests in `live/tests.rs`, with the `debug_assert` in `line_check`.
- [x] `--dump-ui --frames` for longer than the telemetry queues hold, with a scope and an XY scope on screen, reports `telemetry_dropped: 0`, and a clip during the lead-in lights the meter's clip light ([#637](https://github.com/brylie/LoomDSP/issues/637)). Covered by `a_long_run_drops_no_telemetry` and `a_clip_in_the_lead_in_lights_the_meter` in `snapshot/tests.rs`.

## Default panels

**Status:** Draft, M2. Decision: [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md). Requirements: FR-PANEL-09, FR-SDK-01, NFR-UX-01.

A module that declares no panel gets one derived from its parameters, display values, and data slots. The author can change one parameter's widget with a hint, or write the whole panel. *Why:* most of a panel follows from the parameters. M1 panels are already "a knob for every parameter", with a few kept in the inspector. Deriving them means every module, built-in or third-party, gets a consistent, keyboard-reachable panel with no layout work.

### Which widget

Each declared item gets one widget, by the first rule that matches:

| Declared item | Widget |
|---|---|
| A parameter with a hint | The widget the hint names, or none for "inspector only" |
| A whole-number parameter from 0 to 1, with no choice names | Toggle |
| A whole-number parameter with choice names, 2 to 4 | Segmented control |
| A whole-number parameter with choice names, 5 | Radio buttons |
| A whole-number parameter with choice names, more than 5 | Select list |
| Any other parameter | Knob |
| A display value | Light |
| A data slot | Its kind's editor: curve, step grid, or waveform |

- The choice between radio buttons and a select list follows [Radio buttons or a select list?](panel-widgets.md#radio-buttons-or-a-select-list). Choice names come from the parameter ([#160](https://github.com/brylie/LoomDSP/issues/160)).
- A widget that binds more than one parameter (button, XY pad) is never derived. *Why:* the rules cannot know which two parameters belong together. A module that wants one writes its panel by hand, as the Button module does today.
- A widget bound to an output (meter, scope, XY scope, spectrum) is never derived either. *Why:* most outputs carry signals that nobody needs to watch on the tile, so a rule would add displays almost everywhere. A display module writes its panel by hand, as the Level Meter does today.
- A whole-number parameter without names gets a knob, as in M1. Leaning: the number widget, once it exists ([panel-widgets.md, Number](panel-widgets.md#number)).

### Hints

A hint is an option on the parameter, in `ParamOptions`. It names one widget from the parameter's group in [the baseline set](panel-widgets.md#the-baseline-set), such as a fader instead of a knob, or "inspector only".

- The registry refuses a hint outside the parameter's group, such as a toggle on a continuous parameter. *Why:* a hint picks a way to show the same kind of value. It cannot change what the value is.
- "Inspector only" is for parameters that are set once and rarely played: a noise seed, a MIDI channel, a bend range.
- A hint is not a position or a size. *Why:* layout belongs to the panel. A hint lives on the parameter, so it also reaches a sub-patch surface that exposes it (see below).

### Layout

**Status:** The flow and wrap rules (the second and third bullets) are Agreed (2026-11-07) and Implemented for the derived sub-patch surface. The order of data editors and choice widgets, and everything else in Default panels, is still Draft.

- Data editors come first, each on its own row, at the full panel width. Then choice widgets (segmented controls, select lists), then the other parameter widgets in the order the parameters are declared. Then the display values, in their declared order ([Panel layout](#panel-layout)).
- Widgets flow left to right on the panel grid and wrap at 12 grid units, four knobs wide. Each row is as tall as its tallest widget.
- The panel's size is the space its widgets fill. A module with nothing to show gets an empty panel, as today.
- Focus order is the order above, so Tab follows what the user reads.
- *Why declaration order:* the author already puts parameters in a meaningful order for the inspector. Deriving from it keeps the tile and the inspector alike.

### Writing a panel by hand

- A module can still declare its panel in full. Then nothing is derived for it, and the registry checks it as today ([Panel declaration](#panel-declaration-67)).
- **A module's hand-written panel shows every parameter that is not "inspector only".** The registry refuses a module panel that leaves one out, and names it. *Why:* the hint is then the one place that says a parameter is not on the tile. A forgotten knob is caught at registration, and a reader of the descriptor sees the choice. This rule is for module panels only. A sub-patch panel and a front panel show what their user chose, because a performance panel with every parameter on it is not minimal. The inspector shows every parameter either way.
- *Why keep "inspector only" at all:* some parameters are set once and never played, such as a seed or a MIDI channel. On a tile they take space from the controls that are played, and every derived panel would show them.
- The derived panel is a pure function of the descriptor, computed once when the module type is registered. *Why:* the same module must always look the same, and nothing about it may be rebuilt per frame.

### Acceptance criteria: default panels

- [ ] Each built-in module except Button and Level Meter gets its panel derived, with "inspector only" hints where M1 keeps a parameter in the inspector. Its widgets, bindings, and focus order equal today's hand-written panel (test). Positions may change.
- [ ] Deriving the panel twice from one descriptor gives equal panels (test).
- [ ] The registry refuses a hint outside its parameter's group, with a message that names the parameter (test).
- [ ] The registry refuses a hand-written module panel that leaves out a parameter without an "inspector only" hint, with a message that names the parameter (test).
- [ ] A test module with a 2-choice named parameter gets radio buttons, a 0 to 1 parameter without names a toggle, and a 12-choice named parameter a select list (test, once the select list exists).

## Exposed controls in a sub-patch

**Status:** Agreed, M2 (2026-11-01), with [sub-patches](sub-patches.md). Implemented for parameters (slice 3 of [#51](https://github.com/brylie/LoomDSP/issues/51), 2026-11-07): display values and data slots wait for the widgets that would show them. The derived panel for a sub-patch with no designed panel places the exposed items by the Agreed flow and wrap rules of the [Default panels layout](#layout-1), in the order they were exposed. Decision: [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md). Requirements: FR-ENCAP-03, FR-PANEL-03, FR-PANEL-09.

A sub-patch's surface is made of the same widgets as its inside. *Why:* a user who enters a sub-patch, or leaves it, must not have to learn a new set of widgets. The knob outside is the knob inside, and so is the light, the scope, and the curve.

**Any widget can be exposed:** a control, a light, a readout, a meter, a scope, a curve, a step grid, or a waveform. The surface widget binds to the same inner item: a parameter, a display value, an output's probe, or a data slot. Parameters follow the rules below. A display shows the same live value inside and out. An editor on the surface edits the inner module's data, and each edit is one undo step, whichever side it is made on.

- **Exposing copies the parameter's spec.** When an inner parameter is exposed (FR-ENCAP-03), the sub-patch's new parameter takes its label, description, unit, mapping, choice names, and hint. The user can give it a new name and a narrower range. The mapping and the kind of value stay. *Why:* a logarithmic cutoff must still turn like a cutoff, and a named choice must still show its names.
- **The same widget on both sides.** The surface shows the widget the inner tile shows for that parameter: a fader inside is a fader outside. In the panel editor, the user can switch it for another widget in the same group, as a hint does.
- **One parameter, two controls.** Both controls are bound to the same parameter, so they move together, show the same modulation, and an edit on either is one undo step (FR-PANEL-03).
- **A sub-patch with no designed panel** gets a panel derived from what it exposes. Each exposed item keeps the widget it has inside, and the widgets are placed by the [Default panels layout](#layout-1) rules, in the order the items were exposed. When the user opens the panel editor, it starts from that panel.
- **Seeing the link.** Inside the sub-patch, a widget that is exposed has a small mark, a shape and not only a colour (NFR-UX-03), and its hover text adds "Exposed as" and the surface label. On the surface, the hover text adds which inner module and parameter it controls. *Why:* entering a sub-patch, the user should find the knob they just turned outside without searching.

### Acceptance criteria: exposed controls

- [x] Exposing a parameter shown as a fader puts a fader on the sub-patch surface (`Ui` harness: `crates/loom-app/src/levels/tests.rs`, `exposing_a_fader_puts_a_fader_on_the_surface`).
- [x] Turning the surface control moves the inner control, and the reverse. One undo step reverts either (`Ui` harness: `crates/loom-app/src/levels/tests.rs`, `surface_and_inner_controls_move_together_and_undo_as_one`).
- [x] A sub-patch with three exposed parameters and no designed panel shows three widgets in the order exposed (test: `crates/loom-app/src/surface/tests.rs`, `three_exposed_parameters_make_three_widgets_in_exposure_order`).
- [x] An exposed control inside the sub-patch has its mark and its "Exposed as" hover text (UI snapshot and scene: `crates/loom-app/src/surface/tests.rs`, `an_exposed_control_inside_has_its_mark_and_hover_text`; `crates/loom-app/src/levels/tests.rs`, `exposing_a_fader_puts_a_fader_on_the_surface`).
- [ ] A light and a curve exposed from inside a sub-patch show on its surface. The light follows the inner display value, and a point dragged on the surface curve moves on the inner curve. One undo step reverts it (`Ui` harness). Waits for display values and data slots to be exposable; slice 3 exposes parameters only.

## Voice-count choice filtering

A choice parameter may list choices shown only while another integer parameter
is one, or above one. Choice IDs remain stable. The panel is rebuilt when that
integer changes. The choice's effective value may apply the source's documented
mode fallback without rewriting its stored value (polyphony.md, Note input).
Note input and Voice assigner use this for `mode`. A control hidden by
`shown_when` on such a choice follows the effective choice, the one the control
shows, not the stored value. So Voice assigner's `unison`, shown only in Oldest
and Quietest, shows at 8 voices with a stored Legato, because the module plays
Oldest there. *Why:* a control must not hide while it still changes the sound.
The inspector's "Not used in" names the effective choice too.

## Open points

- How third-party modules add display kinds through the SDK.
- Derived panels: check the 12-unit wrap width against real modules and the estimated label widths ([#120](https://github.com/brylie/LoomDSP/issues/120)).
- A derived display value gets a light. Once display values carry a unit and range ([#118](https://github.com/brylie/LoomDSP/issues/118)), should those with a unit get a readout or a meter instead?
