# UI Layer Specification

How LoomDSP's own UI layer turns events into messages, decides what to rebuild, lays out and draws panels, moves keyboard focus, and exposes an accessibility tree, for the desktop app and for other tools that use the layer.

- **Kind:** living specification.
- **Status:** Agreed (2026-11-09). Each section gives the phase of [the plan](../dev/leaving-iced-plan.md) that first builds it. One choice, the text crate, is agreed as a decision rule and made at plan step 2.4 (section 8, rule 8).
- **Decision behind it:** [ADR-0041](../adr/0041-own-ui-layer.md), superseding [ADR-0009](../adr/0009-ui-stack-iced.md) for the window and widgets, and [ADR-0028](../adr/0028-app-accessibility-waits-for-iced.md). Builds on [ADR-0010](../adr/0010-own-patch-canvas.md) (our own canvas), [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md) (plain-data view models), and [ADR-0024](../adr/0024-widgets-follow-the-kind-of-value.md) (widgets follow the kind of value).
- **Use case and requirements:** NFR-UX-01 (keyboard), NFR-UX-06 (screen readers), NFR-PERF-04 (no redraw when idle), FR-SDK-06 (publishable crates), OQ-13 (plugin windows), OQ-14 (Iced churn).
- **Background:** [how a UI layer decides what changed](../research/2026-11-ui-change-propagation.md), [the accessibility spike](../research/2026-11-a11y-beside-iced-spike.md), [our own UI stack instead of Iced](../research/2026-11-own-ui-stack-tradeoffs.md). An explainer in `docs/learn/` follows in phase 4. The [canvas accessibility spec](canvas-accessibility.md) builds on sections 6, 7, and 9 for the patch outline, the patch map, and following cables.
- **Prior art:** the Elm architecture (one message type, one update), and the rebuild-and-compare approach that Levien describes for Xilem. Named only to explain ideas.
- **Issues:** [#683](https://github.com/brylie/LoomDSP/issues/683). The step-by-step work is in [the plan](../dev/leaving-iced-plan.md).
- **Code:** `crates/loom-ui-core`, and `crates/loom-ui` for the runtime of section 2 (plan step 2.1). The rest is still in `crates/loom-app` and `crates/loom-canvas`.

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
| **Effect** | A plain value the app returns from `update` asking the shell to do something outside the app: focus a field, use the clipboard, open a dialog, run work off the UI thread, quit. |
| **View model** | A plain-data struct the app builds from its state, describing what one panel shows (ADR-0021). |
| **Panel** | A part of the window with its own view model and its own place in a layer: the toolbar, the stats bar, the inspector, the module browser, an overlay. The unit of rebuild. |
| **Element** | One node of the tree a panel builds from its view model: a row, a button, a slider, a text. Elements are rebuilt when their panel is rebuilt. |
| **Widget state** | State that belongs to one element and lasts across rebuilds: hover, pressed, scroll offset, caret and selection in a text field, an open list. Not app state. |
| **Live value** | A value that changes at display rate while audio plays: a meter reading, a light, a live modulated position, a scope trace, the audio load. |
| **Custom element** | An element that owns its own drawing, input, and accessibility nodes, such as the patch canvas. |
| **Layer** | A stacking level of the window: canvas, panels, overlays, question. |
| **UI ID** | The stable identity of an element across rebuilds. |
| **Logical pixel** | A window pixel before the display's scale factor. On a Retina display at scale 2, one logical pixel is two physical pixels wide. |

## 1. Crates and what they may know

**Status:** Agreed, phase 1.

| Crate | Holds | May depend on |
|---|---|---|
| `loom-ui-core` | Geometry (`Point`, `Size`, `Rect`, `Vector`, `Color`), input events (`Event`, `PointerEvent`, `PointerButton`, `KeyEvent`, `Modifiers`, `Key`, `ScrollDelta`, and `Platform` for each platform's shortcut key), UI IDs | No UI toolkit, no LoomDSP crate |
| `loom-ui` | Runtime, effects, elements, layout, focus, the accessibility tree, the `winit` shell, the widgets, and the drawing of panels | `loom-ui-core`, `winit`, `wgpu`, the text crate, `taffy`, `accesskit`, `accesskit_winit` |
| `loom-canvas` | The patch canvas, as a custom element | `loom-ui-core`, later `loom-ui`, and `iced` and `iced_wgpu` until phase 2 moves it off them |
| `loom-app` | The app: messages, `update`, view models, panels | All of the above |
| `loom-app/src/iced_bridge/` (temporary) | During phases 2 and 3 only: drives Iced's `UserInterface` for the views not yet moved, turns the effects that address an Iced widget (focus, scroll, select all, the focus query) into Iced widget operations, and draws those views with `iced_wgpu`. Removed in phase 4 | `loom-ui`, `iced`, `iced_wgpu`, `iced_runtime` |

Rules:

1. **`loom-ui-core` and `loom-ui` know nothing about patches.** They never depend on `loom-core`, `loom-patch`, `loom-canvas`, or `loom-app`, and they use no patch words in their API. *Why:* other tools should be able to use them, and publishing them later (FR-SDK-06) should not need a second refactor.
2. **The runtime and the shell take the app as a trait or a generic parameter,** never as `loom_app::App`. *Why:* the same reason.
3. **Each crate has a crate-level doc comment and an example program that does not use LoomDSP.** *Why:* the example proves rule 1, and it is the first documentation a new user reads.
4. **The engine crates never depend on UI crates.** `loom-core`, `loom-dsp`, `loom-modules`, `loom-patch`, and `loom-rt` stay portable (AGENTS.md, hard rule 6).
5. **Every new crate passes `cargo deny check` before it is used** (ADR-0006).
6. **All Iced code during the move lives in the bridge.** `loom-ui` never depends on Iced. The runtime lives in `loom-ui` from phase 2, and the bridge plugs Iced's views into it as one more drawing and routing target per layer. An element still drawn by Iced gets an `iced::widget::Id` made from the text form of its UI ID (section 4, rule 5), so focus and scroll effects reach the same element in both toolkits. *Why:* one place to delete in phase 4, and no Iced type can leak into the crates meant for reuse.

## 2. The runtime: events, messages, and effects

**Status:** Agreed, phase 2.

### Behaviour

One turn of the runtime:

1. An **event** arrives from the driver: a pointer or key event, IME, a file dropped on the window, a resize, a scale factor change (the window moved to another display), window focus gained or lost, a close request, a redraw request, an animation frame (section 3, rule 8), word that the last frame was shown, a wake-up from another thread (a watcher, background work, or a screen reader's action), or a deadline that has passed. The driver passes the moment with it; the runtime never reads the clock.
2. The runtime **routes** it (sections 5 and 6). The element that takes it either handles it as widget state, or turns it into a **message**. Window events with no element (a file drop, a close request) become messages directly.
3. For each message, the runtime calls **`update`** once. `update` changes app state and returns **effects**.
4. The runtime **runs the effects** after `update` returns, before the next message's `update`: each message's effects run right after its own update. An effect that produces a message (a dialog closed, a clipboard read, background work finished, a frame shown) queues it for a later turn.
5. The runtime asks each panel whether its view model changed, and **rebuilds** only the panels that did (section 3).
6. It redoes **layout** for rebuilt panels, updates the **accessibility tree** for changed nodes, and asks the driver for a **redraw** only if something visible changed.
7. With nothing queued, no animation frame asked for, and nothing visible to change, the loop **sleeps** until the next event or deadline.

The contract the app implements, as built in plan step 2.1 (`crates/loom-ui/src/app.rs`). It is designed from what LoomDSP and the tools beside it need, not from Iced's `Task`, `Subscription`, or widget operations. Phase 3 adds the builder that makes a panel's elements, and `View` then gains the message type.

```rust
pub trait App {
    /// The only way app state changes. `Send`, since watchers send it.
    type Message: Send + 'static;

    /// Applies one message and returns its effects.
    fn update(&mut self, message: Self::Message, cx: &Context<'_, Self::Message>) -> Effects<Self::Message>;

    /// Describes the window, one panel at a time: a key, a layer, a model.
    fn view(&self, view: &mut View);

    /// The title and limits, compared after each update like a panel model.
    fn window(&self) -> WindowSpec;

    /// When to wake with no event, and the message to get then.
    fn next_deadline(&self) -> Option<Deadline<Self::Message>> { None }

    /// The message for a close request or a dropped file, if any.
    fn window_event(&self, event: &WindowEvent) -> Option<Self::Message> { None }

    /// True while something moves: a frame each display refresh, no update.
    fn animating(&self) -> bool { false }
}

// Inside `view`. Phase 3 adds a builder after the model.
view.panel(UiId::panel("inspector"), Layer::Panels, &inspector_model);
```

`Context` gives `update` the moment (`now`), a `Waker` for watcher threads, the element with focus, and the window's size, scale, and focus, all read only. `update` reads focus there in the same turn, instead of asking for it with an effect and waiting for the answer.

### Effects

The table covers every use of Iced's `Task`, its widget operations, and `rfd` in `crates/loom-app/src` at commit `a781de4`, and what replaces each (`Task::none` and `Task::batch` become an empty and a combined `Effects`). A new kind of effect is added here first. Each is a variant of `loom_ui::Effect`, and each has a stable name that a driver's panic text uses.

| Effect | Used today by | What the shell does | What the headless harness does |
|---|---|---|---|
| Focus an element, by UI ID, or the next or previous one, optionally selecting all of a text field. The UI ID may name an inner part of a custom element (section 9, rule 6) | Focusing a field, Tab between fields (`editing.rs`, `StepFocus`); opening a field for typing selects all; later, Enter on an outline row focusing a tile control | Moves keyboard focus | The same |
| Scroll an element into view | The module browser and patches lists, a matrix row (`matrix.rs`) | Changes its scroll offset | The same |
| Write the clipboard | Copy, Cut | Writes the system clipboard | Writes its in-memory clipboard |
| Read the clipboard | Paste (`Message::Pasted`) | Reads the system clipboard, then queues a message with the text for the next turn | Reads its in-memory clipboard, and queues the same message |
| Open or save file dialog | Open, Save As | Shows the dialog, then queues a message with the result | Panics, unless the test gave an answer first |
| Ask a question in a native dialog | Unsaved changes, crash recovery, the author name (`rfd::MessageDialog`) | Shows the dialog, then queues a message with the index of the button pressed | Panics, unless the test gave an answer first |
| Run work off the UI thread. It gets a waker and sends one message or many | Loading the module library (`browser.rs`); reading the patches folder in batches (`patches.rs`) | Runs it on a worker thread; each message wakes the loop | Runs it inline, to its end, and queues every message it sends, in order |
| Capture the window as an image | The control channel's `screenshot` | Draws the next frame offscreen, then queues a message with the image | `loom-ui`'s headless driver panics unless the test gave an answer; the app's `Ui` harness draws offscreen when a GPU adapter exists, else answers `NO_GPU` |
| A message once the next frame is shown | The control channel's two-frame wait (`ControlFrame`) | Draws, and queues the message when the frame has been shown | The same, when the test reports the frame shown |
| Draw again although no panel model changed | A request handed to a custom element that it applies when it draws, such as a screen reader's Focus action on the canvas | Draws | Counts the draw |
| Quit | Quitting (`iced::exit` in `lib.rs`) | Ends the loop | Marks the app as exited |

Not effects, because nothing needs them as effects:

- **Which field has focus.** `update` reads it from its `Context`. Iced needed a round trip (`Message::TextFocus`) because it keeps focus inside its widgets.
- **The window's title and limits.** The app describes them in `App::window`, and the runtime passes them on only when they change, as with panel models.
- **Timers.** Nothing uses a one-shot timer; every periodic check is a deadline from state (rule 8). A timer effect can be added when something needs one.
- **A message queued for the next turn** (`Task::done`). It makes a chain of updates, which rule 6 rules out. Inline results come from work, and "after this turn is drawn" is the after-draw effect.

**Dialogs take two turns.** Today `files.rs` and `recovery.rs` call `rfd`'s dialogs inside `update` and block until they close. As effects, the question is one turn and its answer is the next, so the app holds what waits on the answer (the unsaved changes on close, the crash recovery at start). The loop keeps running while a dialog is open, so meters, telemetry, and the accessibility tree do not stall.

### Rules

1. **The app keeps the Elm architecture:** one message type, one `update`, no second path that changes app state. *Why:* undo, autosave, the control channel, and the tests all rely on every change passing through one place. Splitting state changes across callbacks would make each of them miss some changes. ([research](../research/2026-11-ui-change-propagation.md#the-elm-architecture))
2. **An effect is a plain value, not a future.** Background work is a closure the shell runs on a worker thread, not an async task. *Why:* the harness can see each effect and run it or refuse it, so no test passes by skipping one.
3. **A harness that cannot run an effect panics, naming it.** *Why:* a silent skip would let a test pass that the real app fails.
4. **The harness runs background work inline and in order.** *Why:* tests stay deterministic, and a test never waits on a thread. The patches browser already reads its folder this way under test.
5. **One message gives one `update`.** Effects run after it returns. An effect never calls `update` again in the same turn, and a message produced by an effect waits for the next turn. *Why:* chains of updates in one turn are hard to predict and hard to test.
6. **Derived values are computed in view model builders,** not through chains of messages or effects. *Why:* the same reason. A value that follows from state is a function of state.
7. **Threads wake the loop through a waker.** The control channel listener, the MIDI learn watcher (`learn.rs`), the audio input watcher (`input_watch`), the audio stream watcher (`audio_watch`), the telemetry monitor, and background work send messages through it. The waker queues the message and calls the driver's wake function: in the `winit` shell that nudges the `EventLoopProxy`; in the harness it does nothing, since the harness drains the queue itself. A watcher takes its waker from `update`'s `Context`, and the app keeps the watcher's handle in its state: the watcher lives as long as the handle, and dropping the handle stops it. Dropping a handle never blocks the UI thread. It signals the thread and detaches, or joins only a thread known to end at once. *Why:* the loop then sleeps until something happens, rather than polling for it. Holding watchers as state, not as a list of subscriptions restarted by key, makes their lifetime the lifetime of what they watch (an engine, a stream, a listener). A watcher stuck in a MIDI or audio call must not freeze the window on New or Open.
8. **Periodic checks become deadlines, and a deadline that changes nothing costs nothing.** These stay periodic: the device poll (`AudioPoll`, every 250 ms), the MIDI port poll (`MidiPoll`), the MIDI ghost check (`MidiGhosts`), the 500 ms `Tick`, the autosave check (`Autosave`), and the drop-out check (`DropsDone`). Each wakes the loop and runs `update`. When its message changes no view model, it causes no panel rebuild, no accessibility update, and no redraw. *Why:* these checks poll the operating system, which has no event for them. NFR-PERF-04 asks for no redraw while nothing visible changes, which holds even while the loop wakes.
9. **The driver is a trait, and the runtime and the `App` trait use no `winit` type.** No `EventLoop`, `EventLoopProxy`, `ControlFlow`, `Window`, or `winit` event reaches them. The runtime takes events as `loom-ui-core` types, returns the next deadline, and asks the driver to redraw or to apply window commands. Two drivers exist after phase 2: the `winit` shell and the headless harness. *Why:* a plugin's window belongs to the host, and the host runs the event loop. `winit` owns the process's one event loop (on the main thread on macOS), so it is not expected to serve inside a plugin; plugin GUIs use host-window backends instead. The OQ-13 spike confirms which. A third driver for a host window must then be possible without changing the runtime. Otherwise the contract carries the kind of cost section 3, rule 5 warns about.
10. **A turn draws only when something visible changed.** The turn's result tells the driver to draw (`Next::draw`; the runtime never calls the driver to draw, so a shell cannot turn its own redraw request into a loop) when a panel's model differs from the last (section 3, rule 2), when the routing target says it needs a draw, when an effect asks (draw again, a message after the next frame, a capture), or when the system asks. An animation frame draws and runs no update. Nothing else draws. A frame the driver asked its window system for is drawn when it comes, with no redraw request sent for it, and several turns that say to draw before then make one frame. A resize or a scale change draws through the redraw request the window system sends after it; a driver whose system sends none sends one itself. After each frame is on screen the driver says so once, and only for a frame that began after the latest turn that said to draw, since a message waiting for the next frame arrives then. During phases 2 and 3 the routing target for the views Iced still draws is the bridge, and it says it needs a draw after any turn that ran an update. That is Iced's own habit, kept inside the bridge and deleted with it. Measured on `main` at `085f217a`, `--measure-idle` on `subtractive.ron` drew 266 frames in 10 s, 263 of them for meter messages and none for polls, so this keeps today's cost exactly. *Why:* the runtime must not hard-code one toolkit's habit, and no `update` path may write its own "did this change?" guard (section 3, rule 2). The saving comes in phase 3, when each view is a compared panel and meters are live values.
11. **The runtime never reads the clock.** Every turn takes the moment from the driver, and `update` reads it from its `Context`. *Why:* a test then decides what time it is, and a deadline test runs in no time at all.

## 3. Change and cost model

**Status:** Agreed, phase 3. Rule 7 also applies to the accessibility tree built in phase 1 (section 7, rule 7).

This section answers "what changed, and who depends on it?" The background is in the [research snapshot](../research/2026-11-ui-change-propagation.md).

### Rules

1. **The panel is the unit of rebuild.** One panel has one view model. When a message changes nothing a panel shows, that panel's elements, layout, and accessibility nodes are not rebuilt. *Why:* the per-panel view models of ADR-0021 already match what each part of the window reads. A finer unit adds bookkeeping; a coarser one makes every change cost the whole window.
2. **The runtime decides whether a model changed, by comparing it with the last one.** View models implement `PartialEq`. A large model may hold its data in `Arc` and compare with `Arc::ptr_eq`. No call site writes its own "did this change?" guard. *Why:* a guard that each call site must remember will be forgotten somewhere, and the cost of forgetting is a full rebuild that no test notices.
3. **A panel's model is built from the narrowest input that gives what it shows.** The toolbar does not read the graph; the inspector reads the selected module, not the whole patch. *Why:* a model built from a large shared struct changes whenever anything in it changes, which defeats rule 1.
4. **Widget state does not go through `update`.** Hover, pressed, focus rings, the caret, its blink, text selection, scroll offsets, and an open list redraw their element only. They reach the app only when they mean something: a click, a committed value, a focus change the app asked to know about. *Why:* in the frameworks the research looked at, hover and typing cost as much as a real data change when they went through the app.
5. **The contract has a boundary below the window from the start.** `view` builds panels by key, never one undivided tree. The first version may still rebuild more than it must. *Why:* a cost built into the API cannot be fixed later behind it. Faster code inside the runtime can remove work; it cannot remove a call the contract requires.
6. **Nothing redraws while nothing visible changes.** A blinking caret redraws only its own element, only while a text field has focus and the window is focused. *Why:* NFR-PERF-04, and the control channel's rule that the app sleeps when idle.
7. **Live values are not part of panel models.** Meter readings, lights, live modulated positions, scope traces, and the audio load live in a live value table that the app updates from the engine. An element that shows one reads the table when it draws, and is redrawn, not rebuilt, when it changes. The canvas already works this way: panel control values have their own GPU buffer (`PanelValues`), so a moving meter never rebuilds tiles. *Why:* these values change at display rate while audio plays. In a panel model, they would rebuild that panel 60 times a second, and an "equal message rebuilds nothing" check would pass while the real cost went unmeasured.
8. **Animation frames are asked for, not constant.** An element that shows moving live values asks the runtime for animation frames while they move, and stops asking when the engine is idle or the element is off screen. The runtime delivers one frame event per display refresh, only while some element asks. This replaces Iced's `window::frames()` subscriptions `Frame` and `LiveFrame`. `ControlFrame` is replaced by section 2's message once the next frame is shown. *Why:* frame-rate drawing costs nothing when nothing moves (NFR-PERF-04, "a display redraws only while it is on screen").

### Measuring it

The runtime counts, per turn: messages, panels compared, panels rebuilt, elements laid out, accessibility nodes sent, animation frames, and redraws. `--measure-idle` reports these counts beside its existing rebuild counts and causes. *Why:* a cost model that is not measured drifts. The counts also make rules 1 and 7 testable.

## 4. Identity

**Status:** Agreed, phase 1.

### Behaviour

Every element that has widget state, can take focus, appears in the accessibility tree, or can be found by the control channel has a **UI ID**. A UI ID is the panel's key followed by a short path of keys inside the panel.

Keys come from what the element shows, not from where it sits:

| Element | Key from |
|---|---|
| An inspector parameter row | The module ID, the kind `param`, and the parameter name |
| A modulation assignment row | The `AssignmentId` |
| A module browser entry | The module type ID |
| A tile control on the canvas | The module ID and the widget's name in the panel |
| A sub-patch tile, a Patch input or output tile | Its flat module ID: instance and port tiles take IDs from the same table as modules ([sub-patches](sub-patches.md)) |
| An exposed item on a sub-patch's surface, on its tile or its inspector rows | The instance's flat ID, the kind `exposed`, and the item's name on that surface |
| A slot and its parts | The slot's flat ID, then the part's module type or library item |
| A control on a slot's tile, or its inspector row | The slot's flat ID, then its part as above, then the kind and the name: `canvas/7/loom.filter.svf/param/cutoff`. A part that is a sub-patch is `subpatch` and then its key. When the slot swaps its part, the new part's controls are new elements, even where a name repeats |
| One value of an XY pad or a range, one option of a radio or segmented control | The control's ID, then a name for the piece: `x` or `y`, `low` or `high`, or `option` and the option's index, as `canvas/4/param/mode/option/2`. The index counts the parameter's choices from 0: it is the parameter's value less its minimum, so it is what the option shows, not where it is drawn. The piece is a name so that it cannot be taken for the number that counts a second widget on the same value |
| A breadcrumb entry | The path of instance flat IDs from the top patch to that level |
| A library item in the module browser | `library` and the item's path in the library folder |
| A toolbar button | A fixed name (`undo`, `open`, `panic`) |
| A row in a list that never changes length | Its position, allowed only here |

### Rules

1. **One identity scheme serves five uses:** widget state, keyboard focus, accessibility node IDs, event routing, and the control channel's `locate`. *Why:* if each use had its own scheme, they could disagree about which element is which. A screen reader would follow one element while the keyboard moved another.
2. **A key names its kind where a module shows more than one kind of item.** The kind is one of `param`, `exposed`, `display`, `input`, `output`, and `badge`, as the control channel's prefixes already name them. An instance's exposed "Cutoff" and the inner module's "cutoff" row then never share a key. A flat ID needs no level beside it: the flat ID table is keyed by path and never gives an ID to something else while the patch is open ([flattening](../learn/flattening.md)). *Why:* that table already lets the canvas, the selection, and undo work across levels, so identity can rest on it too.
3. **Elements in a list that can change are keyed by a stable domain ID, never by position.** *Why:* by position, removing one row moves every later row's state, focus, and accessibility node onto its neighbour. ADR-0021 recorded this limit in Iced's `keyed_column`.
4. **An accessibility node ID stays the same while its element exists,** and is not reused for another element in the same run. The runtime keeps a table from UI ID to the 64-bit AccessKit node ID. It gives out IDs from a counter that only increases, and removes an entry when its element is gone; if the same UI ID comes back later, it gets a new node ID. *Why:* a screen reader keeps its place by node ID, and a reused ID would make it read the wrong element. Removing entries keeps the table the size of the window, not of the session. The spike packed a module ID and a widget index into 64 bits, which works for tiles but not for rows and overlays.
5. **A UI ID can be written as text,** and that text is what the control channel's `locate` accepts beside its existing names. The text is the keys joined by `/`: `canvas/4/param/cutoff`, `canvas/5/badge/out`, `toolbar/undo`, `inspector/4/param/cutoff`.
   - A key made only of digits is a number. Any other key is a name.
   - Inside a name, `/` is written `%2F` and `%` is written `%25`, so a library item's path stays one key: `browser/library/pads%2Fwarm`.
   - A name made only of digits has its first digit escaped, so it does not read back as a number: the name `12` is written `%312`.
   - Writing a UI ID and reading the text back gives the same UI ID, for every UI ID. A test checks it.
   - The control channel's short names, such as `4:cutoff`, keep working beside it.

   *Why:* agents, tests, and people then use the same names. A path reads like the tree it names, and one escape rule keeps every ID readable back.

## 5. Layers, hit-testing, and who has the input

**Status:** Agreed, phase 2.

### Behaviour

The window has four layers, drawn bottom to top: **canvas**, **panels** (toolbar, inspector, stats bar, module browser), **overlays** (probe menu, recent files, licence chooser, help), and **question**. A layer is drawn by whichever code owns it; during the move, by Iced or by `loom-ui`.

### Rules

1. **A pointer event goes to the topmost layer first,** and down to lower layers only if no element in that layer takes it. *Why:* the user sees the top layer on top, so it must take the click. This holds while two toolkits draw.
2. **The app decides who has the input,** as today (`App::input_owner`, `Overlays::owner`). The layer asks the app; it does not keep its own idea of a modal panel. *Why:* one owner of this rule, so the canvas, the keys, and the accessibility tree all follow the same answer.
3. **While a panel owns the input, the layers below it are hidden from the accessibility tree** and take no screen reader actions. *Why:* the spike left canvas controls in the tree under an open browser, where a screen reader could reach controls the pointer could not.
4. **Layout is in logical pixels, and hit-testing and layout need no GPU.** *Why:* the harness tests events on machines with no GPU, and the control channel answers in logical pixels.

## 6. Focus and the keyboard

**Status:** Agreed. Rules 1, 3, and 5 for the canvas and the inspector in phase 1; the rest in phase 3, for every view. Rule 7 comes with the canvas accessibility spec's outline.

### Rules

1. **One element has keyboard focus at a time, across the whole window,** or none does. The canvas is one focus stop that manages focus among its tiles and controls itself (`FocusState` in `loom-canvas`). *Why:* two focus owners would both act on the same key.
2. **Tab and Shift+Tab move through the focus order:** by layer from the top, then panel by panel in reading order, then elements in the order the panel builds them. A panel may set its own order. *Why:* NFR-UX-01, and an order a user can predict from what they see.
3. **Focus that came from the keyboard shows a visible ring.** Focus from a click does not, because the pointer is already there. *Why:* the canvas works this way today (`via_tab`).
4. **When an overlay opens, focus moves into it and stays there until it closes, then returns to where it was.** If that element is gone (a module deleted while the overlay was open), focus goes to the first focus stop of the panel that held it, or to the canvas if the panel is gone too. *Why:* a keyboard user must not be left behind the overlay, nor lose their place when it closes.
5. **The accessibility tree reports the element with focus,** and changes it when focus moves. *Why:* otherwise a screen reader does not follow the keyboard. In the spike, focus was always on the window.
6. **A key goes to the focused element first,** then to its panel, then to the app's shortcuts. An element or a panel that takes a key stops it there. A focused text field takes every printable key, Space, Backspace, Delete, the arrows, and its editing shortcuts (select all, copy, cut, paste, undo, redo). It does not take Ctrl+Tab or Ctrl+Shift+Tab, which go on to the app's shortcuts (rule 7). The order kept from today (`input_owner.rs`, `Listening::rows`):
   1. **Panic** is always heard, whoever has the input (`always_event`).
   2. **An open question has every other key.** No key both answers it and reaches what is behind it.
   3. **The focused element.** A key it uses goes no further, with the exceptions in step 4.
   4. **Panels that navigate by key hear their keys even when a child element used them:** the matrix its arrows and Escape while its table has focus, the module browser its navigation keys and Escape over its search field, and the patches browser Up, Down, and Escape over its fields.
   5. **The open overlay** (`App::overlay_key`), then the canvas, by the canvas focus policy (`CanvasFocusPolicy`).
   6. **The app's shortcuts** (`Message::Key`): undo, redo, save, delete, and the rest.

   A change to this order is a behaviour change. It is listed in its pull request and in this rule. *Why:* today the order is spread across Iced's widget status and a list of listeners, so a rebuilt layer could change it without anyone noticing. Cmd+Z in a name field must undo the typing, and the canvas must have its arrows before panel navigation does.
7. **Ctrl+Tab moves focus to the next region, and Ctrl+Shift+Tab to the previous one:** the canvas, the right column, then the toolbar, wrapping at both ends. Ctrl is used on every platform, macOS included. Each region remembers the element it last had focused, and the key returns to it; the first time, focus goes to the region's first focus stop. In the right column, that is the open tab's content, so the key lands on the outline when the Outline tab is open ([canvas accessibility](canvas-accessibility.md)). It is an app shortcut (rule 6, step 6) that a focused text field does not take (rule 6), and it does nothing while an overlay or a question holds focus (rule 4). Every handler of Tab ignores Tab while Ctrl is held: today the canvas's range parts (`panel_input.rs`), the matrix (`matrix.rs`), and field stepping (`lib.rs`, which on macOS checks Cmd, not Ctrl) would all act on it. *Why:*
   - Tab moves one element at a time, which is slow across a toolbar, an inspector, and a canvas of tiles. A key that jumps between the window's large parts is what keyboard and screen reader users expect from desktop apps.
   - F6, the usual key for this, needs Fn on most laptop keyboards. Ctrl+Tab is reachable with one hand on any keyboard.
   - Cmd+Tab is the macOS app switcher, so Ctrl is the only modifier that works the same on every platform.
   - It must work from a text field, or a user typing in the browser search could not leave the right column without Tab-stepping out.

## 7. The accessibility tree

**Status:** Agreed, phase 1. The node of a text field (rule 10) is built in phase 3.

### Behaviour

In phases 1 and 2, the tree is built from the view models and the canvas scene, as in the spike. From phase 3, each `loom-ui` element gives its own node, from the same element that draws it. The canvas gives its nodes as a custom element (section 9) in every phase.

Each node has: a UI ID (section 4), a role, a name, a value where it has one, a description, a range and step for sliders, states (disabled, checked, selected, expanded), actions, and bounds.

The role follows the widget that is drawn, as widgets follow the kind of value (ADR-0024):

| Element drawn | Role | Value | Actions |
|---|---|---|---|
| Knob, fader, number | Slider | The number, in the parameter's range (rule 14) | Increment, Decrement, SetValue, Focus |
| Stepped parameter drawn as a knob (octave, note value) | Slider, stepping one option at a time | The option's index, with its name in the description | Increment, Decrement, SetValue, Focus |
| Stepped parameter drawn as radio or segmented buttons | Radio group, with one radio button per option | The chosen option | Click on an option, Focus |
| Toggle | Switch | On or off | Click, Focus |
| Button | Button | None | Click, Focus |
| Select (pick list) | Combo box | The chosen option | Increment, Decrement, Focus. Expand and Collapse from phase 3 (rule 15) |
| XY pad, range | Group, named, with one slider per value inside | Each slider: its number, in its parameter's range | On each slider: Increment, Decrement, SetValue, Focus |
| Readout | Label, with no label of its own | Its name and the text shown, as "Cents: 12.5 cents" (rule 17) | None |
| Text field | Text input | Its text, as text runs with caret and selection (rule 10) | SetValue, SetTextSelection, ReplaceSelectedText, Focus |
| Meter | Meter | The number its text shows, such as -6 for "-6.0 dB", with that text as the description, and the range the meter draws as minimum and maximum. No number when the text names none or more than one (rule 21) | None |
| Light | Image, named, with its state in words in the description | None | None |
| Scope | Image, named, with a description in words from its trace's lowest and highest points | None | None |
| A drawing whose text alternative is another element (the patch map) | Image, named ("Patch map"), with a description that says where its text alternative is | None | None |
| An app-driven tree (the patch outline) | Tree, named | None | Focus |
| A row of that tree | Tree item, with its level, position in set, size of set, and expanded state | None | Focus, and Click where a click does something |
| A message meant to be announced (an output safety warning, an alert in the status line) | Status | Its text | None |
| Tile | Group, named by its module | None | Focus, and Click to select its module |
| Panel | Group, named | None | None |

### Rules

1. **The tree and the screen come from one source.** Nothing writes accessibility names, values, or positions by hand beside what the view draws. *Why:* a tree that disagrees with the screen is worse for a screen reader user than no tree (ADR-0028).
2. **Every interactive element has a role that can be operated,** a non-empty name, and its value. A test checks every element kind in the table. *Why:* in the spike, radios and selects fell through to read-only labels, shown on screen but not operable. The snapshot tests did not notice.
3. **Names, values, and descriptions use our own words, the same ones the screen shows,** with units ("800 Hz", not "800"). *Why:* NFR-UX-06.
4. **A slider's value is a number, and its text with the unit goes in the description,** until AccessKit's macOS adapter can give a spoken value beside a number. Then the text moves to the value text, and this rule changes. *Why:* the spike found that with text in the value, VoiceOver can say the unit but cannot step the knob. Being able to change a control matters more than hearing its unit in the same phrase.
5. **A screen reader's SetValue is applied as sent,** clamped to the range. Increment and Decrement move one step, the same step as an arrow key on that control; for a stepped parameter, one option. *Why:* VoiceOver speeds up while a key is held, and some screen reader users rely on that to cross a range quickly. Phase 1 records a listening test with VoiceOver in its pull request. If a held key cannot be used to set a value, this rule is changed before phase 1 merges.
6. **Every screen reader action becomes an ordinary message,** through the same path as the pointer or the keyboard. *Why:* undo, autosave, MIDI feedback, and the unsaved mark then work with no extra code, as the spike showed.

   **Screen reader edits group into undo steps** like a released arrow-key group ([undo spec](undo.md)). A screen reader sends no key-down or key-up, so each SetValue, Increment, or Decrement counts as one arrow step followed at once by its release. The app owns this group, beside `ArrowGroup` in `edits.rs`.
   - **Opens** on the first such action on a control.
   - **Joins** when the next one reaches the same control within 500 ms of the last.
   - **Closes** 500 ms after the last action. It closes sooner on an action on another control, a pointer press on the canvas, a key press other than a modifier alone, focus moving to another node, the window losing focus, an inspector edit, or Undo or Redo.
   - **Undo stays enabled** throughout, because no key is held. Undo or Redo closes the group first, then acts on it as one step.
   - **The wake that ends the group is one-shot, and it is the canvas's deadline.** The canvas is shown the app's open edit group with its deadline (`EditGroupView` in `loom-canvas`). It is not told who opened the group: a released arrow group and a screen reader's group look the same to it. It asks for one redraw at that time and reports the key or pointer press that ends the group sooner. With no group open, nothing wakes for it. *Why:* a timer that runs while a screen reader is attached would be idle work (NFR-PERF-04), and the canvas already wakes this way for arrow keys.
   - **The wake needs the canvas to be drawn and to have the input.** While a panel owns the input, the canvas ends any open group at its next event, so the group closes sooner, not later. While the window is not drawn at all (minimised, or fully covered), no wake comes. The group then closes at the next thing that closes it. An action that arrives after the deadline never joins: the app compares the action's time with the deadline, so the step is cut correctly even with no wake. Only the moment the step is recorded is late.
   - **A Click is not part of a group.** A Click on a toggle, a button, or an option ends an open group and is one undo step of its own, as a pointer click is. A Click that changes nothing, such as one on the option already chosen, still ends the group.
   - **An action waits while an arrow key is held.** The held key's group owns the edit in progress and may step again, so a screen reader's action does nothing then, as Undo does nothing then ([undo spec](undo.md)). *Why:* two owners of one edit would share an undo step.
   - **Hover text on Undo names the open group** "the screen reader edit".

   *Why:* one VoiceOver session in the spike made about 300 undo steps. `ArrowGroup` itself would never close, since its group waits for a key release that never comes. A group of its own, with the triggers of a released arrow group, gives a screen reader user what an arrow-key user gets once they let go of the key.
7. **The tree updates when what it reports changes, and sends only the nodes that changed:** after a message that changes a view model, and when widget state it reports changes without a message (keyboard focus, a text selection or caret, a scroll offset that moves bounds, an open or closed list), or the layout or scale factor changes. A node that shows a live value (section 3, rule 7) is sent at most every 250 ms. While a screen reader is active, every live value node on screen is kept current at that rate, whether or not it has keyboard focus. A node off screen, or below a panel that owns the input (section 5, rule 3), keeps the value last sent. When the screen reader leaves (`deactivate_accessibility`), the runtime stops building the tree. *Why:*
   - **A screen reader can read any node without moving keyboard focus there,** so a node updated only on focus would show a stale value under the screen reader's cursor, against rule 1.
   - **AccessKit does not tell the app where that cursor is,** so the layer cannot update only the node under it. In `accesskit` 0.25.2 the app's only callbacks are activation, actions, and deactivation. `accesskit_macos` 0.27.2 answers VoiceOver's value reads from its own copy of the tree (`node.rs`, `accessibilityValue`), with no call to the app. A Focus action arrives only when the user has set keyboard focus to follow the VoiceOver cursor. The Windows and Linux adapters were not checked, so this rule holds on every platform. If a later AccessKit release reports the cursor on a platform, the rule narrows there to the node under it.
   - **The cap keeps a meter from flooding the screen reader.** `accesskit_macos` posts a value-changed notification for every node whose value changes (`event.rs`, `node_updated`). Whether VoiceOver speaks those for nodes it is not on is checked in the phase 1 listening test.
   - **No polling.** The spike rebuilt the whole tree every 500 ms (NFR-PERF-04). Here only changed live nodes are sent, and nothing is sent while no screen reader listens.

   How phase 1 does it, on Iced: after each message the app handles, other than the layer's own, it asks the window where the canvas is and builds the tree when the answer comes, one request at a time. It keeps the nodes it last sent and sends only those that differ. A message that only moves live values (lights, meters, traces) asks for nothing: it marks them as waiting, and a 250 ms timer that runs only while they wait sends them. A timer or a watcher that only looks (the device and MIDI port checks, autosave, the pickup check, the audio load reading) asks for a build only when it changed something the tree reports, which the app tells from a small key taken before and after (`PollKey` in `a11y.rs`). So at rest no timer runs, and the last value is sent after a meter stops. `accesskit_macos` 0.27.2 has no callback for a screen reader that leaves, so on macOS the tree is still built after messages once one has listened; the app handles a deactivation event for the adapters that send one.

   While a panel owns the input, each tile's node is sent as hidden, so a screen reader finds nothing below the panel, and what is inside a tile keeps the value last sent. A live value whose node lies outside the canvas's part of the window is not sent, and nothing waits for it, so the live timer does not run for a meter nobody can see. It is sent again when it comes back on screen.
8. **Bounds are sent in physical pixels,** converted from the logical layout with the window's current scale factor, as AccessKit expects. A scale factor change resends the bounds of every node. *Why:* layout and the control channel use logical pixels. Sending them unconverted would put VoiceOver's cursor at half the right place on a Retina display.
9. **Focus on a node never changes the patch.** Focus on a tile's node changes nothing: selecting a tile follows an explicit Click or Enter. A tile's node offers Click, which selects its module through the same message as a pointer click, so the inspector follows. Focus on a control selects that control's tile, as rule 20 says, because keyboard focus on the canvas lives only in the selected tile. Selection is not a patch edit and not an undo step. *Why:* an assistive tool that only inspects the window must not change the patch. The spike's tile selection on a tile's focus is removed.
10. **A text field gives its text as AccessKit text runs,** with the caret and the selection as positions in them, and handles SetTextSelection and ReplaceSelectedText. *Why:* with only SetValue, a screen reader cannot move its cursor through the text or read where the caret is.
11. **The adapter attaches before the window first shows.** The shell creates the window hidden, attaches `accesskit_winit`, then shows it. *Why:* AccessKit documents this order. The spike worked without it by luck.
12. **A tree is driven by the app.** The app handles Up, Down, Right, Left, Home, End, and type-ahead in a tree, and keeps which rows are expanded. The tree's visible rows are sent as direct children of the Tree node, in the order shown, and each still carries its level, its position in set, its size of set, and its expanded state. *Why:* `accesskit_macos` 0.27.2 has no expand or collapse action, reports no level, position, or size of set, and lists only a tree's direct items as its rows (`node.rs`, `accessibilityRows`). Rows nested as children of rows would not be listed there. The Windows adapter maps level, position, size, and expand state †, and the Linux adapter position and size †, so sending them costs nothing and helps where they are read. Keys the app handles work the same everywhere, with or without a screen reader.
13. **Elements may set `controls` relations to other nodes by UI ID,** inside or outside their own panel or custom element, and no other relation. The runtime turns each UI ID into its node ID when the tree is sent, and drops a relation whose target is gone. *Why:* `controls` is the only AccessKit relation the three platform adapters map: on macOS to linked UI elements (`node.rs`, `accessibilityLinkedUIElements`), on Windows and Linux to "controller for" †. `flow_to`, `described_by`, and `details` are mapped by none †, so a relation of those kinds would reach no screen reader. The canvas uses it from each output port to the inputs its cables feed ([canvas accessibility](canvas-accessibility.md), rule 21).
14. **A slider's number is the parameter's own value, with the parameter's minimum and maximum,** not the control's travel from 0 to 1. A step size is declared only where equal steps of the control are equal steps of the value: a linear parameter, or a stepped one. A slider on a logarithmic or decibel parameter declares no step. Increment and Decrement still move it by what an arrow key moves it (rule 5). *Why:* "1000" on a control that reads "1000 Hz" is true, and "0.57" is not. One arrow key press on a logarithmic control is 1% of its travel, which is a different number of hertz at each place, so no one step would be true.
15. **A select says its chosen option and steps one option at a time,** with Increment and Decrement, on a tile and in the inspector. It offers no Expand or Collapse until phase 3, when our own select can give its open list as nodes. Nothing outside the canvas can open a tile's choice list. *Why:* a combo box that says it can expand, and then shows a screen reader nothing to walk, is worse than one that steps.
16. **An XY pad and a range are a named group holding one slider per value.** Each slider is named by its parameter and steps as an arrow key steps that value. A range's handles do not pass each other: a value set past the other handle stops at it, as a drag does. A handle stops from the side it is on: where a typed value has left low above high, low moves freely above and stops when it comes down to high. Where the two are together, low is the one below and high the one above, so each can move away. *Why:* AccessKit has no role for one control with two numbers, and two sliders are what the two values are.
17. **A readout is a label whose value is its name and the text it shows,** as "Cents: 12.5 cents", and it has no label of its own. An adapter gives a label node's label in place of its value (`accesskit_macos` 0.27.2, `label_is_exposed_in_value`), so a name set as the label would hide the reading. No label node has both. **A scope is an image** whose description says its trace's lowest and highest points in words, updated at most 4 times a second (rule 7). A light, and the other drawings a tile shows, are images named by what they are bound to, with the text the screen gives for them as the description. *Why:* a screen reader user should learn that a signal is there and roughly how large it is. A stream of numbers at the frame rate would be noise.
18. **A widget with no caption is named by what it is bound to,** the name its hover text shows. *Why:* most meters and scopes have no caption on the tile, and rule 2 needs every node to have a name.
19. **A tile gives no port nodes in phase 1.** Ports come with the canvas accessibility work ([canvas accessibility](canvas-accessibility.md)). *Why:* that spec designs how ports and cables are read, and nodes added before it would have to be redone.
20. **The Focus action on a control moves keyboard focus there,** and the tree reports focus on the control that has it, or on the window when none has. In phase 1 the app asks the canvas with a request that names the module and the widget, and the canvas reports each focus change once, whatever caused it. Focus on one value of an XY pad or a range, or on one option, focuses the control that holds it. Focus on a control also selects its tile, through the path a press or Tab on the control already takes, so arrow keys then act on it and the inspector follows. Focus on a tile does nothing, and no Focus action changes the patch (rule 9). A widget that only shows a value offers no Focus. A request waits while something else has the input, and is applied on the first redraw after the canvas has it again, so its events are never dropped. When the selection moves to another tile from outside the canvas, as on a screen reader's Click on that tile, focus left on the first tile is dropped and the tree reports focus on the window. The request is applied when the canvas next draws, and the runtime draws the window after every message, so an action that arrives alone still moves the focus. *Why:* VoiceOver can be set to move keyboard focus with its cursor, and a user who then presses an arrow key expects it to act on the control they hear. The request names a module and a widget, nothing of the toolkit, because the focus effect of phase 2 will carry the same.
21. **A meter reports the number its text shows,** not the level as stored. A level meter that reads "-6.0 dB" reports -6, from -60 to 0. A meter on a display value reports that value, with the display's range. The number is the one the text writes, digit for digit: it is read back from the text's one decimal, so "-12.0 dB" is -12 and never -12.04. Where the text says a value past the end of the range, the bounds reach out to it, so the number is never outside its own bounds. The code that writes the text makes the number, and the tree reads it from there (`Reading` in `loom-canvas`, `meter_reading` in `scene.rs`). When the text names no single number, the node has no number and says the text only: a stereo meter says two levels, and a level under the floor reads "below -60 dB". *Why:* a stored level of 0.9 means nothing to a listener, and working out decibels a second time in the tree would let the two drift apart. Decided 2026-11-11, the same answer as rule 14 gives for a knob.

## 8. Text input and IME

**Status:** Agreed, phase 3. The choice of text crate (rule 8) is made at plan step 2.4.

### Rules

1. **Shaping, font fallback, line breaking, glyph drawing, and the editing model come from existing crates.** We write the glue, not a text engine. *Why:* ADR-0041, decision part 3.
2. **A single-line field supports:** caret, selection, word movement, Home and End, clipboard through effects, and undo inside the field. *Why:* users expect these in any text field, and their absence is noticed at once.
3. **Undo inside a field is separate from patch undo,** and ends when the field loses focus or commits. This changes today's behaviour, where Undo and Redo do nothing while a field has focus ([undo spec](undo.md)). The change arrives with each field as phase 3 moves it; until then the field keeps today's behaviour, and the parity checks expect it. *Why:* Cmd+Z while typing a name should undo the typing, not the last patch edit.
4. **IME composition goes to the focused field.** The shell sends preedit and commit events, shows the preedit text in the field, and reports the caret area so the candidate window appears beside it. *Why:* people who type Chinese, Japanese, or Korean need composition to type at all.
5. **A text field reports its text, caret, and selection to the accessibility tree** (section 7, rule 10). *Why:* a screen reader reads and moves through text by these.
6. **Fonts resolve as they do today, through the move.** Iced is built without its `fira-sans` feature, so `Font::DEFAULT` is the system sans-serif, found by `cosmic-text`'s font system, plus Iced's icon font (`iced_graphics` 0.14, `text.rs`). The new text stack picks the same family on each platform, and the `--screenshot-window` pixel diff checks it at every phase. System fonts fill in missing glyphs, as now. *Why:* a different font measures differently, which moves `locate` answers, hit tests, and layout, and breaks the parity gate. Embedding a UI font of our own would make text the same on every platform, but it changes how the app looks, so it is out of scope here.
7. **The multi-line editor (patch info) comes last,** and may stay on Iced's editor until it is done. *Why:* it is the largest piece, and the rest of the move should not wait for it.
8. **The text crate is chosen at plan step 2.4,** the first text drawn without Iced, between `cosmic-text` and Parley. The maintainer decides, from a short comparison recorded as a research snapshot, by these criteria in this order:
   1. IME composition in a single-line field (rule 4).
   2. Reporting text runs, caret, and selection to AccessKit (rule 5).
   3. One font system shared by the canvas and the panels.
   4. How its glyphs are drawn. `cosmic-text` has `glyphon` †, a `wgpu` glyph atlas that Iced's own `cryoglyph` forks. No such atlas for Parley is known to us †: its glyphs would be rasterised with `swash` into an atlas of our own, or drawn through Vello. An atlas of our own counts against rule 1 and must be costed in lines; Vello brings the compositing cost ADR-0041 names for Masonry.
   5. It picks the same system font as today (rule 6).
   6. A licence that passes `cargo deny check`, and versions that match the `winit` and `wgpu` in the lock file.

   This rule is then replaced by the choice, and the plan's steps 2.4 and 3.3 and phase 4 name the chosen crate. *Why:* the two parts of phase 3 most likely to overrun are text fields and IME. Parley's release notes list AccessKit text properties and its editor tracks IME composition †, while `cosmic-text` is what Iced uses today. The comparison is cheapest once phase 2 has its own text drawing to test against.

## 9. Custom elements

**Status:** Agreed, phase 1 for the canvas's input and drawing (plan steps 1.2 and 1.3), phase 2 for the full contract.

A custom element is the layer's supported escape hatch. The patch canvas is the first.

### Rules

1. **A custom element owns its own state, drawing, and input.** It gets pointer and key events inside its bounds, draws into a `wgpu` render pass with a viewport the runtime gives it, and keeps whatever retained data it needs (the canvas keeps its `Scene` and GPU buffers).
2. **It takes part in focus as one stop,** and manages focus inside itself (section 6, rule 1).
3. **It gives accessibility nodes under its own UI ID,** one per inner part a user can reach (a tile, a control, a port), in its own focus order.
4. **It reports when it needs a redraw or animation frames.** That decides whether the runtime composes a frame, not which layers the frame holds: each composed frame draws every visible layer, the canvas included, from its retained GPU data. A panel change therefore redraws the canvas with no rebuild of its scene. Pan and zoom only update its uniforms (ADR-0010).
5. **It reports events to the app as messages,** and never edits app state itself. The canvas already works this way (`CanvasEvent`).
6. **It takes focus requests for its inner parts.** A focus effect whose UI ID names an inner part (a tile control on the canvas) is passed to the custom element, which focuses that part, and the tree reports it (section 6, rule 5). *Why:* a panel can then send focus into the canvas, as Enter on an outline row does, and the screen reader follows.
7. **Its nodes may set `controls` relations,** to its own nodes or to nodes outside it, by section 7, rule 13.

*Why:* a pure pipeline never covers everything. An editor or a canvas needs its own drawing and input. A hatch that is part of the contract keeps such elements inside focus, the accessibility tree, and the tests, rather than beside them.

## 10. Test harness and control channel

**Status:** Agreed, phase 2. Rule 4 is built in phase 2; until then, the phase 1 tests read the tree from `a11y/tree.rs` directly.

### Rules

1. **One runtime, two drivers.** The window and the headless `Ui` harness run the same code for events, layout, messages, and effects. Only the event source and the drawing target differ. *Why:* a test then checks what the app does, not what a copy of it does.
2. **The harness's event and query methods keep their meaning:** `headless`, `press`, `release`, `drag`, `move_to`, `hold`, `tap`, `tap_key`, `key_down`, `key_up`, `typewrite`, `click_text`, `control`, `control_bounds`, `field`, `value_field`, `text`, `focus`, `focused_field`, `output`, `input`, and the rest. Their call sites do not change. `run_task(Task<Message>)` becomes `run_effects(Effects<Message>) -> Vec<Message>`, with the same meaning: run until no messages are left, and return them all. Its call sites change mechanically in phase 2, in one commit with no other change. *Why:* every `loom-app` test (1,004 at commit `a781de4`) depends on the harness. Keeping the meaning fixed shows whether behaviour changed; renaming the one method tied to Iced is the only way to leave Iced.
3. **Event tests need no GPU.** Drawing tests render to an offscreen `wgpu` texture, and skip with `NO_GPU` where there is no adapter. *Why:* CI machines often have no GPU.
4. **The harness can read and act on the accessibility tree:** `Ui::a11y_tree()` returns it, and `Ui::a11y_action(name, action)` performs an action on a node found by name. *Why:* a test can check the window the way a screen reader meets it.
5. **A control channel command always gets its reply.** A command that waits for the window to be drawn asks for a message once the next frame is shown (section 2, the effects table), which also makes the runtime draw. So the two-frame wait of [control channel rule 5](control-channel.md#rules) completes even when the command changes nothing visible, with no animation frames and no update per frame. This replaces `ControlFrame`. *Why:* without a frame to wait for, a command that changes nothing would never be answered, and a script would hang.
6. **The control channel's protocol does not change.** `locate` finds targets from layout data and UI IDs, not from a toolkit's widget tree, and answers in logical pixels. Its replies are the same for every example in the [control channel spec](control-channel.md). *Why:* agents and scripts built on it keep working.

## Acceptance criteria

Each phase also passes the parity gates in [the plan](../dev/leaving-iced-plan.md#parity-gates).

Phase 1:

- [x] `loom-ui-core` exists, depends on no toolkit and no LoomDSP crate, and has an example program. `crates/loom-ui-core/tests/purity.rs` checks the dependencies, and the example is `examples/hit_test.rs`.
- [x] `loom-canvas` names Iced only on its widget side: `widget.rs`, the files under `widget/` (the primitive wrapper and the Iced painter among them), and `iced_convert.rs`. `crates/loom-canvas/tests/purity.rs` checks it.
- [ ] A test checks that every interactive element kind on a tile and in the inspector has an operable role from the table, a non-empty name, and a value, with the unit in the description.
- [x] A test checks that screen reader SetValues on one control, each within 500 ms of the last, make one undo step; that an action on another control, a key press, or Undo closes the group; and that Undo is enabled while the group is open.
- [x] A test checks that Increment on a knob moves it by the same step as an arrow key. Both take it from one function, `loom_canvas::arrow_step`.
- [ ] A test checks that the tree changes when a view model, focus, or a scroll offset changes, and does not change on a message that changes none of what it reports. Done for a view model and for focus (`a11y/tests.rs`). A scroll offset comes with the inspector's nodes.
- [x] A test checks that, while a screen reader is active, a moving meter's node reports changing values both with and without focus, and its final value after the meter stops; that it is sent at most every 250 ms; that it is not sent while the meter is off screen or below a panel that owns the input; and that nothing is sent after `deactivate_accessibility`.
- [x] A test checks that the Focus action on a tile does not change the selection.
- [x] A tree built at scale factor 2 has bounds twice the logical layout.
- [ ] Accessibility node IDs for inspector rows survive adding and removing a module, and the table drops the entries of removed elements.
- [ ] An exposed parameter on a sub-patch tile and the inner module's own row for it have different UI IDs, and both are found by `locate`.
- [ ] A VoiceOver check is recorded in the pull request: which controls were read, which could be changed by keyboard, how a held arrow key felt (section 7, rule 5), whether a meter under the VoiceOver cursor reads its current value, and whether VoiceOver announces meters it is not on (section 7, rule 7).

Phase 2:

- [ ] A test runs an effect the harness cannot run and checks that it panics with the effect's name.
- [ ] A test checks that one message calls `update` once, even when its effects produce more messages.
- [ ] A test checks that background work runs inline in the harness and its messages arrive in order.
- [ ] With the app idle for 10 seconds, `--measure-idle` reports no redraws, no panel rebuilds, and no animation frames, while the periodic deadlines of section 2, rule 8 still run.
- [ ] Moving the window to a display with another scale factor resends every node's bounds.
- [ ] The tree is exposed on macOS, Windows, and Linux. Narrator and Orca checks are recorded, or listed as not checked.
- [ ] `Ui::a11y_tree()` and `Ui::a11y_action()` exist and a test uses each.
- [ ] The text crate is chosen by section 8, rule 8, and the comparison is recorded.

Phase 3:

- [ ] A message that changes nothing rebuilds no panel. A test checks the rebuild counter.
- [ ] While audio plays, with meters and lights moving and nothing else changing, `--measure-idle` reports zero panel rebuilds.
- [ ] Moving the pointer over the toolbar, or typing in the browser search, rebuilds no other panel.
- [ ] A benchmark times one value change with the module browser showing 1,000 entries and a full modulation matrix, and records memory and the first build time.
- [ ] Removing a row from a keyed list moves no widget state, focus, or accessibility node onto its neighbour.
- [ ] Tab and Shift+Tab reach every interactive element in every view, with a visible ring, and the screen reader follows.
- [ ] Opening an overlay moves focus into it and hides the layers below from the tree; closing it restores focus, or moves it by section 6, rule 4 when its element is gone.
- [ ] A test checks the key order of section 6, rule 6, step by step.
- [ ] Ctrl+Tab and Ctrl+Shift+Tab cycle the canvas, the right column, and the toolbar in both directions, wrapping at the ends; return to each region's last focused element; work while a text field has focus; and do nothing while an overlay holds focus. On macOS, Ctrl+Tab neither steps between fields nor leaves the matrix or a range part, as plain Tab does.
- [ ] A focus effect with the UI ID of a tile control, sent from a panel, focuses that control on the canvas, and the tree reports it.
- [ ] A tree's visible rows are direct children of its Tree node, each with its level, position in set, size of set, and expanded state.
- [ ] A `controls` relation to a removed element is dropped from the tree.
- [ ] A screen reader can move its cursor through the browser search text and hear the caret position.
- [ ] IME composition works in the browser search on macOS with a Japanese or Chinese input method. Recorded in the pull request.

## Out of scope

- **Plugin window embedding** (OQ-13). Section 2, rule 9 keeps it possible: a host-window driver can be added beside the `winit` shell. Building it, and choosing what that driver uses, is later work, after a spike.
- **The web target** (ADR-0026).
- **Visual redesign.** Every view looks the same after each phase.
- **Read-tracking state, such as signals.** Comparing panel models is enough at LoomDSP's size ([research](../research/2026-11-ui-change-propagation.md#not-proposed)). Revisit if a measured panel cannot meet its cost with section 3, rule 1.
- **Components with their own message types** inside `loom-ui`. LoomDSP's panels use the app's message. A mapping like Elm's `Html.map` can be added when a second app needs it.
- **Publishing the crates.** Waits until the API settles (FR-SDK-06).
- **Embedding a UI font.** Text uses the system sans-serif today, so it differs between macOS, Windows, and Linux. An embedded font would fix that and make screenshots match across machines, but it is a visual change with its own licence check. It is its own issue and decision, not part of this move (section 8, rule 6).

## Decisions made for this spec

Recorded so later readers know these were chosen, not left open. Each is written into the rule named.

| Question | Decision (2026-11-09) | Rule |
|---|---|---|
| How a held arrow key steps a knob for a screen reader | SetValue is applied as sent; Increment and Decrement move one step; confirmed by a listening test in phase 1 | Section 7, rule 5 |
| How screen reader edits group into undo steps | Their own group beside `ArrowGroup`: each action is a step and its release; joins within 500 ms on the same control; closes 500 ms after the last action or early on another control, a pointer press, a key press, a focus move, window focus loss, an inspector edit, or Undo or Redo; Undo stays enabled | Section 7, rule 6 |
| Whether focus on a tile selects it | No. Only Click or Enter selects | Section 7, rule 9 |
| Number or text for a slider's value on macOS | A number, with the unit in the description, until AccessKit's adapter gives value text | Section 7, rule 4 |
| How often live value nodes update | At most every 250 ms. While a screen reader is active, every live value node on screen updates, focused or not, because AccessKit does not report where the screen reader's cursor is. Widened from "only while focused" on the same day, so a meter under the cursor is never stale | Section 7, rule 7 |
| Which text crate | Chosen at plan step 2.4 by the criteria in the rule, including how its glyphs are drawn | Section 8, rule 8 |
| Hooks for the canvas accessibility spec | Added at the maintainer's request: `controls` relations, focus into a custom element's parts, an app-driven tree, an image with its text alternative elsewhere, and a key between regions | Section 6, rule 7; section 7, rules 12 and 13; section 9, rules 6 and 7 |
| Which key moves between regions | Ctrl+Tab forward and Ctrl+Shift+Tab back, wrapping, with Ctrl on every platform, and working from a text field. Chosen over F6, which needs Fn on most keyboards | Section 6, rule 7 |
| Which layout engine | `taffy`, subject to `cargo deny check` and versions that fit the lock file | Section 1 |
| How a UI ID is written as text (2026-11-10) | A path: keys joined by `/`, digits as numbers, `/` and `%` escaped inside a name, and a name of digits escaped so it reads back as a name. The control channel's short names stay | Section 4, rule 5 |
| Whether a slot's controls stay the same elements when its part is swapped (2026-11-11) | No. They are keyed under the part, so a swap makes new elements with new node IDs. The new part's controls can differ completely, and a screen reader must not read the new filter's cutoff as the old one's | Section 4, the key table |
| What an XY pad and a range are to a screen reader (2026-11-10) | A named group holding one slider per value, each stepping as an arrow key does | Section 7, the role table and rule 16 |
| What a readout and a scope are (2026-11-10) | A readout is a label whose value is its text. From 2026-11-11 the value also says the readout's name, and the node has no label, because the macOS adapter reads a label node's label in place of its value. A scope is an image with a short description in words, updated at most 4 times a second | Section 7, rule 17 |
| What number a non-linear knob reports (2026-11-10) | The parameter's own value in its own range, with no step size declared; Increment and Decrement move what an arrow key moves | Section 7, rule 14 |
| What number a meter reports (2026-11-11) | The number its text shows, with the text as the description and the drawn range as minimum and maximum. No number where the text names none or two | Section 7, rule 21 |
| What the canvas is told of an open undo group (2026-11-11) | That an edit group is open, and its deadline. Not who opened it | Section 7, rule 6 |
| What the Focus action does on a control (2026-11-10) | It moves the canvas's keyboard focus there, in phase 1. On a tile it does nothing. The request names a module and a widget | Section 7, rule 20 |
| Whether Focus on a control selects its tile (2026-11-11) | Yes, as Tab or a click on the control does, so arrow keys work and the inspector follows. Selection is not an edit or an undo step. Focus on a whole tile still does nothing | Section 7, rules 9 and 20 |
| What a select offers before phase 3 (2026-11-10) | Its chosen option, and Increment and Decrement, one option per step. No Expand or Collapse, on a tile or in the inspector | Section 7, rule 15 |
| Whether tiles give port nodes in step 1.4 (2026-11-10) | No. They come with the canvas accessibility work | Section 7, rule 19 |
| Whether a tile's node offers Click (2026-11-10) | Yes. Click selects the tile's module; Focus alone still changes nothing | Section 7, the role table and rule 9 |

| What the runtime's contract is shaped by (2026-11-11) | LoomDSP's own needs, not Iced's `Task`, `Subscription`, widget operations, or application builder. Brylie: "let's use our own design as needed, so we're not bound by the original decision to use Iced" | Section 2 |
| How a deadline becomes a message (2026-11-11) | `next_deadline` returns the time and the message together, built from state after each update | Section 2, the sketch |
| Whose effects run first in one batch (2026-11-11) | Each message's effects run right after its own update | Section 2, Behaviour 4 |
| Whether `update` gets the time (2026-11-11) | Yes, from its `Context`, with a waker, the focus, and the window | Section 2, the sketch; rule 11 |
| Whether paste reads the clipboard in the same turn (2026-11-11) | No, one step later: reading is an effect and the text arrives on the next turn. Brylie chose it, so `update` never waits on the system and the same shape serves a plugin and the web target | Section 2, the effects table |
| Who asks for animation frames before phase 3 (2026-11-11) | The app, with `animating`. A frame draws and runs no update. Section 3, rule 8 moves the asking to elements in phase 3 | Section 2, the sketch |
| What replaces `ControlFrame` (2026-11-11) | A message once the next frame is shown | Section 2, the effects table; section 10, rule 5 |
| How watchers live (2026-11-11) | As handles in app state, taking their waker from the context; dropping the handle stops the watcher and never blocks | Section 2, rule 7 |
| What decides a redraw in phases 2 and 3 (2026-11-11) | A changed panel model, the routing target, an effect, or the system. The bridge's target says "an update ran", which keeps today's cost | Section 2, rule 10 |
| Effects dropped (2026-11-11) | The focus query, select all as its own effect, timers, window commands, and a message queued for the next turn | Section 2, the effects table |

Entries marked † are from web search summaries and the projects' prose, not from their source. Check them before relying on them.
