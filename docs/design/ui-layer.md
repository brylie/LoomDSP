# UI Layer Specification

How LoomDSP's own UI layer turns events into messages, decides what to rebuild, lays out and draws panels, moves keyboard focus, and exposes an accessibility tree, for the desktop app and for other tools that use the layer.

- **Kind:** living specification.
- **Status:** Draft. Each section below has its own status. Phase 1 of the plan may start once the sections marked "needed for phase 1" are Agreed.
- **Decision behind it:** [ADR-0040](../adr/0040-own-ui-layer.md), superseding [ADR-0009](../adr/0009-ui-stack-iced.md) for the window and widgets, and [ADR-0028](../adr/0028-app-accessibility-waits-for-iced.md). Builds on [ADR-0010](../adr/0010-own-patch-canvas.md) (our own canvas) and [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md) (plain-data view models).
- **Use case and requirements:** NFR-UX-01 (keyboard), NFR-UX-06 (screen readers), NFR-PERF-04 (no redraw when idle), FR-SDK-06 (publishable crates), OQ-13 (plugin windows), OQ-14 (Iced churn).
- **Background:** [how a UI layer decides what changed](../research/2026-11-ui-change-propagation.md), [the accessibility spike](../research/2026-11-a11y-beside-iced-spike.md), [our own UI stack instead of Iced](../research/2026-11-own-ui-stack-tradeoffs.md). An explainer in `docs/learn/` follows in phase 4.
- **Prior art:** the Elm architecture (one message type, one update), and the rebuild-and-compare approach that Levien describes for Xilem. Named only to explain ideas.
- **Issues:** [#683](https://github.com/brylie/LoomDSP/issues/683). The step-by-step work is in [the plan](../dev/leaving-iced-plan.md).
- **Code:** `crates/loom-ui-core` and `crates/loom-ui`, once they exist. Until then, `crates/loom-app` and `crates/loom-canvas`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

A blind musician opens a patch with VoiceOver on. They hear the toolbar, move into the inspector with Tab, hear "Cutoff, 800 hertz, slider", and turn it with the arrow keys. A sighted user beside them sees the same focus ring move. An agent drives the same app through the control channel, and a test checks the same thing headlessly.

All three work because one runtime, one set of view models, and one identity scheme sit under the window, the accessibility tree, the control channel, and the tests. This specification says how that layer behaves. It does not change what the app looks like: every view looks the same after each phase.

## Words

| Word | Meaning |
|---|---|
| **App** | The program built on the layer. For LoomDSP, `loom_app::App`. The layer knows it only through a trait. |
| **Runtime** | The code that takes events, calls the app, runs effects, rebuilds what changed, and asks for redraws. The same code in the window and in the harness. |
| **Driver** | What feeds the runtime events and shows its output: the `winit` shell, or the headless `Ui` harness. |
| **Message** | The app's own type for "something happened that may change state". The only way app state changes. |
| **Effect** | A plain value the app returns from `update` asking the shell to do something outside the app: focus a field, use the clipboard, open a file dialog, set a timer, quit. |
| **View model** | A plain-data struct the app builds from its state, describing what one panel shows (ADR-0021). |
| **Panel** | A part of the window with its own view model and its own place in a layer: the toolbar, the stats bar, the inspector, the module browser, an overlay. The unit of rebuild. |
| **Element** | One node of the tree a panel builds from its view model: a row, a button, a slider, a text. Elements are rebuilt when their panel is rebuilt. |
| **Widget state** | State that belongs to one element and lasts across rebuilds: hover, pressed, scroll offset, caret and selection in a text field, an open list. Not app state. |
| **Custom element** | An element that owns its own drawing, input, and accessibility nodes, such as the patch canvas. |
| **Layer** | A stacking level of the window: canvas, panels, overlays, question. |
| **UI ID** | The stable identity of an element across rebuilds. |

## 1. Crates and what they may know

**Status:** Draft. Needed for phase 1.

| Crate | Holds | May depend on |
|---|---|---|
| `loom-ui-core` | Geometry (`Point`, `Size`, `Rect`, `Vector`, `Color`), input events (`PointerEvent`, `KeyEvent`, `Modifiers`, `Key`, `ScrollDelta`), UI IDs | No UI toolkit, no LoomDSP crate |
| `loom-ui` | Runtime, effects, elements, layout, focus, the accessibility tree, the `winit` shell, the widgets, and the drawing of panels | `loom-ui-core`, `winit`, `wgpu`, the text crate, `accesskit`, `accesskit_winit` |
| `loom-canvas` | The patch canvas, as a custom element | `loom-ui-core`, later `loom-ui` |
| `loom-app` | The app: messages, `update`, view models, panels | All of the above |

Rules:

1. **`loom-ui-core` and `loom-ui` know nothing about patches.** They never depend on `loom-core`, `loom-patch`, `loom-canvas`, or `loom-app`, and they use no patch words in their API. *Why:* other tools should be able to use them, and publishing them later (FR-SDK-06) should not need a second refactor.
2. **The runtime and the shell take the app as a trait or a generic parameter,** never as `loom_app::App`. *Why:* the same reason.
3. **Each crate has a crate-level doc comment and an example program that does not use LoomDSP.** *Why:* the example proves rule 1, and it is the first documentation a new user reads.
4. **The engine crates never depend on UI crates.** `loom-core`, `loom-dsp`, `loom-modules`, `loom-patch`, and `loom-rt` stay portable (AGENTS.md, hard rule 6).
5. **Every new crate passes `cargo deny check` before it is used** (ADR-0006).

## 2. The runtime: events, messages, and effects

**Status:** Draft. Needed for phase 2.

### Behaviour

One turn of the runtime:

1. An **event** arrives from the driver: a window event, pointer, key, IME, a redraw request, a wake-up from another thread, or a deadline that has passed.
2. The runtime **routes** it (section 5). The element that takes it either handles it as widget state, or turns it into a **message**.
3. For each message, the runtime calls **`update`** once. `update` changes app state and returns **effects**.
4. The runtime **runs the effects** after `update` returns. An effect that produces a message (a dialog closed, a timer fired) queues it for a later turn.
5. The runtime asks each panel whether its view model changed, and **rebuilds** only the panels that did (section 3).
6. It redoes **layout** for rebuilt panels, updates the **accessibility tree** for changed nodes, and asks the driver for a **redraw** only if something visible changed.
7. With nothing queued and nothing visible to change, the loop **sleeps** until the next event or deadline.

A sketch of the contract the app implements. It is here to make the rules concrete. The names are not binding; the rules are.

```rust
/// The app, as the runtime sees it.
pub trait App {
    /// The app's message type: the only way its state changes.
    type Message;

    /// Applies one message and returns what the shell should do next.
    fn update(&mut self, message: Self::Message) -> Effects<Self::Message>;

    /// Describes the window, one panel at a time. The runtime skips the
    /// build of any panel whose model equals the last one.
    fn view(&self, ui: &mut View<Self::Message>);

    /// The next moment the app needs to wake with no event, if any.
    fn next_deadline(&self) -> Option<Instant>;
}

// Inside `view`: a panel is a key, a model, and a builder.
ui.panel(PanelKey::INSPECTOR, Layer::Panels, &inspector_model, |model, el| {
    // build elements from `model` only
});
```

### Effects

| Effect | What the shell does | What the headless harness does |
|---|---|---|
| Focus an element, by UI ID | Moves keyboard focus | The same |
| Scroll an element into view | Changes its scroll offset | The same |
| Clipboard read or write | Uses the system clipboard | Uses its in-memory clipboard |
| Open or save file dialog | Shows the dialog, then queues a message with the result | Panics, unless the test gave an answer first |
| Window command (title, size, close, cursor) | Applies it | Records it, for the test to read |
| Timer | Adds a deadline | Adds a deadline the test can advance |
| Quit | Ends the loop | Marks the app as exited |

### Rules

1. **The app keeps the Elm architecture:** one message type, one `update`, no second path that changes app state. *Why:* undo, autosave, the control channel, and the tests all rely on every change passing through one place. Splitting state changes across callbacks would make each of them miss some changes. ([research](../research/2026-11-ui-change-propagation.md#the-elm-architecture))
2. **An effect is a plain value, not a future.** *Why:* the harness can see each effect and run it or refuse it, so no test passes by skipping one.
3. **A harness that cannot run an effect panics, naming it.** *Why:* a silent skip would let a test pass that the real app fails.
4. **One message gives one `update`.** Effects run after it returns. An effect never calls `update` again in the same turn, and a message produced by an effect waits for the next turn. *Why:* chains of updates in one turn are hard to predict and hard to test.
5. **Derived values are computed in view model builders,** not through chains of messages or effects. *Why:* the same reason. A value that follows from state is a function of state.
6. **Threads wake the loop through the event loop proxy, and timers are deadlines.** The control channel listener, the MIDI learn watcher, the input watcher, and the audio watcher send messages through the proxy. The frame meter, alerts, and autosave set deadlines. *Why:* with no polling, an idle window uses no CPU (NFR-PERF-04).

## 3. Change and cost model

**Status:** Draft. Needed for phase 3. Rule 6 is needed for phase 1.

This section answers "what changed, and who depends on it?" The background is in the [research snapshot](../research/2026-11-ui-change-propagation.md).

### Rules

1. **The panel is the unit of rebuild.** One panel has one view model. When a message changes nothing a panel shows, that panel's elements, layout, and accessibility nodes are not rebuilt. *Why:* the per-panel view models of ADR-0021 already match what each part of the window reads. A finer unit adds bookkeeping; a coarser one makes every change cost the whole window.
2. **The runtime decides whether a model changed, by comparing it with the last one.** View models implement `PartialEq`. A large model may hold its data in `Arc` and compare with `Arc::ptr_eq`. No call site writes its own "did this change?" guard. *Why:* a guard that each call site must remember will be forgotten somewhere, and the cost of forgetting is a full rebuild that no test notices.
3. **A panel's model is built from the narrowest input that gives what it shows.** The stats bar does not read the graph; the inspector reads the selected module, not the whole patch. *Why:* a model built from a large shared struct changes whenever anything in it changes, which defeats rule 1.
4. **Widget state does not go through `update`.** Hover, pressed, focus rings, the caret, its blink, text selection, scroll offsets, and an open list redraw their element only. They reach the app only when they mean something: a click, a committed value, a focus change the app asked to know about. *Why:* in the frameworks the research looked at, hover and typing cost as much as a real data change when they went through the app.
5. **The contract has a boundary below the window from the start.** `view` builds panels by key, never one undivided tree. The first version may still rebuild more than it must. *Why:* a cost built into the API cannot be fixed later behind it. Faster code inside the runtime can remove work; it cannot remove a call the contract requires.
6. **Nothing redraws while nothing visible changes.** A blinking caret redraws only its own element, only while a text field has focus and the window is focused. *Why:* NFR-PERF-04, and the control channel's rule that the app sleeps when idle.

### Measuring it

The runtime counts, per turn: messages, panels compared, panels rebuilt, elements laid out, accessibility nodes sent, and redraws. `--measure-idle` reports these counts beside its existing rebuild counts and causes. *Why:* a cost model that is not measured drifts. The counts also make rule 1 testable.

## 4. Identity

**Status:** Draft. Needed for phase 1.

### Behaviour

Every element that has widget state, can take focus, appears in the accessibility tree, or can be found by the control channel has a **UI ID**. A UI ID is the panel's key followed by a short path of keys inside the panel.

Keys come from what the element shows, not from where it sits:

| Element | Key from |
|---|---|
| An inspector parameter row | The module ID and the parameter name |
| A modulation assignment row | The `AssignmentId` |
| A module browser entry | The module type ID |
| A tile control on the canvas | The module ID and the widget's name in the panel |
| A toolbar button | A fixed name (`undo`, `open`, `panic`) |
| A row in a list that never changes length | Its position, allowed only here |

### Rules

1. **One identity scheme serves five uses:** widget state, keyboard focus, accessibility node IDs, event routing, and the control channel's `locate`. *Why:* if each use had its own scheme, they could disagree about which element is which. A screen reader would follow one element while the keyboard moved another.
2. **Elements in a list that can change are keyed by a stable domain ID, never by position.** *Why:* by position, removing one row moves every later row's state, focus, and accessibility node onto its neighbour. ADR-0021 recorded this limit in Iced's `keyed_column`.
3. **An accessibility node ID stays the same while its element exists,** and is not reused for another element in the same run. The runtime keeps a table from UI ID to the 64-bit AccessKit node ID. *Why:* a screen reader keeps its place by node ID. The spike packed a module ID and a widget index into 64 bits, which works for tiles but not for rows and overlays.
4. **A UI ID can be written as text,** and that text is what the control channel's `locate` accepts beside its existing names. *Why:* agents, tests, and people then use the same names.

## 5. Layers, hit-testing, and who has the input

**Status:** Draft. Needed for phase 2.

### Behaviour

The window has four layers, drawn bottom to top: **canvas**, **panels** (toolbar, inspector, stats bar, module browser), **overlays** (probe menu, recent files, licence chooser, help), and **question**. A layer is drawn by whichever code owns it; during the move, by Iced or by `loom-ui`.

### Rules

1. **An event goes to the topmost layer first,** and down to lower layers only if no element in that layer takes it. *Why:* the user sees the top layer on top, so it must take the click. This holds while two toolkits draw.
2. **The app decides who has the input,** as today (`App::input_owner`, `Overlays::owner`). The layer asks the app; it does not keep its own idea of a modal panel. *Why:* one owner of this rule, so the canvas, the keys, and the accessibility tree all follow the same answer.
3. **While a panel owns the input, the layers below it are hidden from the accessibility tree** and take no screen reader actions. *Why:* the spike left canvas controls in the tree under an open browser, where a screen reader could reach controls the pointer could not.
4. **Hit-testing and layout need no GPU.** *Why:* the harness tests events on machines with no GPU.

## 6. Focus and the keyboard

**Status:** Draft. Needed for phase 1 (canvas and inspector) and phase 3 (every view).

### Rules

1. **One element has keyboard focus at a time, across the whole window,** or none does. The canvas is one focus stop that manages focus among its tiles and controls itself (`FocusState` in `loom-canvas`). *Why:* two focus owners would both act on the same key.
2. **Tab and Shift+Tab move through the focus order:** by layer from the top, then panel by panel in reading order, then elements in the order the panel builds them. A panel may set its own order. *Why:* NFR-UX-01, and an order a user can predict from what they see.
3. **Focus that came from the keyboard shows a visible ring.** Focus from a click does not, because the pointer is already there. *Why:* the canvas works this way today (`via_tab`).
4. **When an overlay opens, focus moves into it and stays there until it closes, then returns to where it was.** *Why:* a keyboard user must not be left behind the overlay, nor lose their place when it closes.
5. **The accessibility tree reports the element with focus,** and changes it when focus moves. *Why:* otherwise a screen reader does not follow the keyboard. In the spike, focus was always on the window.

## 7. The accessibility tree

**Status:** Draft. Needed for phase 1.

### Behaviour

In phases 1 and 2, the tree is built from the view models and the canvas scene, as in the spike. From phase 3, each `loom-ui` element gives its own node, from the same element that draws it. The canvas gives its nodes as a custom element (section 9) in every phase.

Each node has: a UI ID (section 4), a role, a name, a value and its value text where it has one, a range and step for sliders, states (disabled, checked, selected, expanded), actions, and bounds in window pixels.

| Element | Role | Value | Actions |
|---|---|---|---|
| Knob, fader, number | Slider | Number in the parameter's range, and text with its unit ("800 Hz") | Increment, Decrement, SetValue, Focus |
| Stepped parameter (octave, mode) | Slider stepping one option, or radio group | The option's name | Increment, Decrement, Focus |
| Toggle | Switch | On or off | Click, Focus |
| Button | Button | None | Click, Focus |
| Radio or segmented switch | Radio group, with one radio button per option | The chosen option | Click on an option, Focus |
| Select (pick list) | Combo box | The chosen option | Expand, Collapse, Focus |
| Text field | Text input | Its text, caret, and selection | SetValue, Focus |
| Meter | Meter | Number and text | None |
| Light | Image or status, with a text value | Its state in words | None |
| Tile | Group, named by its module | None | Focus |
| Panel | Group or region, named | None | None |

### Rules

1. **The tree and the screen come from one source.** Nothing writes accessibility names, values, or positions by hand beside what the view draws. *Why:* a tree that disagrees with the screen is worse for a screen reader user than no tree (ADR-0028).
2. **Every interactive element has a role that can be operated,** a non-empty name, and its value. A test checks every element kind. *Why:* in the spike, radios and selects fell through to read-only labels, shown on screen but not operable. The snapshot tests did not notice.
3. **Names and values use our own words, the same ones the screen shows,** with units ("800 Hz", not "800"). *Why:* NFR-UX-06.
4. **Every screen reader action becomes an ordinary message,** through the same path as the pointer or the keyboard. *Why:* undo, autosave, MIDI feedback, and the unsaved mark then work with no extra code, as the spike showed.
5. **Screen reader edits group into undo steps the way arrow keys do:** steps within 500 ms of each other form one step (the [undo spec](undo.md)). *Why:* one VoiceOver session in the spike made about 300 undo steps.
6. **The tree updates after a message that changes a view model,** and sends only the nodes that changed. When the screen reader leaves (`deactivate_accessibility`), the runtime stops building the tree. *Why:* the spike rebuilt the whole tree every 500 ms, which is polling (NFR-PERF-04).
7. **The adapter attaches before the window first shows.** The shell creates the window hidden, attaches `accesskit_winit`, then shows it. *Why:* AccessKit documents this order. The spike worked without it by luck.
8. **Focus on a node does not change the patch,** for example by selecting a tile, unless the open point below decides otherwise. *Why:* an assistive tool that only inspects the window should not change it. See open point 4.

## 8. Text input and IME

**Status:** Draft. Needed for phase 3.

### Rules

1. **Shaping, font fallback, line breaking, glyph drawing, and the editing model come from existing crates.** We write the glue, not a text engine. *Why:* ADR-0040, decision part 3.
2. **A single-line field supports:** caret, selection, word movement, Home and End, clipboard through effects, and undo inside the field. *Why:* users expect these in any text field, and their absence is noticed at once.
3. **Undo inside a field is separate from patch undo,** and ends when the field loses focus or commits. *Why:* Cmd+Z while typing a name should undo the typing, not the last patch edit.
4. **IME composition goes to the focused field.** The shell sends preedit and commit events, shows the preedit text in the field, and reports the caret area so the candidate window appears beside it. *Why:* people who type Chinese, Japanese, or Korean need composition to type at all.
5. **A text field reports its text, caret, and selection to the accessibility tree.** *Why:* a screen reader reads and moves through text by these properties.
6. **The multi-line editor (patch info) comes last,** and may stay on Iced's editor until it is done. *Why:* it is the largest piece, and the rest of the move should not wait for it.

## 9. Custom elements

**Status:** Draft. Needed for phase 1.

A custom element is the layer's supported escape hatch. The patch canvas is the first.

### Rules

1. **A custom element owns its own state, drawing, and input.** It gets pointer and key events inside its bounds, draws into a `wgpu` render pass with a viewport the runtime gives it, and keeps whatever retained data it needs (the canvas keeps its `Scene` and GPU buffers).
2. **It takes part in focus as one stop,** and manages focus inside itself (section 6, rule 1).
3. **It gives accessibility nodes under its own UI ID,** one per inner part a user can reach (a tile, a control, a port), in its own focus order.
4. **It reports when it needs a redraw,** and the runtime draws it only then. Pan and zoom only update its uniforms (ADR-0010).
5. **It reports events to the app as messages,** and never edits app state itself. The canvas already works this way (`CanvasEvent`).

*Why:* a pure pipeline never covers everything. An editor or a canvas needs its own drawing and input. A hatch that is part of the contract keeps such elements inside focus, the accessibility tree, and the tests, rather than beside them.

## 10. Test harness and control channel

**Status:** Draft. Needed for phase 2.

### Rules

1. **One runtime, two drivers.** The window and the headless `Ui` harness run the same code for events, layout, messages, and effects. Only the event source and the drawing target differ. *Why:* a test then checks what the app does, not what a copy of it does.
2. **The `Ui` harness keeps its public API.** `headless`, `press`, `release`, `drag`, `move_to`, `tap`, `tap_key`, `key_down`, `key_up`, `typewrite`, `click_text`, `control`, `field`, `text`, `focus`, `output`, `input`, `run_task`, and the rest keep their meaning, and the files that use them do not change. *Why:* about 900 tests depend on it, and a change there would hide whether behaviour changed.
3. **Event tests need no GPU.** Drawing tests render to an offscreen `wgpu` texture, and skip with `NO_GPU` where there is no adapter. *Why:* CI machines often have no GPU.
4. **The harness can read and act on the accessibility tree:** `Ui::a11y_tree()` returns it, and `Ui::a11y_action(name, action)` performs an action on a node found by name. *Why:* a test can check the window the way a screen reader meets it.
5. **The control channel's protocol does not change.** `locate` finds targets from layout data and UI IDs, not from a toolkit's widget tree. Its replies are the same for every example in the [control channel spec](control-channel.md). *Why:* agents and scripts built on it keep working.

## Acceptance criteria

Each phase also passes the parity gates in [the plan](../dev/leaving-iced-plan.md#parity-gates).

Phase 1:

- [ ] `loom-ui-core` exists, depends on no toolkit and no LoomDSP crate, and has an example program.
- [ ] `loom-canvas` uses no Iced types outside its widget adapter and primitive wrapper.
- [ ] A test checks that every interactive element kind on a tile and in the inspector has an operable role, a non-empty name, and a value with its unit.
- [ ] A test checks that a screen reader SetValue, repeated within 500 ms, makes one undo step.
- [ ] A test checks that the tree changes when a view model changes, and only then.
- [ ] Accessibility node IDs for inspector rows survive adding and removing a module.
- [ ] A VoiceOver check is recorded in the pull request: which controls were read, and which could be changed by keyboard.

Phase 2:

- [ ] A test runs an effect the harness cannot run and checks that it panics with the effect's name.
- [ ] A test checks that one message calls `update` once, even when its effects produce more messages.
- [ ] With the app idle for 10 seconds, `--measure-idle` reports no redraws and no panel rebuilds.
- [ ] The tree is exposed on macOS, Windows, and Linux. Narrator and Orca checks are recorded, or listed as not checked.
- [ ] `Ui::a11y_tree()` and `Ui::a11y_action()` exist and a test uses each.

Phase 3:

- [ ] A message that changes nothing rebuilds no panel. A test checks the rebuild counter.
- [ ] Moving the pointer over the toolbar, or typing in the browser search, rebuilds no other panel.
- [ ] A benchmark times one value change with the module browser showing 1,000 entries and a full modulation matrix, and records memory and the first build time.
- [ ] Removing a row from a keyed list moves no widget state, focus, or accessibility node onto its neighbour.
- [ ] Tab and Shift+Tab reach every interactive element in every view, with a visible ring, and the screen reader follows.
- [ ] Opening an overlay moves focus into it and hides the layers below from the tree; closing it restores focus.
- [ ] IME composition works in the browser search on macOS with a Japanese or Chinese input method. Recorded in the pull request.

## Out of scope

- **Plugin window embedding** (OQ-13). The shell must not block it: it takes a window it is given as well as one it creates. Building it is later work.
- **The web target** (ADR-0026).
- **Visual redesign.** Every view looks the same after each phase.
- **Read-tracking state, such as signals.** Comparing panel models is enough at LoomDSP's size ([research](../research/2026-11-ui-change-propagation.md#not-proposed)). Revisit if a measured panel cannot meet its cost with rule 1 of section 3.
- **Components with their own message types** inside `loom-ui`. LoomDSP's panels use the app's message. A mapping like Elm's `Html.map` can be added when a second app needs it.
- **Publishing the crates.** Waits until the API settles (FR-SDK-06).

## Open points

Each is answered by the maintainer before the phase that needs it.

1. **Which text crate:** `cosmic-text` (what Iced uses today, so one font system during the move) or Parley (whose release notes list AccessKit text properties and whose editor tracks IME composition †). *Leaning:* decide at plan step 2.4, the first text drawn without Iced, after a short comparison of IME and accessible text fields. Rules 4 and 5 of section 8 weigh most. Needed for phase 2.
2. **Which layout engine:** `taffy` † or a small pass of our own for row, column, stack, padding, alignment, and fixed and fill sizes. *Leaning:* `taffy`, if its licence and version fit, because other tools using `loom-ui` will want flexbox and grid, and the inspector matrix is already a grid. Needed for phase 3.
3. **How a held arrow key steps a knob for a screen reader.** VoiceOver doubles its step on each repeat, so a held key leaps across the range ([spike](../research/2026-11-a11y-beside-iced-spike.md#voiceover-session-same-day-by-the-maintainer)). Options: take its value as sent, or read each SetValue as a direction and move a fixed step. *Leaning:* take the value as sent, because some screen reader users rely on the speed-up, and decide after a listening test. Needed for phase 1.
4. **Whether focus on a tile selects it.** Convenient for a screen reader user, but an assistive tool that only inspects may change the selection. *Leaning:* no; selection follows only an explicit action (Click, or Enter). Needed for phase 1.
5. **Number or text for a slider's value on macOS,** until AccessKit's macOS adapter has a spoken value description. With a number, VoiceOver steps but cannot say the unit; with text, it says the unit but cannot step. *Leaning:* a number, with the unit in the node's description, until a pinned fork with the fix is in place. Needed for phase 1.
6. **The exact shape of the panel API** in section 2. *Leaning:* the sketch above, refined in phase 1 when the inspector, the first panel with a model, moves to it.

Entries marked † are from web search summaries and the projects' prose, not from their source. Check them before relying on them.
