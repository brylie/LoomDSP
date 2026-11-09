# Canvas Accessibility Specification

How a screen reader user gets an overview of a patch, follows a signal from module to module, and reaches a module's controls only when they want them; and how the same model draws a small patch map that a sighted user pans with.

- **Kind:** living specification.
- **Status:** Agreed (2026-11-09), M2. The answers to its open questions are in [Decisions made for this spec](#decisions-made-for-this-spec).
- **Decision behind it:** none of its own. Builds on [ADR-0041](../adr/0041-own-ui-layer.md) (our own UI layer, with its accessibility tree), [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md) (plain-data view models), [ADR-0014](../adr/0014-modulation-without-cables.md) (modulation without cables), [ADR-0002](../adr/0002-graph-execution-and-feedback.md) (feedback loops), and [ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md) (the editor's sub-patch levels).
- **Use case and requirements:** UC11, NFR-UX-06, NFR-UX-07, FR-CANVAS-06, FR-CANVAS-13.
- **Background:** [Reading a patch by ear](../learn/reading-a-patch-by-ear.md) (explainer), [overviews of node-link diagrams for screen reader users](../research/2026-11-screen-reader-graph-overviews.md) (research), [the accessibility spike](../research/2026-11-a11y-beside-iced-spike.md), [the large patch review](../research/2026-11-large-patch-ux-review.md) (finding 3, the overview has no words), [Feedback loops](../learn/feedback-loops.md).
- **Prior art:** the WAI-ARIA tree view pattern for the outline's keys, and "overview first, then details on demand" from information visualisation research. Named only to explain ideas; the research snapshot cites them.
- **Issues:** [#683](https://github.com/brylie/LoomDSP/issues/683) (the UI layer), [#527](https://github.com/brylie/LoomDSP/issues/527) (find a module and a readable overview), [#524](https://github.com/brylie/LoomDSP/issues/524) (cables by key).
- **Code:** none yet. The model will sit beside the other view models in `crates/loom-app`, and the canvas nodes in `crates/loom-canvas`. The implementation also adds each loop's run order to `FeedbackLoop` in `loom-core` (rule 7), and one module-name function in `loom-app` that every view uses (rule 4).

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

The maintainer's words: "I can visually scan the whole graph to see what connects to what, before I focus in on particular details. I would like for the accessibility layer to support this level of awareness, so that people don't need to navigate the graph so verbosely when they want to quickly trace a signal path."

The [UI layer specification](ui-layer.md) gives every tile, control, and port an accessibility node. That makes every control reachable, but only one at a time. In the spike, learning what fed the filter meant visiting the filter's tile, then guessing which tile was upstream and visiting that one. A sighted user answers the same question in a glance.

Example, with `subtractive.ron` open and VoiceOver on. The user presses O on the canvas. The outline opens and reads its first row:

> Subtractive. 11 modules, 11 cables, no feedback loops. 2 modules not heard.

They press Down and hear the modules in the order the signal flows, from the source to the output:

> MIDI Note. Source. To Phase, Envelope 5, and Envelope 7.
> ...
> Multiply 8. From SVF and Envelope 7. To Audio Out.
> Audio Out. From Multiply 8. The output.
> Not heard, 2 modules.

On Saw they press `]`, the downstream key, and hear "SVF". On SVF they press `]` again. SVF feeds two modules, so a short list opens: "Lowpass to Multiply 8 A", "Lowpass to Scope In". They pick Multiply 8, press Back, and are on SVF again. They press Enter and land on SVF's tile on the canvas, on its Cutoff knob. No control was visited on the way.

A sighted user beside them sees the same patch drawn small in the map in the canvas corner, with a box around the part the canvas shows. They drag the box to pan to another part of a large patch.

## Words

| Word | Meaning |
|---|---|
| **Patch map** | The plain-data model of one patch level, in two parts. Its **structure** (modules, connections, feedback loops, modulation assignments, which modules are heard, and the signal-flow order) is read by the outline, the map, and the canvas descriptions. Its **geometry** (tile rectangles and the camera's view rectangle) is read only by the map. |
| **Map** | The visual view of the patch map: the level drawn small, with names and connections and no controls. |
| **Outline** | The text view of the patch map: rows a keyboard and a screen reader move through. |
| **Row** | One line of the outline: the summary, the Not heard section, a feedback loop, a module, or one connection of a module. |
| **Heard** | A module with a path to the audio output, over cables and modulation assignments. Every other module is **not heard**. |
| **Signal-flow order** | Sources first, then each module after everything that feeds it (rule 6). |
| **Jump** | Moving from a module to one at the other end of one of its connections. |
| **Back** | Returning to where the last jump started. |
| **Level** | The part of the patch the canvas shows: the top level, or the inside of one sub-patch (ADR-0039). |

## 1. The patch map model

**Status:** Agreed, M2.

### Behaviour

The app builds one `PatchMap` from the level being edited, beside its other view models. Both views read it, and nothing else describes the graph's structure to them.

Structure:

| Field | From |
|---|---|
| Modules: UI ID, name, kind, family, role (source, output, display) | The level's tiles: graph modules, sub-patch instances, and port tiles (ADR-0039) |
| Connections: from module and port, to module and port, signal type, stereo or mono | Cables at this level, including boundary records to port tiles |
| Modulation: assignment ID, source module and output, target module and parameter, depth as the screen shows it, and whether it is on or off | Assignments whose source and target are both on this level. The source is an output port (`OutputRef`), so a module with several outputs can say which one modulates |
| Feedback loops: the modules in each, in the order the loop runs, and what breaks each cycle: a delay inserted on a cable, a delay inserted on an assignment, or the user's own unit delay | The compile report's `FeedbackLoop`s, mapped to this level's tiles. Today `FeedbackLoop::modules` is sorted by ID, and the run order lives only inside the compiler (`LoopOrder`, `compile/steps.rs`). The implementation adds the run order to `FeedbackLoop` |
| Heard or not heard, for each tile | The patch map's own walk backwards from the audio output, over the flat graph's cables and assignments, including assignments that are off. A graph module or an instance is heard if any flat module it stands for is. A port tile has no flat module: it is heard if any flat cable through it is. The compiler's reachability is not used: it also counts outputs probed for port activity or shown on screen, which depend on a preference and on the camera, so it would change the outline as the user pans |
| Signal-flow order | Rule 6 |

Geometry:

| Field | From |
|---|---|
| Tile rectangles | The level's tile positions and sizes |
| The camera's view rectangle | The canvas camera |

### Rules

1. **One model for the map, the outline, and the canvas descriptions.** The map draws the patch map, the outline lists it, and each tile's and port's accessibility description (section 6) reads its connections from it. *Why:* the UI layer's first rule for the tree is that it and the screen come from one source (ui-layer.md, section 7, rule 1). A map that showed a cable the outline did not mention would be worse than no map.
2. **The structure is rebuilt only when the level's structure changes:** a module, cable, assignment, label, the level, or the feedback loops. Moving a tile changes only the geometry, and so does moving the camera. Neither rebuilds the structure. When a move ends (the drag is released, or a group of arrow-key moves ends, as for undo), the tie order of rule 6 is computed again, and only rows whose position changed are sent; focus stays on its row by UI ID. *Why:* the UI layer decides a panel changed by comparing its model (ui-layer.md, section 3), so the outline must read a part that a pan does not touch (NFR-PERF-04). A screen reader must not hear the outline refresh while a tile is dragged, and recomputing at the end of the move keeps ties in the reading order the user now sees.
3. **It never edits the patch.** Every action in the outline or on the map becomes an ordinary message: a selection, a camera move, or focus. *Why:* undo, autosave, and the control channel then need nothing new (ui-layer.md, section 7, rule 6).
4. **A module has one name, used everywhere:** the tile header, the inspector, the matrix, MIDI learn, undo text, and the patch map, written and spoken the same. It is the module's label if it has one, once labels are shown ([#526](https://github.com/brylie/LoomDSP/issues/526)); else its type name, plus its module ID at this level when the level has more than one module of that type, with no "#": "Envelope 5", but "MIDI Note". **This changes what the screen shows today:** tile headers show the bare type name (`scene.rs`), and the inspector and matrix say "Envelope #5", always with the number (`modulation.rs`, `module_name`). The implementation replaces both with one name function, and updates the specs that still show the "#5" form (the inspector, the matrix, MIDI learn, and undo text) in the same pull request. *Why:* the user must hear the name they would see (rule 14), so every view must show the same one. "Envelope number 5" is slow to listen to, and "Envelope hash 5" is worse.

## 2. Levels of detail

**Status:** Agreed, M2.

The outline gives four levels, from the coarsest. The user moves down a level only when they want to.

| Level | What it says | Where |
|---|---|---|
| 1. Summary | Patch name, module and cable count, feedback loops, modules not heard | The outline's first row |
| 2. Signal flow | One row per module, in signal-flow order, with what feeds it and what it feeds | The outline's module rows |
| 3. Connections | One row per connected port or modulated parameter of one module, each a jump | A module row's children, shown when it is expanded |
| 4. Controls | The module's knobs, switches, and the rest | The tile on the canvas, reached with Enter |

### Rules

5. **Controls are not in the outline.** Enter on a module row moves keyboard focus to its tile on the canvas, on its first control, or on the tile itself when it has none, where the existing panel keys apply ([panels.md, Keyboard without a mouse](panels.md#keyboard-without-a-mouse)). *Why:* a second copy of every control would be a second place to edit the same value, and the outline would become as long as the walk it is meant to save.

## 3. Signal-flow order and sections

**Status:** Agreed, M2.

### Rules

6. **Modules are listed in signal-flow order,** built by one step repeated until every module is listed: among the modules and loops whose feeders are all listed, take the first in reading order. A feeder is a module with a cable or a modulation assignment into this one, as the compiler counts them. A loop counts as one unit: it is ready when everything outside it that feeds it is listed, and its place in reading order is its topmost member, then its leftmost. Reading order is top to bottom by tile top, then left to right, then by module ID ([panels.md](panels.md#keyboard-without-a-mouse)). Heard modules are ordered this way at the top level, and not heard modules the same way inside their section. *Why:* sources first and the output last is the order a musician describes a patch in ("the saw goes into the filter, then the amp"). The schedule itself breaks ties by ID, which means nothing to the user; reading order matches what a sighted user scans. One step, stated once, gives one order an implementer cannot read two ways.
7. **A feedback loop is one row, with its modules as children,** in the order the loop runs. Its name is "Feedback loop", its number, and its modules: "Feedback loop 1: SVF, Delay, Mix". Loops are numbered by their place in the outline, from 1. Its description says what breaks the cycle: "Delay inserted from Mix to SVF" for a delay the compiler put on a cable, "Delay inserted on LFO's modulation of SVF cutoff" for one it put on an assignment, and "Unit delay: Delay 1" when the user's own unit delay breaks every cycle and nothing was inserted. *Why:* in a loop, "what comes first" has no answer ([Feedback loops](../learn/feedback-loops.md)). Grouping says so, and the compiler already reports the loop as one unit. Where the delay sits is what a user who built a loop on purpose wants to check.
8. **Heard modules are rows at the top level of the outline. Every module that is not heard is in one section at the end,** a row named "Not heard" with its count, with those modules as its children. With every module heard, there is no section row. A module can be not heard and still run: a scope or a light is shown on screen but sends nothing to the output. *Why:* "what do I hear" is the first question, and a module that is not heard is a common reason for silence. Two groups need no new idea: "heard" means what it says.

   **First state:** the Not heard section and every loop row start expanded; module rows start collapsed. *Why:* Down from the summary then walks through every module, which is the scan the outline is for. A module's connections are one Right away.
9. **A sub-patch instance is one module row,** named as its tile, with "sub-patch, 6 modules inside" in its description. Enter focuses its tile, as on every module row (rule 5), so its exposed controls are reached the same way. Cmd/Ctrl+Enter opens it, as on the canvas ([sub-patches](sub-patches.md)), and the outline then shows the inside. Inside a sub-patch, the summary names where the user is: "Inside Filter bank, in Subtractive. 6 modules, 7 cables, no feedback loops." *Why:* the outline follows the level the canvas shows (ADR-0039), so the two never disagree about where the user is, and the same keys do the same thing in both.

## 4. What a row says

**Status:** Agreed, M2.

A row has a short **name**, read at once, and a **description**, read after it or after a pause, as the screen reader is set. Values with units follow the [port and parameter text](port-and-parameter-text.md) rules.

| Row | Name | Description |
|---|---|---|
| Summary | "Subtractive. 11 modules, 11 cables, no feedback loops. 2 modules not heard." | None |
| Not heard section | "Not heard, 2 modules" | None |
| Module | "SVF. From Saw and Multiply 6. To Multiply 8 and Scope." | "Filter. Cutoff modulated by LFO. In feedback loop 1." |
| Source module | "MIDI Note. Source. To Phase, Envelope 5, and Envelope 7." | Its kind |
| Output module | "Audio Out. From Multiply 8. The output." | Its kind |
| Loop | "Feedback loop 1: SVF, Delay, Mix" | "Delay inserted from Mix to SVF." |
| Connection, input | "In, from Saw out" | Signal type, stereo or mono |
| Connection, output | "Lowpass, to Multiply 8 A and Scope In" | Signal type, stereo or mono |
| Modulation, on a target | "Cutoff, modulated by Envelope 5, 40%" | None |
| Modulation, on a source | "Modulates SVF cutoff, 40%" | None |
| Modulation that is off | "Cutoff, modulated by Envelope 5, 40%, off" | None |

### Rules

10. **A module row names its neighbours, not its ports.** "From Saw" is enough to trace; which port comes at level 3. *Why:* the row is read once per module while scanning. Port names double its length.
11. **At most three neighbours are named on each side,** then "and 4 more". *Why:* a mixer with twelve inputs would otherwise be one long sentence. The connection rows list them all.
12. **A neighbour reached by more than one cable is named once.** *Why:* "From MIDI Note" says what feeds it; the two cables from MIDI Note's pitch and gate are a port-level detail.
13. **Modulation is in the description, not the name,** on a module row; and in the connection rows. *Why:* modulation has no cable (ADR-0014), so the user cannot find it by following one, but naming it on every row would slow the scan. A sighted user sees it as a badge, not as a line, which is the same weight.

    **An assignment that is off is still listed, and says "off" last** ("Cutoff modulated by LFO, off"). It keeps its place in the signal-flow order and in the walk that finds heard modules. *Why:* the compiler keeps a switched-off assignment in the schedule and changes only its enabled flag (`compile/schedule.rs`), so turning it back on moves nothing, and the outline should not reorder either. The source badge already counts assignments that are off ([modulation](modulation.md)), and a user who cannot hear them could not find one to turn back on. "Off" comes last, so the scan hears what is connected first. On the map, a dotted line instead of a dashed one marks it by shape, not only by colour (NFR-UX-03).
14. **Every word is our own, and the same words the screen shows.** *Why:* ui-layer.md, section 7, rule 3.

## 5. Keys in the outline

**Status:** Agreed, M2.

The outline is an **Outline** tab in the right column, beside Inspector and Matrix. O on the canvas, with no text field focused, opens the tab and moves focus to the outline's focused row, or to its first row the first time. Ctrl+Tab moves focus to the next region (canvas, right column, toolbar) and Ctrl+Shift+Tab to the previous one, on every platform, and lands on the outline when its tab is open (ui-layer.md, section 6, rule 7). A click on the tab opens it too, so sighted users can read the outline as well.

The outline is one focus stop. While it has focus, it takes these keys before the app's shortcuts (ui-layer.md, section 6, rule 6). The navigation keys follow the WAI-ARIA tree view pattern, so screen reader users meet what they expect from a file tree.

| Key | Action |
|---|---|
| Down, Up | Next or previous visible row |
| Right | On a collapsed row, expand it. On an expanded row, go to its first child |
| Left | On an expanded row, collapse it. On a child row, go to its parent |
| Home, End | First or last visible row |
| Letters and digits | Type-ahead: go to the next row whose name starts with what was typed. Characters typed less than 1 second apart join one search, so "aud" finds Audio Out. Space is not part of type-ahead; it selects (rule 18) |
| Enter | On a module row, including a sub-patch, focus its tile on the canvas, on its first control or on the tile. On a connection row with one module at the other end, jump there; with several, open the list of rule 23 |
| Cmd/Ctrl+Enter | On a sub-patch row, open it |
| Space | Select the module on the canvas and centre the canvas on it, and stay in the outline |
| `[`, `]` | Follow upstream, follow downstream: jump to the module that feeds this one, or that this one feeds (section 7) |
| Cmd+`[` (Ctrl+`[` on Windows and Linux) | Back: return to where the last jump started (section 7) |
| Escape | Close the jump list if it is open. Else, inside a sub-patch, go up a level, to the instance's row, as Escape with nothing selected does on the canvas. Else leave the outline and focus the canvas where it was. The tab stays open |

### Rules

15. **The app handles every key in the outline itself,** and keeps the expanded rows. It does not rely on the screen reader or the platform to expand rows or move between them. *Why:* `accesskit_macos` 0.27.2 has no expand or collapse action and does not report a row's level, and the Linux adapter was not found to either (research, section 4). Keys we handle work the same on every platform, and for keyboard users with no screen reader. The outline's nodes are built flat, as ui-layer.md, section 7, rule 12 says: the visible rows are direct children of the Tree node, in display order, each with its level, position in set, size of set, and expanded state. A collapsed row's children are not in the tree.
16. **Expanded rows and the focused row survive a rebuild,** keyed by UI ID. If the focused module is deleted, focus goes to the next row, or the previous one at the end. *Why:* ui-layer.md, section 4, rule 3, and a user must not be thrown to the top of the outline because a cable changed elsewhere.
17. **Delete and Backspace do nothing in the outline.** *Why:* the outline is for reading. A user who is listening must not remove a module by a key that, on the canvas, deletes the selection.
18. **Space selects but Enter moves focus.** *Why:* the user can look at a module on the canvas without losing their place in the outline. Neither changes the patch.

## 6. The canvas: tiles and ports

**Status:** Agreed, M2.

The canvas gives a node per tile, control, and port (ui-layer.md, section 9, rule 3). This section adds what those nodes say about the graph.

### Rules

19. **A tile's description names what feeds it and what it feeds,** with the same words as its module row in the outline: "From Saw and Multiply 6. To Multiply 8 and Scope." Its name stays the module's name. *Why:* a user on the canvas hears where they are in the signal path without opening the outline. Putting it in the description, not the name, keeps Tab between tiles quick, since the screen reader reads the description after a pause or on request.
20. **A port's name is its label, and its description names the far end:** "In. From Saw out." An unconnected port says "Not connected" and, for an input, its default ("Not connected, 0"). *Why:* the same question as rule 19, one level down.
21. **An output port node has a `controls` relation to the input port nodes its cables feed,** and to nothing else. *Why:* `controls` is the only AccessKit relation all three platform adapters map (research, section 4): on macOS to linked UI elements, which VoiceOver jumps along, and on Windows and Linux to "controller for". A cable from an output does control what reaches the input, so the meaning holds in that direction only. **Departure:** the WAI-ARIA relation for reading order across elements is `flowto`, and it would fit better, but none of the adapters maps it. This rule is a bonus path; the keys in section 7 are the one users are told about.
22. **A tile's position in signal-flow order is not its Tab order.** Tab keeps reading order ([panels.md](panels.md#keyboard-without-a-mouse)). *Why:* Tab must follow what a sighted keyboard user sees. The outline is where signal-flow order lives.

## 7. Following a cable

**Status:** Agreed, M2.

Following a cable works in the outline and on the canvas, with the same keys: `[` follows upstream, `]` follows downstream, and Cmd+`[` (Ctrl+`[` on Windows and Linux) goes back. On the canvas they act on the focused port, else the focused control's module, else the selected module, and do nothing with no module selected or with more than one. *Why:* the arrow keys already move tiles and Alt-arrow resizes them ([panels.md](panels.md#keyboard-without-a-mouse)). The brackets are free, sit together, and point left and right like the signal on the canvas, and Cmd+`[` is back in browsers.

The keys bind by **physical position**: the two keys to the right of P on a US layout, whatever character they type on the user's layout. The keymap ([#494](https://github.com/brylie/LoomDSP/issues/494)) can change them. *Why:* on Finnish, other Nordic, and German layouts, `[` and `]` need AltGr or Option, so Cmd+`[` would be a three-key chord. The app matches most keys by character today (`Key::Character` in the canvas key handler), so these need the physical key instead.

### Rules

23. **Follow upstream** jumps to the module that feeds the current one. **Follow downstream** jumps to the module it feeds.
    - On a connection row or a focused port, it follows that port's cable or cables.
    - On a module row or a selected tile with exactly one neighbour on that side, it jumps there.
    - With more than one, a short list opens, one row per neighbour with its ports ("Saw out to In", "Multiply 6 out to Cutoff"), in signal-flow order. Up, Down, and Enter pick one; Escape closes it. Modulation sources and targets are in the list too, marked "modulation".
    - With none, nothing moves, and the screen reader hears "Nothing upstream" or "Nothing downstream".

    *Why:* a jump that guessed among several cables would take the user somewhere they did not choose, and they could not tell from the sound alone. The list says how many there are and lets them pick, as the planned cable-by-key list does (#524).
24. **On the canvas, a jump selects the target module and centres the canvas on it.** If the jump started from a port, focus lands on the far port; otherwise on the tile. In the outline, a jump moves focus to the target's row. If that row is inside a collapsed section or loop, those rows are expanded first, since a collapsed row's children are not in the tree (rule 15); nothing else is expanded. *Why:* the sighted user beside them sees where the jump went, and a jump from a port is usually followed by another along the same chain.
25. **Back returns to where the last jump started,** in the same view. Jumps form a list of up to 50 places, kept until the level changes; a place whose module was deleted is skipped. Back with no earlier place says "No earlier place". *Why:* tracing a signal is trying one branch, then the other. Without Back, the user must find their way to the fork again.
26. **Jumps never change the patch, and are not undo steps.** *Why:* rule 3.

## 8. The map

**Status:** Agreed, M2.

### Behaviour

The map sits in the bottom-right corner of the canvas, over it, at most 200 by 150 logical pixels and never more than a quarter of the canvas's width or height, on a panel background so the tiles under it do not show through. It draws the whole level small: every tile as a box in its place on the canvas, with its name, coloured by family; one line for each pair of connected modules, coloured by signal type and thicker for stereo; modulation as a dashed line from source to target, and modulation that is off as a dotted line; and a mark on modules in a feedback loop. It draws no controls, ports, or live values. A rectangle shows the part of the level the canvas shows.

| Gesture | Result |
|---|---|
| Click on the map | Centre the canvas there |
| Drag the view rectangle | Pan the canvas with it |
| Click on a module's box | Select it and centre the canvas on it |
| Scroll over the map | Zoom the canvas, about the point under the pointer |

| Setting (Preferences, and the toolbar's Map toggle) | When the map shows |
|---|---|
| Auto (default) | While the level does not fit in the canvas view at the current zoom |
| Always | Always |
| Hidden | Never |

M on the canvas, with no text field focused, moves through the three settings, and the status line says which one is now set. The setting is per machine ([preferences](preferences.md)), not per patch.

### Rules

27. **Boxes are placed as tiles are on the canvas,** not in signal-flow order. *Why:* the map's job is finding your way on the canvas. A sighted user looks for "the filter at the top right", and the map must agree.
28. **Names on the map are drawn at a readable size or not at all,** from the visual system's text tokens. When a name does not fit its box, it is cut with "…", and when no name fits, boxes are drawn without names. *Why:* the large patch review found an overview with no words useless as a map; a box with a name too small to read is not better.
29. **The map is one image node in the accessibility tree,** named "Patch map", whose description points to the outline. It has no nodes of its own. *Why:* WCAG 1.1.1 asks for a text alternative for a complex image. The outline is that alternative, and a second set of nodes for the same modules would be read twice.
30. **Auto shows the map only while it helps.** When the whole level fits in the view, the canvas already is the map. *Why:* a map of what is already on screen takes room and adds nothing; a large patch is where finding your way is hard.
31. **The map takes the pointer over its own area,** and the canvas gets no pointer events there (ui-layer.md, section 5, rule 1). A tile under the map is reached by panning, or the map is hidden with M. *Why:* the user sees the map on top, so it must take the click.
32. **The map is redrawn only when the patch map changes or the camera moves.** The camera moves only its view rectangle. *Why:* NFR-PERF-04, and ADR-0010's rule that pan and zoom never rebuild drawing data.

## 9. Updates while the user listens

**Status:** Agreed, M2.

### Rules

33. **The outline's nodes are sent when the patch map is rebuilt, and only those that changed.** *Why:* ui-layer.md, section 7, rule 7.
34. **A change to the patch is not announced from the outline.** The edit's own feedback (the status line, the tile) says what happened. *Why:* the outline is a view of the patch, not a second voice for it. An announcement each time a cable moved would interrupt the user.
35. **A module row's name does not include live values.** *Why:* a name that changed with the signal would be re-read whenever it changed, and the outline is about structure.

## 10. Tests and the control channel

**Status:** Agreed, M2.

### Rules

36. **`App::ui_snapshot` and `--dump-ui` include the patch map,** as rows with their names and descriptions. *Why:* agents and tests then check what a screen reader user hears with no screen reader, as they do for the rest of the window.
37. **The outline's rows have UI IDs** from section 4 of the UI layer spec: a module row keyed by its module, a connection row by the module and port, a modulation row by its `AssignmentId` (one output can modulate several parameters, so the port alone does not tell its rows apart), a section and a loop by a fixed name and the loop's lowest module ID. The control channel's `locate` accepts them. *Why:* one identity scheme (ui-layer.md, section 4, rule 1).

## Acceptance criteria

- [ ] A test builds the patch map of `subtractive.ron` and checks the goal's example: the summary row; MIDI Note first and Multiply 8 then Audio Out last among the heard rows; every module after all its feeders; and the Not heard section with Scope and Transport.
- [ ] A test of rule 6's tie-break: sources A and C, B fed by A, with A reading first and B reading before C. The order is A, B, C (not A, C, B, which sorting by depth would give).
- [ ] A test checks the first state: the Not heard section and loop rows are expanded, module rows are collapsed, and Down from the summary reaches every module.
- [ ] A test checks that the tile header, the inspector, the matrix, and the outline name a module the same way: "Envelope 5" with two envelopes, "MIDI Note" with one.
- [ ] A test with a feedback loop checks that the loop is one row named with its number, its modules are its children in run order (not ID order), and its description names the inserted delay. A loop broken by the user's unit delay names that delay, and a loop broken on an assignment names the assignment.
- [ ] A test checks that a module modulated by another, with no cable, lists the modulation in its description and in its connection rows, and comes after its source in signal-flow order.
- [ ] A test checks the sections: a module that feeds only a scope is in Not heard, and a patch where every module is heard has no section row. The sections are the same with port activity on and off, and after a pan.
- [ ] A test with a twelve-input mixer checks that its row names three neighbours and "and 9 more", and its connection rows list all twelve.
- [ ] `Ui` harness: Down, Up, Right, Left, Home, End, and letters move as section 5 says, with no screen reader.
- [ ] `Ui` harness: Enter on a module row focuses that tile's first control on the canvas; Escape on the canvas does not return to the outline, and O does, to the same row.
- [ ] `Ui` harness: follow downstream from Saw reaches SVF, and Back returns to Saw, in the outline and on the canvas. On SVF, which feeds Multiply 8 and Scope, the list opens and Enter picks one.
- [ ] `Ui` harness: a jump to a module inside a collapsed section or loop expands it and focuses the module's row. Enter on an output connection row with two targets opens the list.
- [ ] A test: inside a sub-patch, a port tile whose cables reach the audio output is heard, and one whose cables do not is in Not heard.
- [ ] A test: one LFO output modulating two parameters gives two source-side rows with different UI IDs.
- [ ] A test: an assignment that is off is listed with "off" at the end of its row, keeps its module's place in signal-flow order, and is drawn dotted on the map. Turning it on changes only that word and the line style.
- [ ] A test: moving an independent tile above another reorders their rows when the move ends, and focus stays on the same row.
- [ ] `Ui` harness: Delete in the outline removes nothing.
- [ ] A test checks that moving a tile or the camera does not rebuild the patch map's structure, and that adding a cable does.
- [ ] A test checks that a tile's description and its module row name the same neighbours.
- [ ] A test checks that an output port's node has a `controls` relation to every input its cables feed.
- [ ] `Ui` harness: a click on the map centres the canvas there, and dragging the view rectangle pans it.
- [ ] `Ui` harness: with the Auto setting, the map shows while the level overflows the view and hides when it fits; M moves through Auto, Always, and Hidden, and the setting is saved in preferences.
- [ ] `Ui` harness: O on the canvas opens the Outline tab and focuses its first row; Ctrl+Tab reaches it; at the top level, Escape returns to the canvas with the tab still open.
- [ ] `Ui` harness: with an outline row focused, Ctrl+Tab and Ctrl+Shift+Tab move to the next and previous region, and the outline takes neither key (ui-layer.md, section 6, rule 7).
- [ ] `Ui` harness: on the canvas, `]` on a selected Saw selects SVF and centres it, and Cmd+`[` (Ctrl+`[`) returns to Saw. With a German layout, the same physical keys do the same.
- [ ] Inside a sub-patch, the outline shows the inside, with the port tiles as source and output rows, and the summary names the sub-patch and its path.
- [ ] `Ui` harness: Enter on a sub-patch row focuses its tile, Cmd/Ctrl+Enter opens it, and Escape inside goes up to the instance's row.
- [ ] A VoiceOver check, recorded in the pull request: the summary, a scan of `subtractive.ron`, a jump and Back, Enter to a tile, and what VoiceOver says about a row's level and position (rule 15). Narrator and Orca checks are recorded, or listed as not checked.

## Out of scope

- **Editing from the outline.** Connecting, disconnecting, and adding modules by key belong to [#524](https://github.com/brylie/LoomDSP/issues/524). Its cable list should share the jump list's rows (rule 23).
- **Listening to a point in the path,** such as soloing a module's output to hear what the signal sounds like there. Useful for everyone, and it needs its own spec.
- **Sonified overviews,** such as a tone per module. The research snapshot notes this work; it is not proposed here.
- **Comments and frames** in the outline. When annotations arrive ([#512](https://github.com/brylie/LoomDSP/issues/512)), a frame may become a section; that is decided in their spec.
- **A map of the whole hierarchy at once.** The map and the outline show one level, as the canvas does.

## For the UI layer specification

The hooks this spec needs are in [ui-layer.md](ui-layer.md), merged with PR [#697](https://github.com/brylie/LoomDSP/pull/697): region keys in section 6, rule 7; the app-driven tree in section 7, rule 12; relations by UI ID in section 7, rule 13 and section 9, rule 7; focus into a custom element's part in section 9, rule 6; and the image row in section 7's role table.

## Decisions made for this spec

Asked and answered on 2026-11-09, the last four after a code review of this pull request. Each is written into the rules named.

| Question | Decision | Where |
|---|---|---|
| Where the map and the outline live | The map in the bottom-right corner of the canvas. The outline in an Outline tab of the right column, opened by a key | Sections 5 and 8 |
| When the map shows | Auto by default: only while the level does not fit in the view. A toolbar toggle, a key, and Preferences give Always and Hidden | Section 8, rule 30 |
| The outline's order | Forward, from sources to the output, in signal-flow order | Rule 6 |
| The keys to follow a cable | `[` upstream, `]` downstream, Cmd+`[` (Ctrl+`[`) back, in the outline and on the canvas; Ctrl+Tab and Ctrl+Shift+Tab move between regions | Sections 5 and 7 |
| The keys to move between regions | Ctrl+Tab forward and Ctrl+Shift+Tab back, with Ctrl on every platform, including macOS. Not F6: on most laptop keyboards the F keys need Fn | Section 5, and ui-layer.md, section 6, rule 7 |
| What the outline groups modules by | Two groups only, heard and not heard, from the patch map's own walk from the audio output. A "seen only" group was dropped: the maintainer asked to keep it simple, and the compiler's idea of seen changes with a preference and the camera | Rule 8, and the fields table |
| Module names | One name everywhere, "Envelope 5", with the number only when a type repeats and no "#". Tile headers and the inspector change to match | Rule 4 |
| How the bracket keys bind | By physical position, changeable in the keymap | Section 7 |
| Enter on a sub-patch row | As on the canvas: Enter focuses the tile, Cmd/Ctrl+Enter opens it, Escape inside goes up | Rule 9, section 5 |
| Milestone | M2 for both the map and the outline. The outline needs the UI layer's phase 1 tree; the map needs only the canvas | FR-CANVAS-13, NFR-UX-07 |

The letters O (open the outline) and M (cycle the map setting) were chosen while writing the answers in, as the canvas's single-letter keys A and F already are. Neither was in use. They are listed for the keymap in [#494](https://github.com/brylie/LoomDSP/issues/494).
