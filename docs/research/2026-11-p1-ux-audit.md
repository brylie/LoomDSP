# P1 UX Audit: Panels, Matrix, Inspector, and Help

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at 4e86766 (after #447).
- **Used by:** issues #453 to #460 on the P1 milestone. Decisions go in specs or ADRs.

This audit looks at the P1 desktop app as a user meets it: module tiles, the inspector, the modulation matrix, help, the module browser, and the window chrome. It records what we saw, why it matters, and a direction for each finding. It does not decide anything. Do not update it later; write a new snapshot after the fixes.

## Method

- Rendered every built-in module (46 tiles plus Transport) in a grid patch with `--screenshot` at zoom 1 and scale 2.
- Rendered the window with `--screenshot-window` for `modulation.ron` and `subtractive.ron`, with a module selected and with help open.
- Drove the live app with `--control-script` and `--fake-audio` to open the Matrix tab, the matrix Add form, and the module browser, and saved window screenshots.
- Counted cut labels with `--dump-ui` (the `shown` field).
- Rendered the Envelope, Ladder, LFO, and SVF tiles at 1x, 2x, and 4x zoom to look for aliasing.
- Measured header colours with a short script: WCAG contrast against the header text, and CIE76 colour difference (ΔE) between every pair under normal vision and protanopia, deuteranopia, and tritanopia simulation (Machado, Oliveira, and Fernandes, IEEE TVCG 2009, severity 1). About ΔE 10 or more reads as clearly different.

**Caveat on headless window images.** `--screenshot-window` draws every Iced button as disabled ([#460](https://github.com/brylie/LoomDSP/issues/460)), so its toolbar and tabs look muted. The colour findings below come from live window screenshots taken through the control channel.
- Read the view code for the matrix (`crates/loom-app/src/matrix.rs`), the toolbar and right column (`main.rs`), and help (`help.rs`), and the agreed rules in `panels.md`, `panel-widgets.md`, `modulation.md`, `module-browser.md`, `help-pages.md`, and `port-and-parameter-text.md`.

Not covered: MIDI learn mode, the Preferences dialog, keyboard focus rings, the probe menu, screen readers, and a contrast check of every text and widget pair outside the module headers. These need their own pass.

Priority means: **High** blocks a task or misleads the user; **Medium** slows the user or looks broken; **Low** is polish.

## Summary

The canvas is in good shape. Tiles read clearly at 100%, the category colours on cables and ports work, and the shape previews (Envelope, Ladder, LFO) are a real strength. The weak parts are the side column and the chrome. The modulation matrix does not fit its panel. The inspector and help put developer details before user tasks. Every toolbar button has the same loud orange, so nothing stands out. On the tiles, the main problems are cut captions on common words and layouts that differ from module to module.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Matrix | The table is 600 px wide in a 280 px panel; Depth and Polarity are off screen | High |
| 2 | Canvas | Example patches open below the zoom where labels draw | High |
| 3 | Panels | Nine captions are cut, including Resonance on both filters | High |
| 4 | Chrome | Every toolbar button and tab is the same primary orange | Medium |
| 5 | Matrix | Every cell is a button; the selection marker is a cut orange square | Medium |
| 6 | Tabs | The active tab looks the same as the other tab | Medium |
| 7 | Inspector | The empty state is a 30-line wall of shortcuts | Medium |
| 8 | Help | Two scroll regions, cut text, and mixed type sizes | Medium |
| 9 | Browser | The highlighted row hides its category and ID | Medium |
| 10 | Panels | Layout grammar differs between tiles | Medium |
| 11 | Panels | A knob at its minimum looks disabled | Medium |
| 12 | Help text | Jargon, requirement IDs, and raw values in user text | Medium |
| 13 | Status bar | Developer toggles sit beside user status; long MIDI lists push items off | Medium |
| 14 | Inspector | Port text is an unstructured list; "Add modulation" does not look clickable | Low |
| 15 | Panels | Category colours: many share grey, four fail text contrast, some collapse for colour-blind users | Medium |
| 16 | Names | Module names mix Title Case and sentence case | Low |
| 17 | Type | Eight text sizes and no shared scale | Low |
| 18 | Panels | Small fixes on single tiles | Low |
| 19 | Canvas | Result curves are drawn as boxes, so they look beaded and stepped when zoomed | High |

## Issues

The findings are grouped into seven issues on the P1 milestone. A tooling bug found along the way is [#460](https://github.com/brylie/LoomDSP/issues/460).

| Issue | Findings |
|---|---|
| [#453](https://github.com/brylie/LoomDSP/issues/453) Panel layout principles, and apply them to every built-in panel | 10, 18 |
| [#454](https://github.com/brylie/LoomDSP/issues/454) Visual system: type scale, button roles, and a calmer chrome | 4, 11, 13, 17 |
| [#459](https://github.com/brylie/LoomDSP/issues/459) Module families and a shared colour palette | 15 |
| [#455](https://github.com/brylie/LoomDSP/issues/455) Canvas readability: smooth result curves, labels at open zoom, no cut captions | 2, 3, 19 |
| [#456](https://github.com/brylie/LoomDSP/issues/456) Modulation matrix: fit the side column, plain cells, and real tabs | 1, 5, 6 |
| [#457](https://github.com/brylie/LoomDSP/issues/457) Inspector, help overlay, and module browser: user details first | 7, 8, 9, 14 |
| [#458](https://github.com/brylie/LoomDSP/issues/458) Module names and summaries in plain words | 12, 16 |

## Findings

### 1. The matrix does not fit its panel (High)

`matrix::view` builds the table at a fixed `Length::Fixed(600.0)` (`matrix.rs:369`) and puts it in a container 280 px wide (`matrix.rs:539`, `main.rs:3448`). In the live app only On, Source, and part of Target show. Depth and Polarity, the two columns a user changes most, need a sideways scroll. Target names are cut ("SVF #4: Cut").

The spec allows the table to scroll sideways (modulation.md, slice 3). It does not say the right column stays 280 px when the matrix is open. So this needs a spec change, not only code.

Directions to weigh:

- Let the right column grow when the Matrix tab is open, or make the column resizable.
- Or use a two-line row in 280 px: line 1 is `Source → Target`, line 2 is the depth slider, depth text, polarity, on, and remove. This is how the inspector already shows an assignment, so the two views would match.
- Or open the matrix as a bottom panel under the canvas, where width is free.

### 2. Example patches open with labels hidden (High)

`modulation.ron` and `subtractive.ron` open zoomed to fit a 1440 by 900 window. At that zoom the canvas is in its middle level of detail, so every port label and knob caption is a grey bar. Only titles show. A new user's first view of an example has no words on the controls.

The cause is the base sizes: `LABEL_SIZE = 9` and `LABEL_MIN_PX = 7.5` (`tile_text.rs`, `widget.rs`) mean labels hide below about 83% zoom. Directions: have Frame (F) and patch open never go below the label zoom, and pan instead; or lower the label threshold; or raise the base label size (see 17).

### 3. Cut captions on common words (High)

`--dump-ui` over all modules finds nine cut labels:

| Module | Label | Shown |
|---|---|---|
| SVF | Resonance (knob), Bandpass, Highpass (ports) | Reson…, Bandp…, Highpa… |
| Ladder | Resonance | Reson… |
| One-pole | Highpass (port) | Highpa… |
| Compare | Hysteresis | Hyster… |
| MIDI Controller | Controller, Smoothing | Cut to 6 and 5 letters |
| Constant (pitch) | Semitones | Semit… |

Resonance is on the two most used filters. The hover text gives the full word, but a performer should not have to hover to read a knob. The rule "cut, never wrap" (panels.md) is right; the widths are what fail.

Directions: let a knob caption use the gap to its neighbour (most of these knobs have empty space beside them); widen the port label column from 4 to 5 units on tiles with long port names; or add short display labels ("Res", "HP", "BP") as a separate descriptor field, never a rename of the ID.

### 4. One loud orange for everything (Medium)

In the live app ten of the eleven toolbar buttons (all but MIDI learn), both tabs, the Help and Close buttons, Open example, Add, Create, Cancel, and every matrix column heading use `button::primary` on `#F5A623`. Orange is also the colour panels.md gives to editable values on the canvas. When everything is primary, nothing is: New and Panic have the same weight, and the orange chrome competes with the orange knob arcs.

Direction: keep orange for values and the one main action in a view. Give toolbar, tabs, and dialogs a quiet secondary style (tile grey with light text), and group the toolbar (file, edit, performance). Panic may deserve the danger colour. This is one place to change, the app theme in `main.rs:1066`, plus the button style calls.

### 5. The matrix is a grid of buttons (Medium)

Each Source, Target, Depth, and Polarity cell is a `button::secondary` (`matrix.rs:425-500`), so a row is four grey boxes plus a slider and a checkbox. The row marker is an orange square with "○", "●", or "▶" in it, cut at the left edge. Headings are orange buttons. There is also a status line, "Row #3: Depth cell (editing)", that reads as debug output.

Direction: draw cells as plain text with a hover and focus outline, keep the selected row's highlight, drop the marker column (the highlight already shows the row), use plain text headings with a sort arrow, and remove the status line or turn it into an accessible label.

### 6. No active tab (Medium)

The Inspector and Matrix tabs are two equal orange buttons (`main.rs:3436`). Nothing shows which is open. The Inspector tab is then followed by an "Inspector" heading, so the word shows twice. Direction: a real tab style (underline or filled background for the active tab), and drop the repeated heading.

### 7. The inspector's empty state is a manual (Medium)

With nothing selected the inspector shows 18 lines of shortcuts, about 30 lines at 280 px. It is useful once and noise after. Directions: a short empty state ("Select a module to edit it. Press A to add one."), with the full list behind a "Keyboard shortcuts" link or in help (F1 with nothing selected).

### 8. Help overlay (Medium)

- The descriptor part (Inputs, Outputs, Parameters) scrolls in its own inner box, and the hand-written notes continue below it in the outer overlay. The overlay has two scroll regions, and the example patch is below the window edge. *Correction, found while fixing [#457](https://github.com/brylie/LoomDSP/issues/457): this was a misreading. The bar was the scroll thumb of the one scroll region, sized to the part shown. The cut text at the right edge, below, was real: the scrollbar was drawn over the text.*
- Body text is cut at the right edge ("without digita"), so long lines do not wrap inside the overlay.
- Sizes do not form a hierarchy: section titles 18 px for Inputs, 22 px for "When to use it"; body 13 px in one part and 15 px in the other (`help.rs:119-140`).
- The canvas behind is not dimmed, so the overlay floats over live tiles with no clear edge.
- Raw text: "unconnected: 0", "default 1000" with no unit, "0 to 1 , default" with a space before the comma, "audio, Bipolar" with mixed case.

Direction: one scroll region; one type scale (see 17); a dimmed backdrop; reuse the unit formatter the inspector already uses for ranges and defaults; order sections as summary, When to use it, Parameters, Inputs, Outputs, Example, so the user-facing text comes first.

### 9. Module browser highlight (Medium)

The highlighted row has an orange background, and its category and ID are drawn in a dim orange on it, so they disappear. The ID column is cut at the right ("loom.clock.transpor") and the scrollbar sits on top of it. Directions: use light text on the highlight; give the ID column room or drop it in favour of the description that already shows at the bottom; keep the scrollbar out of the text.

### 10. Layout grammar differs between tiles (Medium)

Tiles were laid out one at a time, and it shows:

- The caption above a radio group ("Mode", "Law", "Colour") sits above the group on some tiles and centred over the panel on others.
- Two-choice settings use a radio on most tiles (Wrap/Fold, Sample/Track, Above/Below, Linear/Exponential, Free/Synced), a select on Envelope, and a toggle on Logic (Invert). A two-choice radio takes 3 grid rows; a select or segmented control takes 2.
- Lights float: Envelope's Level light is alone in the top right; Clock and Clock Divide put "Fired" under the knobs; MIDI Note puts Gate in the middle of empty space.
- Some tiles leave large empty areas: Ladder (right of the preview and below Cutoff), Audio In, MIDI Controller.
- Audio In shows its four channel pairs as "1, 2, 3, 4" in a tall radio. "1-2, 3-4" or a select would be clearer and shorter.
- Mixer repeats "Level 1..4" and "Mute 1..4" under each strip. One row label per kind would read better.
- Button draws an empty grey box with "On" under it. The label belongs on the button.
- Level Meter's caption under the meter is "Out", the name of the bound output, which reads like a port label.

Direction: write a short layout guide in panels.md (order: preview, then choices, then knobs; captions centred on their widget; lights next to what they report; two-choice settings as one widget kind), then do one pass over all tiles with the screenshot grid as a before and after. The grid patch from this audit can become `examples/dev/all-modules.ron` so the pass is repeatable.

### 11. A knob at its minimum looks disabled (Medium)

A knob with its value at the bottom of its range has no orange arc, only the dark track. Hysteresis, Multiply, Divide, Drive, Gain on Audio In, and Channel all look switched off or greyed out at their defaults. Direction: always draw a small value tick in orange, or draw the track itself a little lighter, so a knob at zero still looks live. Colour must not be the only signal here anyway (NFR-UX-03).

### 12. Help text tone and jargon (Medium)

The summary line is the first thing in the browser, the inspector, hover text, and help, so it should speak to a musician.

- Implementation terms lead: "State variable filter (trapezoidal integration)", "(polyBLEP)", "(polyBLAMP)", "Delay-line all-pass filter", "Interpolated delay line". Move these to the notes and lead with what the module does.
- A requirement ID is in user text: Noise says "(NFR-SND-04)".
- Marketing tone: SVF's notes say "warm analog-style sweeps and clean resonance without digital cramping near Nyquist". This does not match the plain style AGENTS.md asks for, and "Nyquist" needs an explainer link.
- Envelope's Mode range reads "0 to 1, whole numbers, choices ADSR, AD". For a choice parameter the range should be just the choices.
- Constant's range line lists seven control ranges in one cell.

Direction: a short rule for summaries in port-and-parameter-text.md (lead with the job, no internal terms, no IDs), a CI check that rejects `NFR-` and `FR-` in descriptor text, and one editing pass.

### 13. The status bar mixes audiences (Medium)

"Measure", "Move camera", and "redraws on change" are developer tools and sit first. The MIDI field lists every input by full name, and with two controllers it pushes "no feedback loops" off the right edge ("no feedb"). Directions: move the bench toggles behind a developer flag or a menu; show "MIDI: 4 inputs" with the names in hover text; give the right-hand items a fixed place.

### 14. Inspector details (Low)

- The raw ID line "loom.filter.svf@1 #4" sits under the title. It helps developers; most users need it only in help.
- The port list at the bottom is one paragraph per port ("Input: In. The audio to filter. Usually -1 to 1."). A small two-column list (name, signal and range) with the description in hover text would scan better.
- "Add modulation" is dim text with no box or icon, so it does not look like a control.
- The polarity pick list ("Follow source (bipolar)") is wider than the depth slider it sits under. A short form ("Source ±") with the full text in hover would save space.

### 15. Category colours (Medium)

Oscillators, filters, envelopes, phase, delays, and MIDI each have a colour. Level, Logic, Math, Mix, Pitch, Random, I/O, UI, Display, and Modulation (LFO) all share grey. More than half the tiles in the gallery have grey headers, so the colour does not help find a tile. The LFO is grey while it sits next to coloured oscillators. Measured against the header text `#ECEEF2`, four headers fail 4.5:1: Envelope, MIDI, and Time at 4.4:1, and I/O at 4.3:1. Under simulation, I/O and Time are ΔE 1.5 apart for protanopia and 3.2 for deuteranopia, and Envelope and Time are 3.7 apart for tritanopia.

Direction: group the 17 categories into about eight families, each with one colour from a shared palette, and test contrast and colour-blind distance (`category_colour`, `scene.rs:37`). A search over muted hues found eight dark header colours at least ΔE 18 apart under all three simulations, so eight families are possible without bright headers. Details and candidates are in #459.

### 16. Module names (Low)

Most names are Title Case ("Phase Scale", "Level Meter", "Clock Divide"), but some are sentence case ("Phase wrap and fold", "Sample and hold"). Hyphen use varies ("All-Pass", "One-pole", "1-Sample Delay"). Three modules are called Constant, told apart by "(level)" and "(pitch)"; "Level Constant" and "Pitch Constant" would sort and search better. Port-and-parameter-text.md asks for Title Case labels, so this is a fix, not a decision.

### 17. No type scale (Low)

The app sets text size inline at 11, 12, 13, 14, 15, 17, 18, and 22 px across `matrix.rs`, `inspector.rs`, `settings.rs`, `help.rs`, and `main.rs`. The canvas uses 11 for titles, 9 for labels, and 7 for badges. A shared scale of four or five named sizes (caption, body, label, heading, title) in one module would make the side column and help consistent and is cheap to adopt.

### 18. Small fixes (Low)

- Envelope's shape preview line is drawn in visible steps at 100% zoom. It should be smooth like the LFO preview.
- Scope's display has no zero line or grid, so a signal's offset is hard to judge.
- MIDI Controller's Source radio has three choices next to two knobs and a third knob below; the tile is twice the height it needs.

### 19. Result curves look beaded and stepped when zoomed (High)

Found in a follow-up check at 1x, 2x, and 4x zoom. The Envelope shape, Ladder response, and LFO wave are not drawn as lines. `crates/loom-canvas/src/controls.rs` (around line 346) pushes one axis-aligned rounded rectangle per segment, sized to the segment's bounding box, for 128 segments (`SHAPE_SEGMENTS`, `crates/loom-app/src/scene.rs:16`). At 1x the line looks a little stepped. At 4x it is a row of separate beads, and the steep Envelope attack becomes a filled staircase.

Cables, knob arcs, modulation arcs, dashed modulation lines, and text stay smooth at 4x, because they use the Bezier strip and capsule pipelines. Scope traces already have an anti-aliased capsule pipeline (`trace.rs`, `fs_trace` in `canvas.wgsl`). Direction: draw previews as one continuous anti-aliased polyline, through that pipeline or a joined strip.

## What works well

- Cable and port colours by signal kind, with square gate ports, read at a glance.
- Shape previews on Envelope, Ladder, and LFO show the result of the knobs, which is rare and valuable.
- The modulation dashes and the badge on the source make assignments visible on the canvas without opening the matrix.
- Hover text gives the full label, description, and stable name everywhere, so cut labels are never a dead end.
- The headless screenshot, `--dump-ui`, and control script made this audit possible without a human at the window. The `shown` field for cut labels is a ready-made regression check.

## Suggested order

1. #454, the visual system. Later work then uses the final sizes, colours, and button roles.
2. #455, canvas readability. It touches every user in the first minute, and the curve fix is small.
3. #456, the matrix. It is the newest P1 feature and the least usable today.
4. #453, panel layout. Agree the principles first; the pass over all tiles is the largest piece.
5. #459, module families and the palette. It needs an ADR first, and it sets the header colours #453 lays out.
6. #460, so headless window screenshots can be trusted for the before and after images the other issues ask for. It is small; do it first if screenshots are needed early.
7. #457 and #458, the inspector, help, browser, and module text. These can run beside the others.
