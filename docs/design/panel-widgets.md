# Panel Widgets Specification

The baseline set of panel widgets that LoomDSP modules, sub-patches, and front panels share, so each kind of interaction has one common way to work.

- **Kind:** living specification.
- **Status:** Draft, except the XY scope section, which is Agreed. Nothing else here is ready to build. The widgets that exist today (knob, toggle, radio buttons, button, light, meter) are specified and Implemented in [panels.md](panels.md#module-panels-on-tiles-p1); this document only plans what comes next and the shared rules that tie them together.
- **Decision behind it:** [ADR-0024](../adr/0024-widgets-follow-the-kind-of-value.md) (widgets follow the kind of value, and structured data has shared kinds with one editor each). Builds on [ADR-0013](../adr/0013-modules-carry-their-own-panel.md) (modules carry their own panel).
- **Use case and requirements:** UC2, UC3. FR-PANEL-02, FR-PANEL-03, FR-PANEL-04, FR-LIB-02, FR-LIB-12, FR-LIB-16, FR-LIB-17, FR-LIB-18, FR-LIB-19, FR-LIB-29, FR-MOD-04, FR-VIS-02, FR-VIS-03, NFR-UX-01, NFR-UX-02, NFR-UX-03.
- **Builds on:** [panels.md](panels.md) (widgets, layout, interaction, level of detail, redraw), [modulation.md](modulation.md) (range arcs, dots, live value), [telemetry.md](telemetry.md) (probes), [port and parameter text](port-and-parameter-text.md) (labels and descriptions).
- **Prior art:** hardware synthesizers and other patching tools have knobs, faders, rotary switches, scopes, and drawn envelopes. They are named here only to explain a convention, such as "an XY scope shows one signal against another, like an oscilloscope in XY mode". The widget set, names, and gestures below come from our own catalogue and requirements (ADR-0018).
- **Issues:** [#278](https://github.com/brylie/LoomDSP/issues/278) (agree this plan), widgets [#279](https://github.com/brylie/LoomDSP/issues/279) (select list), [#280](https://github.com/brylie/LoomDSP/issues/280) (XY scope), [#281](https://github.com/brylie/LoomDSP/issues/281) (curve editor), [#282](https://github.com/brylie/LoomDSP/issues/282) (shape from parameters), [#283](https://github.com/brylie/LoomDSP/issues/283) (fader), [#284](https://github.com/brylie/LoomDSP/issues/284) (number), [#285](https://github.com/brylie/LoomDSP/issues/285) (readout), [#286](https://github.com/brylie/LoomDSP/issues/286) (XY pad), [#287](https://github.com/brylie/LoomDSP/issues/287) (step grid), [#288](https://github.com/brylie/LoomDSP/issues/288) (waveform), [#289](https://github.com/brylie/LoomDSP/issues/289) (keyboard inside composite widgets), [#313](https://github.com/brylie/LoomDSP/issues/313) (range), [#314](https://github.com/brylie/LoomDSP/issues/314) (step grid ring and keys layouts), [#315](https://github.com/brylie/LoomDSP/issues/315) (waveform frames). Related: [#7](https://github.com/brylie/LoomDSP/issues/7) (umbrella, closed), [#50](https://github.com/brylie/LoomDSP/issues/50) (module data), [#60](https://github.com/brylie/LoomDSP/issues/60) and [#62](https://github.com/brylie/LoomDSP/issues/62) (scope), [#61](https://github.com/brylie/LoomDSP/issues/61) (resizable tiles), [#76](https://github.com/brylie/LoomDSP/issues/76) (panel editor), [#99](https://github.com/brylie/LoomDSP/issues/99) (stepped and decibel mappings), [#101](https://github.com/brylie/LoomDSP/issues/101) (keyboard without a mouse), [#109](https://github.com/brylie/LoomDSP/issues/109) (spectrum), [#114](https://github.com/brylie/LoomDSP/issues/114) (lights and triggers), [#118](https://github.com/brylie/LoomDSP/issues/118) (display value labels), [#120](https://github.com/brylie/LoomDSP/issues/120) (estimated label widths), [#160](https://github.com/brylie/LoomDSP/issues/160) (choice names), [#199](https://github.com/brylie/LoomDSP/issues/199) and [#208](https://github.com/brylie/LoomDSP/issues/208) (test modules).

## Goal

A user who has learned to turn the cutoff knob, pick a filter slope, and shape an envelope on one module can do the same on every other module, on a sub-patch tile, and on a front panel, without learning anything new. When they meet the Segment Envelope, the Curve Reader, and the Shaper's drawn curve, all three edit a curve in exactly the same way. When they meet "Unison voices" and "Filter slope", both show a word or a number they can click to see the other choices.

The panels spec already lists a widget set ([panels.md, Widgets](panels.md#widgets)). This document checks that list against the [module catalogue](../product/module-catalogue.md), fills the gaps, and says for each widget:

- what it binds to;
- how it shows modulation;
- how it works by pointer and by keyboard, and what it tells a screen reader;
- which catalogue entries use it.

## The baseline set

Widgets are grouped by the **kind of value** they show or edit, not by the module that first needs them (ADR-0024). Within a group, every widget shares its gestures, its keyboard model, and its way of showing modulation.

| Group | Widget | Binds to | Exists today | Proposed phase |
|---|---|---|---|---|
| One number | Knob | A parameter | Yes | P1 |
| | Fader | A parameter | No (planned as "slider") | P2 |
| | Number | A parameter | No | P2 |
| One choice | Toggle | A whole-number parameter, 0 to 1 | Yes | P1 |
| | Radio buttons | A whole-number parameter | Yes | P1 |
| | Select list | A whole-number parameter | No (planned as "selector") | P2 |
| Action | Button | A whole-number parameter, 0 to 1, and a mode parameter | Yes | P1 |
| Two numbers | XY pad | Two parameters | No | P2 |
| | Range | Two parameters on one axis: a low and a high | No | P2 |
| Module state | Light | A display value | Yes | P1 |
| | Readout | A display value | No | P2 |
| | Meter | An output, or a display value | Output only | P1, display value in P2 |
| Signals | Scope | An output | No ([#62](https://github.com/brylie/LoomDSP/issues/62)) | P1 |
| | XY scope | Two outputs | No | P1 or P2 |
| | Spectrum | An output | No ([#109](https://github.com/brylie/LoomDSP/issues/109)) | P2 |
| Shapes and data | Curve | Curve data, or a shape the module computes from its parameters | No (planned as "graph") | P2 |
| | Step grid | Pattern data. Drawn as rows, a ring, or keys | No | P2 |
| | Waveform | Buffer data, with markers bound to parameters. Whole, or as the frames of a wavetable | No | P2 |
| Static | Label | Text written in the panel | No | P2 |

Names in [FR-PANEL-02](../product/requirements.md) map as follows: "slider" is the **fader**, "selector" is the **select list**, and "graph" is the **curve**. FR-PANEL-02 does not list the number, readout, XY scope, spectrum, waveform, or range. If this document is agreed, FR-PANEL-02 is updated in the same pull request.

*Why these and no more:* every widget above is used by at least three catalogue entries (see [Which modules use which widget](#which-modules-use-which-widget)), or by one entry that cannot work without it. A widget that only one module wants is a sign that the module needs a different control, not a new widget.

## Bindings

Today a widget binds to a parameter, a display value, or one output (panels.md, Bindings). The baseline adds four kinds of binding.

| Binding | Used by | New? | What it needs |
|---|---|---|---|
| One parameter | Knob, fader, number, toggle, radio, select list, button | No | Choice names for whole-number parameters ([#160](https://github.com/brylie/LoomDSP/issues/160), [#99](https://github.com/brylie/LoomDSP/issues/99)), so the select list and radio buttons can show words. |
| Two parameters | XY pad, range | Yes | Two names on one widget. The registry checks both. A range's two parameters must have the same range and mapping, because they share one track. |
| Parameter as a marker | Waveform (start, end, loop points). Later: handles on a curve. | Yes | A widget part bound to a parameter, drawn on a data view. |
| Display value | Light, readout, meter | No | A label, a unit, and a range for each display value ([#118](https://github.com/brylie/LoomDSP/issues/118)), so a readout can format it and a meter can scale it. |
| One output | Meter, scope, spectrum | No | Probes, as today. |
| Two outputs | XY scope | Yes | A **paired probe**: one telemetry stream that carries both outputs' samples from the same block. See [XY scope](#xy-scope). |
| Module data | Curve, step grid, waveform | Yes | Module data ([#50](https://github.com/brylie/LoomDSP/issues/50), OQ-17) in one of three shared **data kinds**: curve, pattern, and buffer. See [Data kinds](#data-kinds). |
| Shape from parameters | Curve (read-only) | Yes | A pure function in the descriptor that turns parameter values into points. See [Curve](#curve). |

### Data kinds

Structured data that a module keeps beyond its parameters comes in three shared kinds. Each kind has exactly one editor widget (ADR-0024).

| Data kind | What it holds | Editor | Used by, for example |
|---|---|---|---|
| Curve | Up to 64 points with a bend per segment, and optional named markers | Curve | Segment envelope, curve reader, drawn transfer curve, drawn LFO shape |
| Pattern | Up to 8 lanes of up to 64 steps. Each lane holds gates or values. | Step grid | Multi-lane sequencer, parameter sequencer, matrix, a set of pitch classes (a scale) |
| Buffer | Audio frames, mono or stereo, up to a maximum length the module declares. Up to 64 slice points. A module can set a frame length, so the buffer holds a table of frames. | Waveform | Buffer, buffer reader, slice map, grain engine, audio cycle |

- A module declares its data slots by name and kind, as it declares parameters. A widget binds to a slot by name. The registry refuses a curve widget on a pattern slot.
- The limits above are fixed capacities. *Why:* new data is built on the control thread and swapped into the engine whole (ADR-0001), so the audio thread never allocates. A fixed capacity also keeps every editor's layout predictable. The numbers are leanings, to settle with [#50](https://github.com/brylie/LoomDSP/issues/50).
- **A buffer's length is not one number for every module.** A one-shot sample and a 10-minute recording need very different sizes, so the module declares its maximum, such as a length in seconds. The control thread allocates the buffer at that size when the module is created or the maximum changes, never on the audio thread. A module that records into a buffer writes into that space and stops at its end. Slice points are part of the buffer data, so a slice map's slices travel with its audio when the buffer is copied.
- **A wavetable is a buffer in frames.** A module that reads wavetables sets a frame length, such as 2048 samples. The buffer then holds whole frames only, up to 256 of them. *Why not a fourth data kind:* a wavetable is audio. It loads, stores, and copies like any buffer. Only the way it is read differs, and that belongs to the module.
- *Why shared kinds:* the catalogue already says the segment envelope and the curve reader "use the shared curve data". If each module kept its own curve format, each would need its own editor, and users would learn a new gesture set per module. One kind also lets a user copy a curve from an envelope and paste it into a shaper.

## Shared rules

These rules hold for every widget. Each widget section below says only what differs.

1. **The same gesture means the same thing everywhere.** Each group has one set of gestures, and the groups follow the P1 widgets (panels.md, Interaction):

   | Gesture | One number (knob, fader, number), XY pad, range, and the parts of a data editor (see below) | One choice (toggle, radio buttons, select list) | Display, and empty space in a display |
   |---|---|---|---|
   | Drag | Changes the value | Moves the tile | Moves the tile |
   | Shift-drag | 10 times finer | | |
   | Click | Selects the module | Acts: flips, picks, or opens the list | Selects the module |
   | Double-click | Resets to the default | Resets to the default | Opens the larger view, if the module has one |
   | Mouse wheel | Zooms the canvas | Zooms the canvas, except over an open select list | Zooms the canvas |

   The button is its own case (panels.md, Interaction). Data editors (curve, step grid, waveform) follow the first column for drags, Shift, and double-click, but a click acts on the part under the pointer: it selects a curve point or segment, adds a point on empty curve space, or flips a gate cell. Each widget's section gives the details. *Why:* a part of a data editor is a small control of its own, and clicking it should do the obvious thing, as clicking a toggle does. *Why:* a gesture learned once must work on every widget, or users hesitate on stage. *Why a choice control does not change on a drag:* toggles and radio buttons already move the tile on a drag, and a value that changes during a drag meant to move the tile is heard at once.
2. **Drags are relative, never jumps.** Pressing a control does not move its value. Only movement does. There are two exceptions, both only while Alt is held, so Alt always means "the value follows the pointer": pressing an XY pad jumps the point to the pointer ([XY pad](#xy-pad)), and dragging across a value lane of a step grid draws the values ([Step grid](#step-grid)). *Why:* a value that jumps to the pointer is heard at once on stage. *Why one modifier for both:* a user who learns "Alt draws" on one widget knows it on the other.
3. **Modulation shows the same way for the same kind of value.**

   | Value | Range | Live value | When all assignments are off |
   |---|---|---|---|
   | One number (knob, fader, number) | Arc or bar beside the control | Dot on the arc or bar | Hollow ring, no range |
   | One choice (toggle, radio, select list, button) | None. A filled dot beside the control. | None | Hollow ring |
   | Two numbers (XY pad) | A bar along each axis | A hollow point in the pad | Hollow ring, no bars |
   | Two numbers on one axis (range) | A bar beside the track for each handle | A thin line at each handle's live value | Hollow ring, no bars |
   | Marker on a data view | A band along the marker's axis | A thin line | Hollow ring on the marker |
   | Data (curve points, steps, buffer) | Not modulated. Modulate the module's parameters instead. | | |

   *Why:* the rules for knobs, toggles, and radio buttons are Agreed in [modulation.md](modulation.md#seeing-assignments). Every new control reuses the rule of the closest existing one. *Why data is not modulated:* a curve point or a step is not a parameter, so it has no cell for an assignment. A module that reads data has parameters, such as a curve reader's offset and scale, and those can be modulated.
4. **Composite widgets have an inside.** A widget with more than one editable part (XY pad, range, curve, step grid, waveform) takes focus as one control. Enter goes inside. Inside, the arrow keys move between parts, Space or Enter grabs a part, the arrows then change it, and Enter or Escape lets it go. Escape again leaves the widget. *Why:* without an inside, Tab would have to visit every step of a 64-step pattern before reaching the next control. A grab step means the arrows never both move focus and change a value, so a keyboard user always knows which one they do.
5. **Text comes from the descriptor.** Captions are labels. Hover text is the label and description. Choice names come from the parameter, display units from the display value, data slot labels from the slot. A screen reader gets the widget's label, its kind, and its value as text (NFR-UX-02). *Why:* the same rule as ADR-0016. Text written in the descriptor reaches the tile, the inspector, hover text, and screen readers alike.
6. **Displays never change the sound.** Reading display values, probes, or data never changes output (AGENTS.md rule 3). Editors change the sound only through parameters and module data, and every edit is one undo step per gesture.
7. **Draw only what changes, only when seen.** Live widgets (lights, readouts, meters, scopes, playheads) get data only while on screen at a zoom where they are drawn, and wake the UI only when what they draw changes. Data views (curve, step grid, waveform) build their GPU data when the data changes, never per frame (ADR-0010, panels.md, Redraw).
8. **Colour is never the only signal.** States that colour shows (on and off, modulated, playing step, clipping) also differ by shape, fill, or position (NFR-UX-03). *Why:* colour vision deficiency is common, and stage light changes colours.
9. **Every widget has a middle-zoom shape.** At middle zoom, text becomes bars and each widget keeps a simple shape that still shows its state. Far out, widgets are not drawn (panels.md, Level of detail).

## One number

### Knob

Implemented ([panels.md](panels.md#module-panels-on-tiles-p1)). Listed here as the reference for the other number controls.

### Fader

A straight track with a handle, vertical or horizontal. Same binding, mapping, and gestures as a knob.

- **Binds to:** any parameter. The author or panel editor picks the direction.
- **Drag:** relative, like a knob (rule 2). Movement along the track changes the value; the full range is the track length on screen, and at least 200 screen pixels, like a knob. *Why the minimum:* a short fader at middle zoom would otherwise be too coarse.
- **Modulation:** a bar beside the track in the modulation colour, from the lowest to the highest reachable value. The live value is a dot on that bar. Dragging the bar changes the depth of one assignment, as dragging a knob's arc does ([modulation.md, Arc drag](modulation.md#arc-drag)).
- **Keyboard:** as a knob.
- **Middle zoom:** track and handle, no text.
- **Size (leaning):** 2 × 6 grid units vertical, 6 × 2 horizontal.
- *Why a fader as well as a knob:* a row of faders shows several levels at a glance, as on a mixing desk, and a long throw gives finer control on a front panel.

### Number

A value shown as text, such as "120.0 BPM" or "7". For parameters where the exact number matters more than a position.

- **Binds to:** any parameter. Most useful for whole numbers with many values, such as a division, a step count, or a seed, and for tempo.
- **Drag:** vertical, relative, through the parameter's mapping, like a knob. Whole-number parameters step by one.
- **Modulation:** a dot, as on a toggle. The text shows the base value. *Why not the live value:* text that changes every frame cannot be read. The inspector and a readout can show it.
- **Keyboard:** as a knob. Typing a value is done in the inspector for now (see Open points).
- **Middle zoom:** a box with a bar for the text.
- **Size (leaning):** 3 × 2.

## One choice

Toggle and radio buttons are Implemented. The select list joins them. All three bind to a whole-number parameter, and the radio buttons and the select list are two ways to show the same thing.

### Choice names

A whole-number parameter can name its values, such as "6 dB", "12 dB", "18 dB", "24 dB" for a filter slope. Radio buttons and select lists show these names. A parameter without names shows its number and unit, such as "4 voices". Choice names live in the `ParamSpec`, so the inspector shows them too ([#160](https://github.com/brylie/LoomDSP/issues/160)). Today `WidgetKind::Radio` carries its own option labels. Those labels move to the parameter, and the radio widget reads them from there. *Why:* two places for the same names can disagree, and a radio button switched for a select list must show the same words. This is the "stepped" mapping of FR-PANEL-04 ([#99](https://github.com/brylie/LoomDSP/issues/99)).

### Select list

The current choice is shown as text. Clicking it shows the other choices in a list.

- **Binds to:** a whole-number parameter. Its choice names, or its numbers with the unit.
- **Closed:** a box with the current choice and a small down-pointing mark at the right end, which says "there is a list here" without colour.
- **Click:** opens the list. The list shows every choice, the current one marked by a tick and a filled row. It opens below the box, or above when there is no room below, and always inside the canvas. Clicking a choice picks it and closes the list: one undo step. Clicking outside the list, pressing Escape, panning, or zooming closes it without a change.
- **List size:** fixed screen size, 12 px text, like hover text, whatever the zoom. At most 12 rows are shown; a longer list scrolls, with the mouse wheel over the list. *Why fixed size:* the list is for reading and picking, so it must be readable at middle zoom. *Why the wheel scrolls here:* the open list covers the canvas, and scrolling it cannot change the sound.
- **Drag on the closed box:** moves the tile, as on toggles and radio buttons (rule 1). *Why not step through the choices:* a select list is a choice control, and choice controls do not change on a drag. A performer sweeps through choices with a MIDI controller or the arrow keys instead.
- **Double-click:** resets to the default.
- **Modulation:** a dot, as on radio buttons. The box shows the base choice.
- **Keyboard:** focused, Up and Down step the choice at once, as with every whole-number control. Space or Enter opens the list. This differs on purpose from toggles and radio buttons, where Space or Enter picks the next option. *Why:* a select list is used when there are many choices, so stepping one at a time with Space is slow, and Up and Down already step. In the list, Up and Down move the highlight, Enter picks, and Escape closes without a change.
- **MIDI learn:** a controller sweeps through the choices, as on radio buttons.
- **Middle zoom:** the box, a bar for the text, and the mark.
- **Size (leaning):** 6 × 2. The caption is above or below the box, as for a knob.

### Radio buttons or a select list?

They bind to the same parameter, so the author or the panel editor can switch one for the other. A [default panel](panels.md#default-panels) applies these rules when the module gives no hint.

- **Radio buttons** when there are 5 choices or fewer and the panel has room. *Why:* all choices are visible at once, and one click picks any of them. On stage, seeing the options matters.
- **Select list** when there are more choices, when the names are long, or when the panel is small. Unison voices from 1 to 16, a function choice, a scale, or a musical division such as 1/16 or 2 bars are select lists.
- **Toggle** for exactly two states that read as off and on. Two named states that are not off and on, such as "Linear" and "Equal power", use radio buttons.

## Action

### Button

Implemented ([panels.md](panels.md#interaction-68), [clock.md](clock.md#button-details)). No change.

## Two numbers

### XY pad

A square area with a point. The point's horizontal position is one parameter, its vertical position another.

- **Binds to:** two parameters, `x` and `y`. They may have different mappings. The registry refuses a pad whose two names are the same.
- **Drag:** moves the point by the pointer's movement, relative, like a knob in both directions at once. With Alt held, pressing jumps the point to the pointer. *Why relative by default:* rule 2. *Why a jump at all:* a pad played like a touch surface wants to jump; the modifier keeps it a choice, not an accident.
- **Shift-drag:** 10 times finer. **Double-click:** resets both parameters to their defaults.
- **Modulation:** a bar along the bottom edge for `x` and along the left edge for `y`, each like a fader's bar. The live value is a hollow point inside the pad.
- **Keyboard:** one point, so no inside (rule 4) is needed. Left and Right change `x`, Up and Down change `y`, in 1% steps, 0.1% with Shift.
- **MIDI learn and hover text:** per axis. Each edge of the pad has the hover text of its parameter. Learn for each axis is set in the inspector.
- **Middle zoom:** the square and the point.
- **Size (leaning):** 6 × 6, resizable once tiles are ([#61](https://github.com/brylie/LoomDSP/issues/61)).

### Range

A track with two handles, for a low and a high value on the same scale. The band between the handles is filled.

- **Binds to:** two parameters, `low` and `high`, with the same range and mapping. The registry refuses a range whose two parameters differ in either. *Why:* both handles sit on one track, so one position must mean one value for both.
- **Drag a handle:** moves it, relative, like a fader. A handle stops at the other handle and cannot pass it. *Why stop and not push:* pushing would change a value the user is not touching.
- **Drag the band:** moves both handles together and keeps the width. It stops when either handle reaches the end of the track.
- **Drag on the track outside the band:** moves the tile, as on empty space in a curve. *Why:* a press there must never jump a handle to the pointer (rule 2).
- **Shift-drag:** 10 times finer. **Double-click:** on a handle, resets that parameter. On the band, resets both.
- **Low above high:** modulation, a MIDI controller, or a typed value can put `low` above `high`. The widget draws each handle where its value is, and the module reads the lower value as the low end. *Why:* a module must never fail or go silent because of a modulation setting.
- **Modulation:** each handle has its own bar beside the track and a thin line at its live value (rule 3).
- **Keyboard (rule 4):** Enter goes inside. Left and Right (Up and Down on a vertical range) pick the low handle, the band, or the high handle. Space or Enter grabs. The arrows then move it in 1% steps, or 0.1% with Shift. Enter or Escape lets go.
- **Screen reader:** the label and both values in the parameter's units, such as "Range, 0.20 to 0.70".
- **MIDI learn and hover text:** per handle, as on the XY pad.
- **Middle zoom:** the track and the band.
- **Size (leaning):** 6 × 2 horizontal, or 2 × 6 vertical.
- *Why one widget and not two knobs:* the band shows the span at a glance, and a drag can never cross the two ends.

## Module state

### Light

Implemented. Lights for one-sample triggers are [#114](https://github.com/brylie/LoomDSP/issues/114).

### Readout

A display value shown as text: a count, a current step, a detected pitch, a position in bars and beats.

- **Binds to:** a display value. Its format comes from the display value's unit: a plain number with a set number of decimals, a note name with cents, or choice names (for example, which input a selector passes). Needs display values to carry text and a unit ([#118](https://github.com/brylie/LoomDSP/issues/118)).
- **Update rate:** at most 10 times a second, and only when the shown text changes. *Why:* text that changes faster cannot be read, and each change lays out text again.
- **Pointer:** a display. Pressing or dragging selects or moves the tile.
- **Screen reader:** reads the text, but does not announce each change. *Why:* a value read out 10 times a second would drown everything else.
- **Middle zoom:** a bar for the text.
- **Size (leaning):** 3 × 2, or 6 × 2 for longer text.

### Meter on a display value

The meter exists for outputs. The baseline also lets it bind to a display value, scaled by the display value's range. *Why:* gain reduction in a dynamics module, or an envelope follower's level, is module state, not an output level in decibels. One meter shape keeps "how much" looking the same everywhere.

A display value whose range runs below and above zero draws from zero, so a tuner's cents or a slope detector's direction read left or right of it. *Why:* the display value's range already says where zero is. The meter follows that, so no new widget is needed.

## Signals

### Scope

Specified in [panels.md](panels.md#scope-module), built in [#60](https://github.com/brylie/LoomDSP/issues/60) and [#62](https://github.com/brylie/LoomDSP/issues/62).

### XY scope

**Status:** Agreed for P1 ([#280](https://github.com/brylie/LoomDSP/issues/280)). This is the only widget in this document that is ready to build; the rest stays Draft.

One signal against another: the first moves the trace left and right, the second up and down. A circle means two sines a quarter cycle apart. A line means the two signals are the same. A stereo signal shows how wide it is.

- **Module: `loom.display.xy`**, category Display, the XY mode of the Probe (MT-VIEW-01). It is its own module because its widget, ports, and parameters differ from the scope's ([merge rule](../adr/0017-primitives-traits-and-abstractions.md)).

| | |
|---|---|
| Inputs | `x`, `y` (default 0) |
| Outputs | `x_out`, `y_out`, each passing its input through bit for bit |
| Parameters | `range`: a choice, 0 to 2 for ±0.1, ±1, ±10 (default 1). `time`: 0.001 to 0.1 s, logarithmic, default 0.02. `rotate`: toggle, default 0 |
| Panel | The XY scope widget, a knob for `time`, a radio for `range`, a toggle for `rotate` |

- The app probes the pair `(x_out, y_out)` with a [paired probe](telemetry.md#paired-probes). An unconnected input reads 0 and draws a dot or a line along an axis.
- **`time`** is how much history the trace shows. The trace draws at most 2048 points. When `time` holds more frames than that, the app keeps every Nth pair, with `N` the frame count divided by 2048, rounded up (at 48 kHz, 0.1 s is 4800 frames, so `N` is 3). *Why:* the knob then changes the length of the trace over its whole range, and the point count stays fixed. Older points fade, so the newest part is brightest.
- **`rotate`** (mid and side view) turns the trace by 45 degrees: with the new axes `(x + y) / √2` across and `(y − x) / √2` up. A mono signal (`x` equal to `y`) is then a vertical line. The audio is untouched.
- **Binds to:** two outputs of its module, `x_out` and `y_out`. The module is the Probe (MT-VIEW-01) in XY mode: two inputs, each passed through to an output, as the level meter does.
- **Data: a paired probe.** One telemetry probe carries both outputs' samples from the same block, interleaved, in one chunk. *Why not two scope probes:* each queue drops chunks on its own when full, so two probes can pair a sample of `x` with a sample of `y` from another block. The trace would then show shapes that are not in the signal.
- **Drawing:** a line through the last points, as many as fill the module's time window. Older points fade, so the newest part of the trace is brightest. A faint cross marks 0 on both axes.
- **Range:** from -1 to 1 on both axes by default. The module offers other ranges, as the [control ranges](control-ranges.md) do for parameters, for example -0.1 to 0.1 for small control signals and -10 to 10 for large ones. Points outside the range are clamped to the edge, never wrapped.
- **Mid and side view:** a module parameter turns the trace by 45 degrees, so a mono signal is a vertical line and a wide stereo signal spreads sideways. *Why a parameter and not a widget setting:* it is part of what the module shows, saved with the patch, and it can sit on a front panel.
- **Pointer:** a display. Pressing or dragging selects or moves the tile. Double-click opens the larger view when there is one (panels.md, Interaction on the canvas).
- **Keyboard:** not focusable. Its module's controls are.
- **Screen reader:** the label, and "XY scope of" the two input labels.
- **Redraw:** every frame while on screen and the signal moves. A still or silent signal draws no frames.
- **Middle zoom:** the frame and a single point at the latest position.
- **Size:** 6 × 6 grid units, resizable with the tile ([panels](panels.md#resizable-tiles)).
- **Explainer:** `docs/learn/reading-an-xy-scope.md` (phase, stereo width, and why a circle means a quarter cycle), written with the widget and linked from this section and the module's help.
- **Acceptance criteria**, following the checklist in [Acceptance criteria](#acceptance-criteria): the registry refuses an XY scope bound to anything but two outputs of its own module; a sine on `x` and a quarter-cycle-later sine on `y` draw a circle (geometry test on the point list); equal signals draw a diagonal and, with `rotate`, a vertical line; points outside `range` clamp to the edge and never wrap; two samples in a trace always come from the same block (paired-probe test); the widget is in `App::ui_snapshot`; middle zoom draws the frame and one point; a silent or still signal draws no frames; the widget is on a test module from [#208](https://github.com/brylie/LoomDSP/issues/208).

### Spectrum

Proposed in [#109](https://github.com/brylie/LoomDSP/issues/109). It belongs to this group and follows its rules.

## Shapes and data

### Curve

One widget draws and edits every curve: envelope shapes, transfer curves, drawn LFO shapes, and curves over bars.

**Binds to one of two sources:**

- **Curve data** (editable): a module data slot of the curve kind.
- **A shape from parameters** (read-only): the module's descriptor has a pure function that turns parameter values into curve points, such as an ADSR's shape from its four times and sustain level, or a shaper's built-in transfer curve. The UI calls it when a parameter changes. It never runs on the audio thread. *Why:* a module whose shape comes from knobs can still show that shape, the same way as a drawn curve, without its own drawing code.

**The curve data kind:**

- Points from left to right. Each has `x` from 0 to 1 and `y` from 0 to 1. The first point is at `x = 0` and the last at `x = 1`. Up to 64 points.
- Each segment between two points has a **bend** from -1 to 1. At 0 it is straight; positive bends toward its upper side, negative toward its lower side.
- The module maps the axes to what they mean. An envelope maps `x` to time through a length parameter and `y` to level. A shaper maps both axes to -1 to 1. *Why 0 to 1 and a separate length:* the shape and the time stay apart, so a length parameter can be modulated or synced to the transport while the shape stays put.
- **Options set by the module, not the user:**
  - **Wrap:** the curve repeats (an LFO shape). The last point's `y` is kept equal to the first's, and the editor draws a little of the next cycle at each end.
  - **Odd symmetry:** a transfer curve that is the same turned upside down around the centre. The user edits the right half; the left half follows.
  - **Markers:** named points the module reads, such as a hold point, or loop start and loop end. Each is attached to a point and drawn as a labelled flag.

**Pointer:**

| Gesture | On a point | On a segment | On empty space in the curve |
|---|---|---|---|
| Drag | Moves the point. It cannot pass its neighbours. End points move only up and down. | Up and down change the bend. | Moves the tile, so a drag on empty space never edits the curve by accident. |
| Shift-drag | 10 times finer | 10 times finer | |
| Click | Selects the point | Selects the segment | Adds a point on the curve, at the pointer's `x` |
| Double-click | Removes the point (not the end points) | Resets the bend to straight | Adds one point, as a click does |
| Drag a marker flag | Moves the marker to the nearest point | | |

*Why click adds and double-click removes:* double-click means "back to the default" everywhere (rule 1). A curve's default is to have no extra point there, and a segment's default bend is straight.

*Why double-click on empty space does not open the larger view, as on a display:* empty space in a curve is part of an editor, not a display. Its first click has already added a point. The curve's larger view opens from its corner mark (see Open points).

- **Live position:** a module that reads the curve publishes where it reads, as a display value from 0 to 1. The curve draws a thin vertical line there and a dot where it meets the curve. The line moves with rule 7.
- **Modulation:** not shown on the data (rule 3). For a shape from parameters, the shape follows the base values. See Open points for a live shape.
- **Keyboard (rule 4):** Enter goes inside and selects the first point. Left and Right move between points and segments. Space or Enter grabs; then arrows move a point, or Up and Down change a segment's bend; Enter or Escape lets go. The plus key adds a point in the middle of the selected segment. Delete removes the selected point.
- **Screen reader:** each point reads as "point 3 of 7, time 0.25, level 0.8" in the module's units.
- **Undo:** one step per gesture or key press.
- **Middle zoom:** the curve line only. No points or flags.
- **Size (leaning):** at least 9 × 6. Editing small curves on a tile is cramped, so the curve also opens in a larger view (see Open points).

### Step grid

Rows of steps. Each row (lane) holds gates or values. A column shows the playing step.

- **Binds to:** a module data slot of the pattern kind. Or, read-only, to a pattern the module computes from its parameters, such as a Euclidean rhythm from its hits, steps, and rotation.
- **Lanes:** a gate lane has cells that are on or off. A value lane has cells drawn as bars from 0 to 1. The module maps values to what they mean, such as pitch through the scale, or level.
- **Playing step:** from a display value. The column has an outline and a filled marker above it. *Why both:* colour alone is not enough (rule 8).
- **Pointer:**
  - Gate lane: click flips a cell. Dragging across cells sets each one to the state the first cell got. *Why:* painting a run of hits is one gesture.
  - Value lane: drag up and down in a cell to change its value, relative, as a fader. The drag stays on the cell where it started.
  - Value lane, with Alt held: drag across the lane to draw. Each cell the pointer passes takes the value of the pointer's height in the lane. This is one of the two exceptions to rule 2. *Why:* drawing a line or a ramp across 16 cells must be one gesture, and only an absolute value can follow the pointer's path. *Why only with Alt:* a plain press must never change a step that is playing.
  - Double-click a cell resets it to the lane's default.
- **Keyboard (rule 4):** Enter goes inside. Arrows move between cells. Space flips a gate cell. On a value cell, Space or Enter grabs it, Up and Down change it, and Enter or Escape lets go.
- **Modulation:** cells are data and are not modulated (rule 3).
- **Middle zoom:** cells as filled or empty shapes, the playing column outlined.
- **Size (leaning):** one grid unit per step, two per lane, plus a row for the step marker. Lanes longer than the tile scroll sideways, or show in the larger view.
- **Layouts.** The panel picks one. Gestures, keyboard, screen reader text, and data are the same in every layout. *Why:* ADR-0024 gives each data kind one editor. A layout changes where the cells are drawn, not how they work.
  - **Rows** (the default): as above.
  - **Ring:** the cells of a lane go around a circle, clockwise from the top. Up to 4 lanes draw as rings inside each other, the first lane outermost. Each ring marks its own playing step: the cell is outlined, with a filled dot on the ring's outer edge. *Why:* lanes of different lengths (polymetric patterns, P3) are on different steps at the same time. If the module also publishes a phase display value, a thin hand from the centre shows it, so a slow cycle visibly moves between steps. Left and Right move around a ring. Up and Down move between rings. *Why a ring:* a rhythm is a cycle. On a ring, the evenness of a Euclidean pattern and the turn of a rotation show at once.
  - **Keys:** for one gate lane that holds a set of pitch classes, one cell per pitch class. With 12 cells, the cells are drawn as one octave of a piano keyboard. With any other number of cells, they are drawn as a ring. The layout follows the number of cells, so a live change of tuning never leaves a broken drawing. Left and Right move in pitch order, black keys included. *Why a piano at all:* most players read a scale fastest on a keyboard. *Why not only a piano:* LoomDSP does not assume 12-tone equal temperament. A tuning with 19 or 31 notes per octave draws as a ring of 19 or 31 cells.
  - **Which cell is which key:** cell 0 is the tuning's reference pitch class (C in a 12-note tuning, pitch 0.0 in FR-SIG-05), not the scale's root. *Why:* if cells counted from the root, every major scale would draw as the white keys, and the keys would not match the notes. The module marks the root cell. How it is marked is an open point.
  - **Cell names:** in the keys layout, the module names each cell for hover text and the screen reader, such as "C sharp" in a 12-note tuning or "degree 5 of 19" in another. The names change when the tuning does. See Open points.
  - The ring and keys layouts draw gate lanes only. The registry refuses them on a data slot that can hold value lanes. *Why:* a value cell is set by height, and a ring has no single up and down.
- *Why one grid for gates and values:* the multi-lane sequencer mixes both in one pattern, and a matrix of levels (MT-ROUTE-04) is a grid of value cells. One widget keeps them alike.

### Waveform

A buffer drawn as its outline, with markers for where a module reads.

- **Binds to:** a module data slot of the buffer kind. Each marker binds to a parameter: start, end, loop start, loop end, a grain position. Slice points come from the module's data.
- **Drawing:** the outline (lowest and highest sample per column) is computed on a control thread when the buffer changes, not per frame. *Why:* a buffer can hold millions of samples.
- **Playheads:** from display values. A module that plays several reads at once (a grain engine) publishes the range they cover, drawn as a band.
- **Frames (wavetables):** when the buffer has a frame length (see [Data kinds](#data-kinds)), the waveform draws two things. Above, a large outline of the frame at the current position, mixed between its two neighbours the way the module reads it. Below, a strip with one small outline per frame. The position is a marker bound to a parameter, like the other markers, so its modulation band shows which frames modulation can reach (rule 3). The module's live read position comes from a display value and is drawn as a thin line on the strip. *Why not an XY scope:* an XY scope shows two live signals against each other. A wavetable is stored data with a position, which is what the waveform's markers are for.
- **Pointer:** dragging a marker sets its parameter, relative. Shift for fine. Double-click a marker resets its parameter. The outline itself is a display (rule 1): pressing it selects or moves the tile, and double-clicking it opens the larger view.
- **Modulation:** each marker shows its reachable range as a band along the time axis, and its live position as a thin line (rule 3).
- **Keyboard (rule 4):** markers are the parts. Arrows move between them; grab and move as on a curve.
- **Middle zoom:** the outline only.
- **Size (leaning):** at least 9 × 4, resizable.

## Static

### Label

Text placed in a panel by the user or the author, for section titles and notes on front panels. It has no binding and is not focusable. Its text is part of the panel data, saved with the patch, and translatable later (NFR-UX-04).

## Which modules use which widget

The catalogue entries that would use each new widget. The knob is used by nearly every module and is not listed.

| Widget | Catalogue entries |
|---|---|
| Fader | MT-MIX-01 levels, MT-MIX-02, MT-UI-02, MT-EQ-01 band gains, MT-ARR-06, MT-SRC-03 group levels |
| Number | MT-CLOCK-01 tempo, MT-CLOCK-03 division, MT-PHASE-02 ratio, MT-PHASE-06, MT-GATE-02 hits, steps, and rotation, MT-LOGIC-04 count, MT-PITCH-01 offsets, MT-IO-04 controller number, MT-ARR-01 and MT-ARR-02 length in bars, TR-07 seed |
| Radio buttons | MT-FILTER-04 slope, MT-MIX-02 law, MT-LEVEL-02 response, MT-PHASE-08 wrap or fold, MT-ROUTE-01 and MT-ROUTE-02 how to choose, MT-DELAY-04 time change, MT-LOGIC-01 mode, TR-02 timebase, TR-03 retrigger |
| Select list | MT-SRC-08 and MT-VOICE-04 unison voices, MT-SRC-02 colour, MT-LEVEL-01 unit, MT-MATH-02 function, MT-MATH-03 conversion, MT-LOGIC-02 operation, MT-VOICE-01 merge mode, MT-SHAPE-01 curve, MT-DYN-01 mode, MT-FX-01 space, MT-SEQ-04 algorithm, MT-PITCH-02 and MT-PITCH-03 scale, MT-ENV-01 mode, TR-01 basic shape, TR-02 musical division, TR-05 character, TR-06 quality |
| Toggle | MT-MIX-01 mute, MT-ENV-01 loop, MT-DELAY-04 ping-pong, MT-SRC-01 sync |
| XY pad | MT-UI-03, MT-STATE-02, MT-FX-10 position |
| Range | MT-RAND-01 range, MT-LEVEL-05 limits, MT-LEVEL-03 range map, MT-LOGIC-01 window, MT-NOTE-07 pitch and velocity ranges, MT-SAMPLE-05 zones, MT-SEQ-04 note range |
| Readout | MT-VIEW-01 number mode, MT-VIEW-04, MT-VIEW-05, MT-PITCH-04, MT-LOGIC-04 count, MT-CLOCK-01 position, MT-ROUTE-01 current input, MT-ARR-05 current part |
| Meter on a display value | MT-DYN-01 gain reduction, MT-ENV-04 level, MT-DYN-02, MT-VIEW-05 and MT-PITCH-04 (centre-zero), MT-ENV-05 |
| XY scope | MT-VIEW-01 XY mode, MT-MIX-03 stereo width, MT-SRC-04 attractors, MT-SRC-12, MT-FILTER-10 (the two outputs make a circle), MT-PHASE-05 (phase in against phase out shows the warp), MT-RAND-01 walk in two dimensions |
| Curve, data | MT-ENV-02, MT-MOD-02 (drawn LFOs, section curves for FR-LIB-16, velocity and keytracking curves), MT-SHAPE-01 drawn curve, TR-01 drawn shape (so MT-SRC-01 and MT-MOD-01) |
| Curve, shape from parameters | MT-ENV-01, MT-ENV-03, MT-PHASE-05, MT-SHAPE-01 built-in curves, MT-MIX-02 law, MT-LEVEL-03 range map, MT-DYN-01 transfer curve, MT-FILTER-01 to MT-FILTER-04 and MT-EQ-01 response |
| Step grid, data | MT-SEQ-01, MT-SEQ-02, MT-ROUTE-04, MT-SRC-03 partial levels |
| Step grid, from parameters or display | MT-GATE-02, MT-LOGIC-05, MT-LOGIC-06, MT-RAND-03 |
| Step grid, ring layout | MT-GATE-02, MT-LOGIC-04, MT-LOGIC-05, MT-LOGIC-06, MT-CLOCK-03, MT-PHASE-06, MT-ARR-03 |
| Step grid, keys layout | MT-PITCH-02, MT-PITCH-03, MT-NOTE-02, MT-MOD-03 (read-only: one gate per pitch class) |
| Waveform | MT-SAMPLE-01, MT-SAMPLE-02, MT-SAMPLE-03, MT-SAMPLE-04, MT-SAMPLE-06, MT-SAMPLE-08, MT-ARR-02, MT-FX-04 |
| Waveform, frames | TR-01 wavetable shapes (so MT-SRC-01 and MT-MOD-01), and the wavetable reader in FR-LIB-02 |

**Not in the baseline.** These catalogue entries need a view of their own. Each gets its own spec when its module is designed, and joins the set then if a second module needs it.

- A note editor with notes of any length and pitch: MT-SEQ-03 piano roll, MT-ARR-01 note cycle.
- A playable keyboard: MT-UI-04. Waits on how notes travel on cables (OQ-16). The keys layout of the step grid is not this. It edits a set of pitch classes and sends no notes.
- A note and controller monitor list: MT-VIEW-04 beyond its readouts.
- A diagram of states: MT-LOGIC-07.
- A list of sections: MT-ARR-07.
- A 2D field with several stored points: MT-STATE-02 beyond one XY pad point.

## Acceptance criteria

This is a plan. Each widget gets its own acceptance criteria when its section becomes Agreed, written as for the P1 widgets in [panels.md](panels.md#acceptance-criteria-module-panels). Every widget section, when Agreed, must cover:

- [ ] The registry refuses each kind of bad binding for the widget, with a message that names the widget.
- [ ] Pointer gestures follow rule 1 and rule 2, tested with the `Ui` harness by name, not by pixel.
- [ ] Keyboard use reaches every part, with a visible focus ring (rule 4, NFR-UX-01).
- [ ] Modulation shows as rule 3 says, tested on the geometry.
- [ ] The UI snapshot (`App::ui_snapshot`) reports the widget, its value as text, and its world place, so the control channel can find it.
- [ ] Middle and far zoom follow rule 9.
- [ ] Nothing redraws while nothing it shows changes (rule 7).
- [ ] The test modules from [#208](https://github.com/brylie/LoomDSP/issues/208) gain the widget.

## Out of scope

- Building any widget. This document only plans them.
- Themes, skins, and custom images (FR-PANEL-06, FR-PANEL-07).
- The panel editor ([#76](https://github.com/brylie/LoomDSP/issues/76)). It places these widgets; it does not change how they work.
- The format of module data in the patch file, and how it reaches the engine. That is [#50](https://github.com/brylie/LoomDSP/issues/50) (OQ-17). This document only names the three data kinds and their limits.
- Touch gestures.

## Open points

- **Larger view.** Curves, step grids, and waveforms are cramped on a tile. Leaning: a small corner mark on the widget opens a larger editor over the canvas, at fixed screen size, editing the same data. On a waveform, double-clicking the outline opens it too, because the outline is a display (rule 1). A curve and a step grid have no display area to double-click, since every press there edits, so they use the corner mark only. Decide with [#61](https://github.com/brylie/LoomDSP/issues/61) (resizable tiles).
- **Handles bound to parameters on a curve.** An ADSR shape could have handles that set attack, decay, sustain, and release directly. The waveform already binds markers to parameters, so this is the same idea. Leaning: yes, after the read-only shape view is built and tried.
- **Live shape under modulation.** A shape from parameters could also draw, faintly, the shape at the live modulated values. Leaning: yes, if it needs no new engine data. The live value table ([modulation.md, Live value](modulation.md#live-value)) already has each modulated parameter.
- **Typing a value on the tile.** Number widgets and knobs could take typed values. Leaning: not on the tile in P2; the inspector has a text field. Revisit with the panel editor.
- **Select list type-ahead.** Typing a letter in an open list could jump to the first choice that starts with it. Leaning: yes for lists longer than 12, which scroll.
- **Snap to grid on a curve.** A modifier key, or a module option, that snaps points to a grid of 8 or 16. Which key, and whether the grid follows the transport for curves over bars.
- **Matrix as data or as parameters.** A matrix (MT-ROUTE-04) with a level at each crossing could store levels as pattern data (no modulation) or as parameters (modulatable, but N × M of them). Decide in its module spec.
- **Which size of the panel grid suits text widgets.** Sizes above are leanings, to check against the estimated label widths ([#120](https://github.com/brylie/LoomDSP/issues/120)).
- **Cell names in the keys layout.** Names come from the active tuning (MT-PITCH-03, #309), so they are module state, not descriptor text. Leaning: a list of display texts the module publishes, read like a readout's text ([#118](https://github.com/brylie/LoomDSP/issues/118)).
- **Marking the root in the keys layout.** Outline and filled dot already mean the playing step, so the root needs another mark, such as a small bar under the key.
- **Tunings with more than 64 notes per octave.** A pattern lane holds 64 cells, enough for 53 equal divisions of the octave. Larger tunings, such as 72 divisions, would need a longer lane. Leaning: wait until someone asks.
- **Polar views.** A polar view on the XY scope, where the angle shows the balance between the two sides and the distance shows level, could show a stereo field more plainly than the mid and side view. A polar XY pad (angle and distance) suits the spatialiser (MT-FX-10). Leaning: decide when the spatialiser is designed, since each view has only one clear user today.
- **Slow history.** A display value drawn over seconds or bars, for a random walk, a slew, or an envelope follower ([#97](https://github.com/brylie/LoomDSP/issues/97)). Leaning: a long time window on the scope, fed by display values instead of audio samples, so it costs little.
- **Wavetables and FR-LIB-02.** FR-LIB-02 lists a wavetable reader in P1, but buffer data is planned for P2 ([#288](https://github.com/brylie/LoomDSP/issues/288)). Either move the wavetable reader to P2 in FR-LIB-02, or bring buffer data forward. Decide in #278.
