# Plan: Leaving Iced, Step by Step

- **Kind:** Implementation plan, written to be handed to a coding agent. It is not a decision and not a specification.
- **Date:** 2026-11-06, at commit `6091e52`.
- **Issue:** [#683](https://github.com/brylie/LoomDSP/issues/683) covers the whole move, all phases.
- **Background:** [Our own UI stack instead of Iced](../research/2026-11-own-ui-stack-tradeoffs.md) and [the accessibility spike](../research/2026-11-a11y-beside-iced-spike.md).
- **Decisions it touches:** [ADR-0009](../adr/0009-ui-stack-iced.md) (Iced for the UI), [ADR-0010](../adr/0010-own-patch-canvas.md) (our own canvas), [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md) (plain-data view models), [ADR-0028](../adr/0028-app-accessibility-waits-for-iced.md) (accessibility waits for Iced).

## Goal

Replace Iced with a thin UI layer of our own, one step at a time, so that:

1. LoomDSP owns its accessibility tree, through AccessKit, on macOS, Windows, and Linux.
2. LoomDSP owns its window and event loop, through `winit`, so plugin embedding (OQ-13) is not blocked by a toolkit.
3. Text shaping and drawing still come from existing crates. We do not write a text shaper, a font loader, or a glyph rasteriser.
4. Every test that runs today still runs, and agents can still drive the running app (control channel, screenshots, `--dump-ui`) at every step.

The app must build, pass `mise run check`, and behave the same for a user after every phase. Each phase is one pull request, or a short series. No phase leaves the app half on one stack and half broken.

## Read first

The agent reads these before starting. Each step below assumes them.

- `AGENTS.md`, all of it. The hard rules, spec first, where tests live, writing style.
- `docs/dev/architecture.md`, the section on how the app handles a message.
- `docs/design/control-channel.md`, `docs/design/visual-system.md`, `docs/design/audio-qa.md`, `docs/design/undo.md`.
- The two research snapshots linked above.
- `crates/loom-app/src/ui_test.rs` (the headless harness), `screenshot.rs`, `control.rs`, `control/run.rs`, `snapshot.rs`, `a11y.rs`.
- `crates/loom-canvas/src/widget.rs` and `gpu.rs`.

## What we start from

Measured at commit `6091e52`.

| Part | Today |
|---|---|
| App shell | `iced::application(start, App::update, App::view)` in `crates/loom-app/src/lib.rs`, Elm style: `update(Message) -> Task<Message>`, `view() -> Element`, subscriptions in `subscriptions.rs`. |
| Iced widgets used | `container` 53, `canvas` 49 (our `PatchCanvas`), `button` 48, `pick_list` 15, `tooltip` 12, `text_input` 11, `scrollable` 10, `opaque` 10, `mouse_area` 9, `space` 8, `pin` 8, `checkbox` 7, `radio` 5, `slider` 3, `markdown` 3, `toggler` 2, `text_editor` 1, plus `row`, `column`, `stack`, `text`, `rule`. |
| Heaviest Iced users | `style.rs`, `matrix.rs`, `ui_test.rs`, `lib.rs`, `controls.rs`, `pointer.rs`, `editing.rs`, `inspector.rs`, `screenshot.rs`, `input_owner.rs`. |
| Canvas | `loom-canvas`: an Iced `Widget` (`widget.rs`) and an Iced shader `Primitive` (`gpu.rs`, `canvas.wgsl`). It uses Iced's geometry types (`Point`, `Rectangle`, `Size`, `Color`) throughout, and Iced's text renderer for tile text. |
| Text | Iced, through `cosmic-text` and `cryoglyph`. |
| GPU | `wgpu` 27, through `iced_wgpu`. |
| Window | `winit` 0.30, through Iced. Features `x11` and `wayland`. |
| Tests | About 912 `#[test]` functions in `loom-app`. 15 files drive the `Ui` harness, which builds Iced's `UserInterface` headlessly and falls back to `tiny-skia` without a GPU. |
| Agent tools | `--control` / `--control-script` (JSON lines, commands by name, `locate` through an Iced widget `Operation`), `--screenshot`, `--screenshot-window`, `--dump-ui`, `scripts/bundle-macos.sh` for desktop automation. |
| Accessibility | The `a11y-spike` feature: an AccessKit tree built from `App::ui_snapshot`, attached to Iced's `NSView` on macOS only. |

## Rules for this work

1. **The view models are the seam.** `App::ui_snapshot`, `inspector.rs`, and the other plain-data models (ADR-0021) describe what the user sees. The accessibility tree, the control channel, `--dump-ui`, and the tests all read them. New code reads them too. Nothing reads a toolkit widget tree to find out what is on screen.
2. **One runtime, two drivers.** The window and the headless harness must run the same code for events, layout, and messages. Only the event source and the drawing target differ. This is what keeps the tests honest.
3. **No hidden skips.** A harness that cannot run an effect (a file dialog, a window command, a timer) panics with a message, as the `Ui` harness does today.
4. **Do not invent text.** Shaping, font fallback, line breaking, glyph rasterising, and the editing model come from Parley and `swash`, chosen at step 2.4 ([spec](../design/ui-layer.md), section 8, rules 1 and 8). The glyph atlas that draws them is ours, because no atlas crate takes Parley's glyph runs at our `wgpu`; it stays small and behind the `Painter` boundary.
5. **Licences.** Every new crate goes through `cargo deny check` before it is used. Only permissive licences (ADR-0006).
6. **Clean room.** The agent may read the source of dependencies accepted under ADR-0006, which includes Iced, `winit`, `wgpu`, `cosmic-text`, and AccessKit, and from step 2.4b Parley, its crates, and `swash`. It must not read the source of other audio applications or other UI frameworks that are not dependencies.
7. **Same versions during the move.** Use the `winit` and `wgpu` versions Iced 0.14 already pulls in, so the lock file never holds two of either.
8. **Parity gates** at the end of every phase (below). A phase is done only when they pass.
9. **Build the layer for reuse.** The new UI crates are candidates for publishing later (FR-SDK-06), and could serve other audio tools beside LoomDSP. So:
   - `loom-ui-core` and `loom-ui` never depend on `loom-core`, `loom-patch`, `loom-canvas`, or `loom-app`. They know about windows, input, layout, text, widgets, and accessibility, not about patches or modules.
   - The shell (phase 2.2) and the runtime (phase 2.1) take the app as a trait or generic parameter, not as `loom_app::App`.
   - The instanced shape renderer from `canvas.wgsl`, if it moves into `loom-ui` for phase 3, stays free of patch types. The patch canvas keeps its own crate on top of it.
   - Each crate gets a crate-level doc comment and an example program that does not use LoomDSP.

   Publishing waits until the API settles. This rule only keeps the door open, so publishing later does not need a second refactor.

### Parity gates

Run all of these at the end of each phase and list the results in the pull request.

- `mise run check`, including the 80% coverage gate.
- All `loom-app` tests pass with no change to what they assert. Changes to harness internals are allowed. Changes to expected values need a reason in the pull request.
- The control channel examples in `docs/design/control-channel.md` give the same replies, run through `--control-script`.
- `--screenshot-window` for `examples/patches/subtractive.ron` with `--select 4`, before and after, compared by eye and with a pixel difference. Small differences in text anti-aliasing are fine; layout shifts are not.
- `--measure-idle --bench-seconds 10` on `subtractive.ron`: CPU and redraw counts no worse than before (NFR-PERF-04).
- `--generate 300 --bench`: frame rate no worse than before.
- From phase 1 on: a VoiceOver check on macOS, recorded in the pull request (which controls were read, which could be changed).

## Phase 0: Decide and specify

No code. This phase needs the maintainer.

1. **Write an ADR** that supersedes ADR-0009 (for the shell and widgets, not the canvas) and ADR-0028. It records the decision to leave Iced in the phases below, the options (stay, fork Iced, Vizia, our own thin layer), and the triggers that would stop the work. The research snapshot already has the options. Mark it Proposed and ask the maintainer to decide. It merges only once it is Accepted.
2. **Write a specification**, `docs/design/ui-layer.md`, with sections for:
   - the runtime: events in, messages out, effects (what replaces `Task`);
   - layout;
   - focus and keyboard order (NFR-UX-01);
   - the accessibility tree: node IDs, roles, names, values, value text, steps, actions, refresh;
   - text input and IME;
   - the test harness and the control channel contract.

   Each open behaviour question (for example, how a held arrow key steps a knob for a screen reader) is listed and answered by the maintainer before the phase that needs it.
3. **Link the ADR and the spec from issue #683.** One issue covers all phases. Each phase's pull request says `Part of #683`, and the last one closes it.

Done when the ADR is Accepted and the spec's sections for phase 1 are Agreed.

## Phase 1: Own the seams, still inside Iced

The app still runs on `iced::application`. This phase moves our code off Iced's types so later phases only swap the bottom layer.

### 1.1 A toolkit-free geometry crate

1. Create `crates/loom-ui-core` (desktop only, no Iced). It holds `Point`, `Size`, `Rect`, `Vector`, and `Color`, with the operations the canvas uses. The types are our own design, written from plain geometry and not ported from Iced (ADR-0018). Each rule, such as which edges a rectangle holds, is stated in the crate and tested against itself. The input events arrive with step 1.2 and UI IDs with step 1.4, where each has a caller that shapes it.
2. Add conversions to and from the Iced types in one adapter module, `loom-canvas/src/iced_convert.rs`, not spread through the code. They are extension traits (`to_iced()`, `to_core()`), not `From` impls: Rust allows a `From` impl only in a crate that owns one of the two types, and `loom-ui-core` must not know Iced. The module is in `loom-canvas`, not `loom-app`, because the canvas widget needs it and cannot depend on the app. `loom-app` uses the same module.
3. Move `loom-canvas` to these types: `scene.rs`, `tile_text.rs`, `camera.rs`, `controls.rs`, `hover.rs`, `trace.rs`, `gpu.rs`. `widget.rs` stays an Iced widget. It uses the new types inside and converts only where Iced hands it a value or takes one.
4. Tests: the existing canvas tests pass unchanged except for imports and the name `Rect`. Add unit tests for the conversions.

### 1.2 Canvas input as a plain state machine

1. Add our own input events to `loom-ui-core`: `Event`, `PointerEvent`, `KeyEvent`, `Modifiers`, `Key`, `ScrollDelta`. Define them from what the canvas handlers match on, not from a toolkit's event list (ADR-0018).
2. Move the event handling in `loom-canvas/src/widget.rs` (drags, hover, double-click, key handling) into plain code in `loom-canvas/src/input`:
   - `CanvasInput` is the canvas's widget state, the part that lasts. `CanvasModel` is its view model: what the app shows this frame.
   - `CanvasInput::handle(&mut self, model, event, moment) -> Reaction`. The `Moment` carries the pointer's place, the canvas bounds, and the time. The pointer travels beside every event, not inside pointer events, because a key press can depend on it and only the driver knows when something above the canvas covers it.
   - A `Reaction` holds the `CanvasEvent`s to report, whether the input was used, and whether to redraw.
   - `CanvasInput::next_deadline` gives the time the canvas next needs to wake with no input (an arrow group closing, hover text appearing). It is a fact of the state, not of one event, as the [spec](../design/ui-layer.md) models deadlines (section 2). The driver schedules it.
   - `CanvasInput::pass` handles something the driver received that means nothing to the canvas, so the steps that run on every event still run.
   - No handler reads the clock.
3. The Iced widget becomes a thin adapter that converts events, calls `CanvasInput`, and applies the `Reaction`.
4. Move the existing widget tests to test the plain code directly. Keep the `Ui` harness tests as they are. Test the adapter's conversions on their own, since the plain tests no longer pass through them.

### 1.3 Canvas drawing without Iced's primitive

1. Give `gpu.rs` a plain API with no Iced types. `CanvasRenderer::new` takes a `wgpu` device and the target's format. `prepare` takes the device, the queue, a `CanvasFrame` of plain data, and a `Target` (the viewport's logical size and the pixel scale). `draw` takes a render pass. The host owns the pass and sets its viewport and scissor; the renderer never begins a pass and never presents. *Why a pass, not a texture view:* a custom element "draws into a `wgpu` render pass with a viewport the runtime gives it" ([spec](../design/ui-layer.md), section 9, rule 1), and a host that is not our window, such as a plugin's, also hands over a pass.
2. `loom-canvas` depends on `wgpu` itself, at the version Iced builds with, so the plain files name `wgpu` types with no Iced path. There is one copy of `wgpu` in the tree.
3. The Iced shader `Primitive` and `Pipeline` wrap the renderer, in `widget/primitive.rs`.
4. `CanvasInput::frame` builds the `CanvasFrame`, with a `FrameCache` the host keeps beside it, so the widget reads none of the canvas's state.
5. Tile text still goes through Iced's text renderer in this phase, behind a boundary in `painter.rs`. The canvas hands each piece to a `Painter` and asks a `TextMeasure` how large a piece of text is. A `Painter` takes text runs (`TextRun`: text, anchor, room, alignment, style, colour, clip), boxes (`Quad`), and layers. *Why more than text runs:* the choice list, the typed field, and hover text paint boxes in layers above the tile text, and two of them measure text to place a caret and size a box. The text is borrowed and no list is built. `CanvasInput::paint` is the entry point. `widget/painter.rs` is the Iced side.
6. `tests/purity.rs` holds the line: Iced is named only in `widget.rs`, files under `widget/`, `iced_convert.rs`, and files under `iced_convert/`.

### 1.4 The accessibility tree as a real module

**Done.** All four parts have landed; what is left needs a listening check or step 1.5 (Phase 1 done, below).

This turns the spike into supported code. It needs the phase 0 spec. UI IDs ([spec](../design/ui-layer.md), section 4) join `loom-ui-core` in this step, with the table of accessibility node IDs as their first user.

The step lands in four parts, the second as two pull requests:

1. UI IDs and the node ID table in `loom-ui-core`; `a11y.rs` split into the three files of task 1; the feature and the environment switches of task 2 removed. The tree covers what the spike covered, and is still rebuilt on a timer while a screen reader listens.
2. The canvas tree made correct, in two pull requests, because together they pass the size a review can hold:
   - each tile widget's role and real range, the arrow-key step, the undo group of task 4, task 5, and new bounds for every node when the window's scale factor changes;
   - canvas focus, tiles hidden below a panel that owns the input, and live values off screen.
3. The toolbar, the status and stats bars, the inspector, and the matrix tab, as three pull requests: the bars with the toolbar and the right panel's tabs, where the bounds of toolkit-drawn parts and `locate` by UI ID also land; the inspector; the matrix.
4. **Done.** The module browser and every overlay, and the tests of task 6 that cover every view, as three pull requests: the menus, help, and the question (4a); the module browser and Your patches (4b); Preferences, the licence chooser, and task 6's tests (4c). Until this part lands, a screen reader finds nothing in the window while a panel or a menu is open: what is below is hidden, as a pointer cannot reach it, and the panel itself has no nodes yet. This part closes that gap.

The tasks:

1. Split `a11y.rs` into:
   - `a11y/tree.rs`: builds an AccessKit `TreeUpdate` from `UiSnapshot` and the other view models. No platform code, so it builds and is tested on every OS.
   - `a11y/actions.rs`: turns AccessKit `ActionRequest`s into `Message`s.
   - `a11y/macos.rs`: the `SubclassingAdapter` attach code from the spike, kept only until phase 2 replaces it.
2. Remove the `a11y-spike` feature and the `LOOM_A11Y_*` environment switches. Use the value and name policy the spec chose.
3. Extend the view models so the tree covers the toolbar, status bar, inspector rows, module browser, and every overlay, not only tiles. Each gets an accessible name, role, value, and state in our own words, as ADR-0028 asked.
4. Undo: group screen reader adjustments into undo steps the same way arrow-key edits group (`docs/design/undo.md`).
5. Rebuild the tree after messages that change the snapshot, not on a timer. Send only changed nodes. Handle `deactivate_accessibility`.
6. Tests in `a11y/tests.rs`, **done in 4c**, over every view and overlay (`with_every_view`):
   - every interactive widget in every view has a node with a non-empty name, a role, and a place: `every_view_names_its_acting_parts_and_keeps_them_in_the_window`;
   - every node's bounds lie inside the window, cut by any node that scrolls it: the same test;
   - actions round-trip through `App::update` as one undo step: `an_overlay_edit_is_one_undo_step`, beside the canvas's `increment_turns_the_knob_and_can_be_undone`; patch details and preferences add none, as their specs say (`a_licence_from_the_chooser_adds_no_undo_step`);
   - the tree changes when the snapshot changes, and only then: `every_view_sends_nothing_when_nothing_changed`, `one_change_in_preferences_sends_only_what_changed`, and, with audio playing, `preferences_with_audio_playing_rebuilds_only_on_a_change`.

A known cost, measured in slice 2 with `--measure-tree` (release build, macOS): one build of the tree takes about 3 ms on a 300-module patch and 0.3 ms on `subtractive.ron`. A build is the UI snapshot, the widget IDs, the whole tree, and the comparison with what was sent; how the time splits between them is not measured. There is at most one build per turn of the event loop, so a knob drag builds the tree at up to the pointer's rate. Timers and watchers that change nothing the tree reports build nothing, and moving live values build it 4 times a second. Since the stats bar is reported, its load reading is one of those live values: with audio running, a patch with no meter on screen is still built at the live rate. On the 300-module patch that is about 21 builds in 8 seconds, and 73 frames against 35 with no screen reader. The cost begins when an assistive tool first asks for the tree. On macOS it then stays until the app quits, because `accesskit_macos` 0.27.2 never says that the tool left. Making one build cheaper is not part of step 1.4. The matrix's parts, measured another way: a release-mode test that opens a patch, opens the Matrix tab, and times 900 builds of the tree with `App::a11y_tree` (after 30 warm-up builds), so the display's rate cannot change the result. On a patch of 25 LFOs and 25 filters with 50 assignments (356 items with the tab open), one build takes 0.70 ms (median; 0.84 ms at the 95th percentile), against 0.28 ms with nothing reported for the matrix. This method times the snapshot, the widget IDs, and the whole tree, without the comparison with what was sent that `--measure-tree` includes, and in a test, not the live window, so its numbers are not comparable with the 3 ms above. The same way, on `subtractive.ron`: 0.061 ms with nothing open, and with an overlay open at most 0.088 ms, for module help's 27 items (a menu, the Recent list, and the question add under 0.005 ms). Slice 4b, the same way, with 300 generated patches in the library folder: the module browser open over every module and the 300 library items (366 items, each option with its own description) takes 0.36 ms a build (median; 0.37 ms at the 95th percentile), against 0.022 ms with it closed; Your patches with the 300 files (319 items) 0.28 ms, against 0.022 ms. Both are under the 1 ms line, so every row is reported. With no screen reader, `view()` takes 0.161 ms with the browser open (0.147 ms before 4b) and 0.119 ms with Your patches (0.105 ms): naming each row's container costs about 15 µs a frame once its ID's text is kept with the row, and 0.1 ms when it was written on every frame. Slice 4c, the same way, on a fake host with 8 output devices and 64 MIDI inputs: Preferences open (84 items) takes 0.12 ms a build (median; 0.14 ms at the 95th percentile), against 0.024 ms with it closed; the licence chooser over it (13 items) 0.044 ms. `view()` takes 0.033 ms with Preferences open, against 0.011 ms closed, since its model is built for each view; the poll key's hash of that model adds 0.016 ms to each poll while it is open, and nothing while it is closed.

### 1.5 AccessKit's macOS adapter

The spike found two gaps in `accesskit_macos` 0.27.2:

- it never sets the accessibility label from the node's name, so VoiceOver does not read a slider's name;
- it has no spoken value description beside a numeric value, so a slider can either be stepped or say its unit, not both.

1. Write each gap as a minimal reproduction: a small example program that shows the problem with VoiceOver, with no LoomDSP code. **Done:** `examples/accesskit-macos-repro/`, a crate outside the workspace on `winit` and `accesskit_winit`, with two programs, a script that reads the accessibility attributes, and the two issue reports as drafts. Both gaps were confirmed in the source of 0.27.2 and of AccessKit's `main` ([research note](../research/2026-11-accesskit-macos-gaps.md)). What VoiceOver says still needs a listening check with the reproduction.
2. Ask the maintainer before filing anything upstream. Filing an issue or a pull request with AccessKit is an outward action.
3. Once approved, propose the fix upstream. AccessKit is a dependency accepted under ADR-0006, so reading and patching its source is allowed.
4. If LoomDSP needs the fix before a release, use a `[patch.crates-io]` entry that points at a fork on a pinned commit. Allow that source in `deny.toml` with a comment linking the upstream pull request. Remove the patch as soon as a release contains the fix.

### Phase 1 done

The parity gates pass. VoiceOver reads names and values for every control on a tile and in the inspector, and changes them by keyboard. `loom-canvas` has no Iced types outside `widget.rs`, the files under `widget/` (the primitive wrapper and the Iced painter among them), and `iced_convert.rs`. `crates/loom-canvas/tests/purity.rs` checks it.

**Not yet done.** Steps 1.1 to 1.4 have landed. What is left:

- step 1.5: the two `accesskit_macos` gaps, waiting for Brylie's approval to file them upstream;
- Brylie's VoiceOver listening check: names and values on tiles and in the inspector, and changing them by keyboard; how a held arrow key feels (UI layer, section 7, rule 5); the module browser's highlighted row (rule 27); stepping the audio device list in Preferences (rule 15); and VO+Shift+M on a tile, which the macOS adapter does not pass on.

The purity gate holds: `crates/loom-canvas/tests/purity.rs` passes.


## Phase 2: Own the window and the event loop

The window and the loop become ours. Iced's widgets still draw the panels around the canvas, but through Iced's lower layers driven by our loop, as the `Ui` harness already does headlessly. This is the biggest single step. It removes `iced::application` and Iced's `winit` shell.

### 2.1 Our own runtime

1. **Done (2.1a):** the runtime in `crates/loom-ui`, generic over the app and the routing target, with the driver as a trait ([UI layer spec](../design/ui-layer.md), section 2, rule 9). Built from LoomDSP's own needs, not from Iced's shapes (section 2's decisions). It has:
   - `DriverEvent`: an element event with the pointer beside it, a window event (resize, scale factor, focus, close request, dropped file), a system redraw request, an animation frame, a frame shown, a wake-up, and a deadline reached. Element events stay `loom-ui-core`'s `Event`;
   - `Runtime::handle(event, now, driver) -> Next`: the message queue, one `update` per message with its effects right after it, messages from effects on the next turn, and `Next` (run again, frames, a deadline, or sleep);
   - `Effects` as plain values, each named, carried out by the runtime, the routing `Target`, or the `Driver` (section 2's effects table), and `Context` for `update` (the time, a waker, the focus, the window);
   - the redraw guarantee: a turn draws when a panel model changed, the target asks, an effect asks, or the system asks (section 2, rule 10). A request with no model change, such as a screen reader's Focus action on the canvas, asks with the draw-again effect. Not "a redraw after every batch": the bridge's target gives that for the Iced views until they move;
   - `Waker` and deadlines from state (`App::next_deadline`), so the loop sleeps until something is due;
   - `headless::Headless` and `headless::Harness`: a driver that keeps everything in memory, runs work inline and in order, and panics naming any dialog, question, or capture the test did not answer. Step 2.3's `Ui` stands on it;
   - `examples/countdown.rs`, a terminal driver with no window, and `tests/purity.rs` (only `loom-ui-core`, no window toolkit named, no clock read).
   The phase 2 acceptance criteria on effects were ticked at step 2.3, item 1, when the app's `Ui` harness came to run on this driver (part d of item 2, second pull request). The tests that already hold them: `headless/tests.rs` (`a_file_dialog_with_no_answer_panics_naming_it` and the two beside it), `runtime/tests.rs` (`one_message_gives_one_update_and_effect_messages_wait`, `work_runs_inline_and_its_messages_arrive_in_order`).
2. Port `App::update` to return `Effects` instead of `Task<Message>`, file by file, so the app keeps running on `iced::application` until 2.2 lands. Four pull requests: (a) the turn, the dialogs, and work; (b) focus and the clipboard; (c) scrolling and the capture; (d) the subscriptions mapped through the bridge, the `Ui` harness on the runtime's headless driver (with the offscreen capture of the effects table, which answers `NO_GPU` until then), the nested updates, and the outbox deleted.
   - **Done (2.1b, part a):** the app runs inside `loom_ui::Runtime`, and `crates/loom-app/src/iced_bridge.rs` is Iced as its driver and routing target, not a converter from effects to tasks. An Iced message enters through the runtime's waker and runs one turn; a waker used from another thread wakes Iced through one subscription. Files not yet ported pushed their tasks into the bridge's outbox, which went in part (d). `tests/purity.rs` lists the files that may still name Iced's `Task`, its widget operations, clipboard, exit, and screenshot, or `rfd`, and the list only shrinks. Tests apply a message with `App::send`, which answers dialogs from the fakes and runs work inline (rename of `app.update(m)` in its own commit). `loom-ui` gained `FocusTarget::Nothing` and `Context::new`, for the bridge and for tests that step one app.
   - **Done (2.1b, part b):** focus and the clipboard.
     - A text field's widget ID is made from its part's UI ID, and only that pair reads back as a field: `places::field_widget_id` and `places::field_of`.
     - The bridge asks Iced where focus is after a turn whose message may have moved it (the app sets a flag, `App::take_focus_question`) and after each focus effect. Every answer reaches the app as `Message::FocusMoved`, changed or not, as `TextFocus` did. `App::focus` is a copy for the view and the accessibility tree, and `update` checks that it equals `cx.focused()`.
     - Focusing a field, select-all, and taking focus away are focus effects. Copy and Cut write the clipboard, and Paste reads it.
     - **Kept until phase 3:** undo and redo, Tab between a view's fields, and the control channel's `type` still ask Iced first (`iced_bridge::after_focus_read`, `after_focus_part`; window questions since part d). A field can take focus with no message, or a key can come before the answer to a click, so only Iced's widgets know in time. `ui_test/tests.rs`, `undo_right_after_a_click_into_a_field_leaves_the_patch`, holds the undo case.
   - **Done (2.1b, part c):** scrolling and the capture are effects.
     - The module browser, Your patches, and the matrix ask for `Effects::scroll_into_view` with the UI ID of the row, or of the matrix's card and then its cell, instead of working out an offset. The bridge finds the part with `places::Collect`, which now also notes each part's nearest scrolling area, and moves that area just enough on both axes (`loom_ui_core::Rect::reveal`). Iced's scroll operation is named only in the bridge.
     - The browser keeps its own offset, moved by the same rule, because it places a library row's menu at the row and Iced does not report a scroll the app made. Your patches drops its offset.
     - The control channel's `screenshot` is `Effects::capture`, and `loom_ui::effect::Image` carries the scale it was drawn at.
     - `browser.rs` and `patches.rs` are off the purity list.
   - **Part (d), first pull request: the last Iced tasks and the outbox go.**
     - What only Iced's widgets know is a window question (`window_question.rs`): which part has focus (undo, redo, Tab, `type`), where a text or a named part is (`locate`, a screenshot of a part), and where every named part is (the accessibility tree). The bridge asks them after the turn and hands each answer back as a message, in order. `after_focus_read` and `after_focus_part` go.
     - Window events are routed before `update`, in the bridge and in `App::send`: each message an event becomes is its own update.
     - No nested updates. A menu item's Enter, a screen reader's action, and a control command post their messages for the next turn (`App::post`), each its own update, with a trailing message for what must come after them: a step's release (`a11y::Event::Applied`), or the command's reply (`Message::ControlReply`), which reads what the messages changed. `update_iced` became `apply`; the offscreen `--dump-ui` and `--screenshot` paths, which run no runtime, still call it directly.
     - The outbox and every `-> Task` in app code are gone. The bridge's start runs the macOS adapter's attach.
     - `tests/purity.rs` lists only `a11y/macos.rs` (step 2.2) and `ui_test.rs`, which comes off with the harness on the runtime, the second pull request.
   - **Part (d), second pull request: the `Ui` harness on the runtime.** Step 2.3, item 1, done there. Posting for the next turn also became an effect, `Effect::NextTurn`, which the runtime carries out itself (Brylie's choice, 2026-11-13). The bridge runs another turn when the runtime says messages wait, since no waker announces them.
   - **Dialogs take two turns.** Done in part (a), in `dialogs.rs`: Open, Save As (both sites), Save to library, and the credit question, which the save waits for. The unsaved changes on close were already a question inside the window. The crash recovery question is asked from a deadline due at once, once the window is open, over an empty patch; a failed restore shows its error in the window ([autosave](../design/autosave.md), The offer at start). Both are behaviour changes, listed in that pull request.
   - **The four `Task::done` call sites** go, each by its own reason (a fifth, `a11y.rs` 742, read focus back after taking it away, and went in part (b) as a focus effect to no element). The readers went in part (a). None queues on purpose to run after a commit or after the view changed: `browser.rs` 525 and `patches.rs` 237 are the test-only inline readers, which become work effects that the headless driver runs inline; `a11y/macos.rs` 98 answers "no window" and goes with that file at step 2.2; `control/run.rs` 72 answers "no window" for a screenshot and becomes the capture effect.
   - **The focus query** (`Message::TextFocus`) becomes a read of `cx.focused()` in the same turn. **Paste** becomes the clipboard read effect, still one turn later.
   - **Frames stop running `update`.** `LiveFrame` today calls `scene::update_live_values` (`lib.rs` 1750), which copies live modulated values into the canvas's values, and checks the audio input. With frames that run no update, that copy moves to a draw-time read of the live value table (section 3, rule 7). Until then, a deadline at display rate keeps the live positions moving. **Decided (2026-11-12):** the deadline, returned by `next_deadline` only while modulation moves, from item 3, when frames become `animating`; the draw-time read comes with rule 7 in phase 3, with the meters (spec section 3, rule 8).
3. Port subscriptions to event sources that wake the loop:
   - the control channel listener, the MIDI learn watcher, the input watcher, audio watch, and the telemetry monitor become watchers that take a `Waker` from `update`'s context. The app holds each watcher's handle in its state, and dropping the handle stops it without blocking (section 2, rule 7). **Done in the runtime:** the `Waker` and its wake function.
     - **Done (item 3, part a):** eight watchers run on the runtime's waker: the audio stats, the audio input, the lights, the telemetry monitor, MIDI learn, the MIDI port list, the control channel, and the bench report (`watcher.rs`, `watching.rs`). A `Watcher` handle stops its thread when dropped, with no join. Its `Outlet` holds a few messages at most and refuses more as full until the app takes one, at the top of `App::apply`, so a busy UI meets no backlog of stale readings, as with the bounded channels before. A ledger per kind of message records who sent each one, so a message from a replaced engine's watcher frees nothing on the new one. After each update, `App::sync_watchers` restarts only a watcher whose key (what it reads, hashed) changed. The handles that read one engine live on the session's engine, the rest on the `App`. Watchers start once the window is open (`Message::StartWatching`, from the bridge's start), so tests and the offscreen paths run none;
   - timers (frame meter, alerts, autosave) become deadlines the app returns from `next_deadline`, which the `winit` shell waits on with `ControlFlow::WaitUntil`. **Done in the runtime;** the app side waits for item 2;
   - `ControlFrame` becomes the after-draw effect, and `Frame` (measuring) and `LiveFrame` become `animating`.
     - **Done (item 3, part b):** the seven timers are deadlines kept in the app's state (`timers.rs`), started and stopped after each update under the conditions their subscriptions had, and `App::next_deadline` gives the earliest. The bridge wakes for a later deadline through one long-lived sleeper thread per window (`iced_bridge/sleeper.rs`). `LiveFrame`, and `Frame` while measuring, are deadlines at display rate, 16 ms, since Iced gives no refresh rate (decided 2026-11-14). `ControlFrame` is an after-draw message, and the bridge sends `Presented` on each of Iced's frames while one waits (`Runtime::waits_for_draw`), a frame early at most until the `winit` shell. Timers, like watchers, start once the window is open. `subscriptions.rs` keeps only the window's close request, the input listeners, and the accessibility layer's events, for step 2.2.

   While nothing is due, the loop sleeps (NFR-PERF-04, control channel rule 7).

   **Not met at today's baseline:** phase 2's idle criterion asks for no redraws in 10 idle seconds. On `main` at `085f217a`, `--measure-idle` on `subtractive.ron` draws 266 frames, 263 of them for meter messages that arrive while nothing plays. That is section 3, rule 7's work (live values outside `update`), not this step's.

### 2.2 The winit shell

Checked against the dependencies' source at the versions we pin, in the [research note](../research/2026-11-winit-shell-facts.md) (2026-11-13). Four pull requests. 2.2b, 2.2c, and 2.2d start after step 2.1b part (d), because the shell needs an app that gives no `Task`, no outbox, and subscriptions mapped to event sources.

- **2.2a:** this text, the research note, and the decisions below. Docs only.
- **2.2b:** the shell, with Iced still drawing everything, the canvas included. Same pixels, a new owner of the window.
- **2.2c:** our pass draws the canvas.
- **2.2d:** `accesskit_winit`. 2.2c and 2.2d do not depend on each other.

**Where the code lives** (spec section 1, the crate table and rules 2 and 6). The shell is in `loom-ui`, the one module there that names `winit` (`crates/loom-ui/tests/purity.rs`), and takes the app through the runtime's traits, never as `loom_app::App`. It names no Iced type. Everything that names Iced is in `loom-app/src/iced_bridge/`, as the routing and drawing part the spec already describes: building and updating the `UserInterface`, Iced's redraw work, its input method report, its cursor icon, and `present`. Two traits meet there. The runtime's `Target` stays as it is, free of any toolkit: the shell turns winit's events into `loom-ui-core` events for the runtime. The shell's module defines its own window-facing trait, `shell::Layers`. That module is the one place in `loom-ui` that names `winit` and `wgpu`, and `crates/loom-ui/tests/purity.rs` then names it. `Layers` takes winit's events as they are, so the bridge converts them with `iced_winit::conversion`, since only it needs Iced's events; it draws a frame (item 2); and it reports the cursor icon, the input method's cursor area, and when it next needs a redraw. `loom-app` implements `Layers` with the bridge. So `iced_winit` becomes a dependency of `loom-app`, and `winit` and `wgpu` of `loom-ui`.

1. Replace `iced::application` in `lib.rs::run` with the shell (2.2b). The shell, a `winit` 0.30 `ApplicationHandler`:
   - makes the window, the surface, and the device in `resumed`, and keeps them if `resumed` comes again (winit 0.30 has no `can_create_surfaces`; that is 0.31);
   - creates the window hidden, and shows it after the first frame;
   - chooses the surface as `iced_wgpu` does today, so the pixels stay the same: a format that is not sRGB (the workspace turns on Iced's `web-colors`), the clear colour in raw components, frame latency 1, and PostMultiplied alpha first. Iced's integration example chooses sRGB, which would change every pixel;
   - wraps its `EventLoopProxy` in the runtime's waker, waits with `ControlFlow::WaitUntil` for the earliest deadline the runtime and `Layers` give, and exits after the turn whose driver asked it to;
   - reads the window's monitor refresh rate (winit's `MonitorHandle::refresh_rate_millihertz`), again when the window moves to another monitor, and gives that period to the app for its display-rate deadlines (`timers.rs`). That removes the 16 ms fallback of item 3, part b, and its cap of about 60 frames a second on a faster display (decided 2026-11-14);
   - sends winit events to the runtime and to `Layers`, and redraws only when the runtime, `Layers`, or a resize asks. Until the panels leave Iced, every turn with an update asks (spec section 2, rule 10);
   - sets the cursor icon and the input method's cursor area that `Layers` reports, through winit;
   - shows file dialogs and questions as today, blocking and modal. Sheets on macOS (`rfd`'s `set_parent`) are a later pull request, since the runtime's dialogs already take two turns.

   The bridge, as the shell's `Layers`:
   - does the per-frame work `iced_winit` does in a private module: Iced's `RedrawRequested` event and the up to three updates after it, keeping their messages; `RedrawRequest::At` as a deadline for the shell; and the cursor icon and input method report, handed to the shell;
   - draws the input method's preedit text with an overlay of its own (decided 2026-11-13, spec section 8, rule 4). Iced's text field already turns winit's `Ime::Preedit` into the `InputMethod` that `UserInterface::update` reports: the cursor's area, the text, the selected range, and the text size. The bridge draws that text as an Iced layer on top of the window: a box at the cursor's area, kept inside the window, above every layer, at the reported size (or the field's size token), underlined, with the selected range marked. About 100 lines. If it needs more than that report, or grows past about 200 lines, it stops for a decision.
2. Draw each frame in this order. Iced cannot draw into our render pass: `iced_wgpu::Renderer::present` makes its own encoder and passes and submits them. So a frame is separate submissions on one queue, into the frame's texture view. `loom-ui` never names `loom-canvas` or Iced (spec section 1, rule 1), so the shell does not draw the app's layers itself. It hands `Layers::draw(&mut self, frame: &Frame)` a `Frame` (the device, the queue, the texture view, its format, the size, and the scale), and `loom-app` draws into it, in order:
   1. clear, in the theme's background;
   2. from 2.2c, the canvas with `CanvasRenderer`, with an encoder of its own, and the viewport and scissor `iced_wgpu` sets for it today (its bounds, from the `places` walk, times the scale, cut to the window);
   3. Iced's `present` with no clear colour, on the same view, for the rest of the window.

   Then the shell calls `pre_present_notify` and presents the frame. The three steps are one `loom-app` function that takes any texture view. `Layers::draw` calls it for the window. The capture effect and `--screenshot-window` call it with an offscreen texture of the surface's format and read it back. So from 2.2c `screenshot.rs::draw_window` stops using Iced's own screenshot call, which opens its own encoder and target and would leave the canvas out once Iced no longer draws it, and the capture no longer needs `iced::window::screenshot`.
3. Route events through one `UserInterface::update` per batch in the bridge, then the app's input listeners with each event's status, as `iced_winit` orders them today. The canvas keeps a stand-in widget in Iced's tree until phase 3 (2.2c): it takes the canvas's input and hands it to `CanvasInput`, and draws nothing. *Why:* a `UserInterface` has no public hit test; Iced decides inside its tree which layer gets the pointer, so the stand-in keeps overlays above the canvas exactly as today. Routing the canvas area outside Iced waits for our own layout (phase 3).
4. Delete `a11y/macos.rs` (2.2d). The tree goes through `accesskit_winit` 0.34 in the shell, created before the window is shown (it panics on a visible window), with its events through the proxy. On Linux it brings `accesskit_unix` and `zbus` 5, which fail `cargo deny check` on three advisories that only say the crates are unmaintained: RUSTSEC-2026-0370, -0371, and -0372. *Decided 2026-11-13:* ignore the three in `deny.toml`, each with its reason, so Linux has a screen reader from the start, and track their removal in an issue for when `accesskit_unix` moves to `zbus` 6 (spec section 7). 2.2d also rewrites the comment in the root `Cargo.toml` that turns off Iced's `linux-theme-detection`: the fixed theme is still its reason, and keeping `zbus` 5 out no longer is.
5. Keep the window size, title, minimum size, high-DPI behaviour, file drop, and close confirmation exactly as they are. List each in the pull request as checked by hand. Cmd+Q quits without asking, today and after, so unsaved work is lost with no question; that is a bug for an issue of its own (to be filed), not part of this step.

Gates for 2.2b and 2.2c, with main as the base:
- every test passes unedited;
- `--dump-ui` and `--screenshot-window` are byte-identical on the seven example patches;
- a control script with `--fake-audio` gives the same replies, and byte-identical target and rect images. The whole window differs only as two runs of main differ from each other;
- four runs each of `--measure-idle`, `--generate 300 --bench`, and `--generate 300 --measure` stay within the runs' spread;
- `--fake-audio --record`, then `loom-cli analyze`, passes the same checks.

2.2d's gate is VoiceOver: the tree attaches at start, and its actions work.

### 2.3 The headless harness on the same runtime

1. Rewrite `ui_test.rs` internals so `Ui` drives the phase 2.1 runtime, not Iced's runtime. Its public API (`Ui::new`, pointer and key methods, `output`, `input`, `control`, `text`, `exited`, the in-memory clipboard) does not change. The 15 files that use it should not need edits.
   - **Done (2.1b, part d, second pull request).** The `Ui` holds the app's own bridge, `Bridge<UiDriver>` (`iced_bridge/headless.rs`), and runs the Iced tasks the bridge returns on its view, so input routing, the turn, focus and scroll, window questions, and the focus answer run through the app's code, not a copy. `UiDriver` is `loom-ui`'s headless driver, with the fake dialogs in the crate's tests, so under `tests/` a dialog with no answer panics, naming it. A capture draws the view offscreen on wgpu and answers `NoGpu` on tiny-skia. One change to the API: the runtime owns the app, so tests reach it through `Ui::app()` and `Ui::app_mut()`, a mechanical rename in its own commit. `app_mut` stands on `loom_ui::Runtime::app_mut`, behind `loom-ui`'s `test-driver` feature, which only loom-app's test builds turn on (through its `test-ui` feature), so a real driver cannot reach it. The clipboard is the driver's, through `Ui::clipboard` and `Ui::set_clipboard`. `tests/purity.rs` lists only `a11y/macos.rs`.
2. Separate layout and hit-testing from drawing, so the harness needs no GPU for event tests. Text that Iced still draws keeps Iced's measure, so layout matches the drawn widths. Text that Parley draws, from step 2.4b on, is measured by Parley, which needs only its font and layout contexts on the CPU.
3. Drawing tests (`screenshot.rs`, its smoke test) render to an offscreen `wgpu` texture. Without a GPU adapter they skip with `NO_GPU`, as today. Iced's `tiny-skia` fallback goes away; nothing should depend on it once events need no renderer. If CI has no GPU, try a software `wgpu` adapter (such as llvmpipe through Vulkan, or WARP on Windows) and record whether it works.
4. Add an accessibility test driver to the harness: `Ui::a11y_tree()` returns the current tree, and `Ui::a11y_action(name, action)` performs an action by node name. Agents and tests can then check the tree the way a screen reader sees it.
   - **Done.** `crates/loom-app/src/ui_test/a11y.rs`: `A11yTree` reads the whole tree with the harness's own places, and finds a node by accessible name, by a path of names, or by UI ID. `Ui::a11y_action(target, Action)` and `Ui::a11y_set_value(target, value)` send the request as the adapter does, then draw a frame. The signatures are in the spec, section 10, rule 4, and a decision row dated 2026-11-14.

### 2.4 Text drawing for the canvas

1. **2.4a, done:** the text crate is Parley ([research note](../research/2026-11-text-crate-choice.md); spec section 8, rule 8). No `glyphon` release builds with our `wgpu` 27, and nothing like it takes Parley's glyph runs, so step 2.4b builds a glyph atlas of our own: rasterise with `swash`, pack with `etagere`, and draw instanced quads, about 680 lines in the parts the research note costs. Ask the maintainer if it grows well past that (stop points).
2. **2.4b, done:** draw tile text with Parley and that atlas, behind a `Painter` and a `TextMeasure` (phase 1.3), in place of the Iced ones. Ask Parley for `system-ui`. Keep one Parley layout per label, keyed by its text, size, and weight, beside the canvas's frame cache, since a layout is rebuilt for any change. Keep the level-of-detail rules in `docs/design/panels.md` (text close up, bars further out).
3. The canvas has its own Parley font context beside Iced's font system until phase 3 moves the panels. Ask for `system-ui` with the optical size axis at its default. Tile text changes width here (spec section 8, rule 6), so this step's gate is: the `--screenshot-window` pixel diff is identical outside the bounds of the moved text (masked from the text rectangles the scene or the view lays out), every `locate` answer is unchanged, the pixels inside those bounds may differ, and the pull request records the change in width per label (least, greatest, and median). That screenshot is then the new baseline, and the pixel-identical gate resumes from 2.4b on. The other parity gates are unchanged.
4. **Measured at 2.4b,** on macOS, against `e04c5bde`:
   - The gate (`text_gate`, an ignored test in `crates/loom-app/src/screenshot/text_gate_tests.rs`): on all seven example patches, no pixel differs outside the moved text, inside or outside the canvas. `--dump-ui`, plain and with a selection, is byte-identical for each patch, so every tile, port, and control bound that `locate` answers from is unchanged. Tile layout and hit tests never measured text: they use `scene::text_width`'s estimate.
   - Widths, 424 tile labels: -1.7% to +5.3%, median +3.4%. One label now leaves its box: "Out +90°" in `modulation.ron`, by 0.27 world units of 37 (#120).
   - The first layout, with the font scan: about 80 ms.
   - `--generate 300 --bench --bench-seconds 10`, four runs each, base then new: CPU 19.2% to 25.8% before, 16.6% to 19.1% after; worst frame 22 to 28 ms before, 23 to 30 ms after; 60 fps both (one base run at 72, as the display's rate moved). Glyph instances upload on frames where the camera moves, as Iced's text did; scene geometry still uploads once.
   - `--measure-idle` on `subtractive.ron`, two runs each: CPU 7.4% and 7.8% before, 5.3% and 5.6% after; frame causes unchanged (meters, #730); one text upload per run.
   - The atlas: about 840 lines with its shader, 670 without comments.

### 2.5 The agent tools on the new shell

1. `--control` and `--control-script`: the listener thread wakes the loop through the proxy. `locate` finds targets from our layout data and the view models, not from an Iced widget `Operation`. The JSON protocol does not change.
2. `screenshot` (the command) and `--screenshot-window`: draw the same frame offscreen.
3. `--dump-ui`: unchanged, since it comes from `ui_snapshot`.
4. `scripts/bundle-macos.sh` and desktop automation: now see a real accessibility tree. Update `docs/design/control-channel.md`, which still says the window has none.
5. Run every command listed in the Commands section of `AGENTS.md` and record that each works.

### Phase 2 done

The parity gates pass. `iced::application`, Iced's `winit` shell, and `iced_runtime`'s task runner are no longer used. Screen readers work on macOS, and the tree is exposed on Windows and Linux (checked with Narrator and Orca if a machine is at hand; otherwise listed as not checked).

## Phase 3: Our own widgets, one view at a time

Now replace Iced's widgets with a small widget layer of our own, view by view. Iced's `UserInterface` keeps drawing the views not yet moved.

### 3.1 The widget layer

1. Create `crates/loom-ui` with:
   - **Elements:** a tree built each frame from view models, keyed by stable IDs (ADR-0021 keyed identity).
   - **Layout:** row, column, stack, padding, alignment, fixed and fill sizes. Use `taffy` † if a flexbox engine saves real work; otherwise a small layout pass of our own. Decide in the spec.
   - **Drawing:** our instanced rounded-rectangle pipeline (the canvas already has one in `canvas.wgsl`) plus the glyph renderer from 2.4. Clipping by scissor rectangles.
   - **Hit-testing and focus:** a focus order, focus rings, Tab and Shift+Tab, and focus that the accessibility tree reports.
   - **Accessibility:** every widget gives its AccessKit node from the same element, so the tree and the screen cannot disagree.
   - **Style:** the tokens in `style.rs` (text sizes, button roles, palette) carry over unchanged. The test that forbids inline `.size(N)` keeps working.
   - **Slider steps:** our slider has a drag step apart from its arrow-key step, so dragging stays fine while the arrow keys move further. The inspector's slider then steps 1% of travel on an arrow key and on a screen reader's Increment, 0.1% with Shift, as the tile's knob does: Brylie's choice of 2026-11-11, deferred until here because Iced's slider shares one step between both ([UI layer spec](../design/ui-layer.md), the decisions row on how an inspector slider steps).
2. Stacking follows the layers the user sees, not which toolkit draws them. While both toolkits draw, keep today's order: canvas, then panels (toolbar, inspector, stats bar), then overlays, then the question on top. A converted toolbar must not cover an Iced question. So draw and route by layer: each layer is drawn by its toolkit in turn, and an event goes to the topmost layer first, then down to lower layers only if it is not taken. Converting the overlays early (row 3 below) keeps this simple, because the top layers then belong to `loom-ui`. A test checks that the question takes a click over the toolbar at every step.

### 3.2 The order of views

Move the simplest views first. One pull request per row, each passing the parity gates.

| Order | View | Widgets it needs |
|---|---|---|
| 1 | Stats bar, status messages | text, container, tooltip |
| 2 | Toolbar | button (four roles), toggler, tooltip, space, rule |
| 3 | Overlays: question, probe menu, recent, licence chooser | container, opaque, pin, button, radio, scrolling list |
| 4 | Inspector and matrix | slider, checkbox, radio, pick_list, number entry, scrollable |
| 5 | Module browser | text_input with focus, scrollable list, keyboard navigation |
| 6 | Help pages | markdown: parse with `pulldown-cmark` †, lay out with our text |
| 7 | Patch info | multi-line text editor |

### 3.3 Text input and editing

1. Single-line input (browser search, number entry, names) builds on Parley's `PlainEditor`, with Parley's `complex-scripts` feature for dictionary word breaking in Thai, Lao, Chinese, and Japanese (off for the canvas, which never moves by word), behind a field type of our own: cursor, selection, word movement, and clipboard come from it, and undo inside the field is ours, by snapshots of text and selection (about 80 lines).
2. IME: send `winit` IME events to the focused field through `PlainEditor`, and report `ime_cursor_area` to the window so the candidate window appears in the right place. `winit` ends a composition with an empty `Preedit` and then a `Commit` that carries the final text, so the mapping is:
   - `Preedit` with text: `set_compose(text, cursor)`.
   - `Preedit` with empty text: `clear_compose()`. This drops only the preedit text, never committed text.
   - `Commit(text)`: insert `text` at the selection (`insert_or_replace_selection`). `finish_compose` is not used for it: it would keep the preedit as text, and the commit would then insert it a second time.
   - Focus leaving the field while composing: `finish_compose()`, so what the user saw stays.

   A test sends the sequence `Preedit("にほ")`, `Preedit("")`, `Commit("日本")` and checks the field holds "日本" once, with the caret after it. Also test with a Japanese or Chinese input method on macOS and record the result.
3. The multi-line editor (patch info authors) comes last. If it costs too much, keep it on Iced's `text_editor` and finish phase 3 without it. Phase 4 then waits until it is done.
4. Clipboard: use a permissive clipboard crate such as `arboard` †, behind the effect from 2.1, so the harness keeps its in-memory clipboard.

Each view's move changes its text widths once (spec section 8, rule 6). The view's step uses the gate of step 2.4b, item 3, masked to that view's text, and its screenshot becomes the new baseline.

### Phase 3 done

No view uses an Iced widget. The parity gates pass. The VoiceOver check covers the toolbar, inspector, browser, and every overlay, with keyboard focus that the screen reader follows.

## Phase 4: Remove Iced

1. Remove `iced`, `iced_wgpu`, and `iced_runtime` from `Cargo.toml` files. Depend on `winit`, `wgpu`, Parley, `swash`, `accesskit`, and `accesskit_winit` directly. With Iced gone, `cosmic-text` and `cryoglyph` leave the tree, and the canvas and the panels share one Parley font context. Once our `wgpu` is free to move, weigh Vello against our glyph atlas.
2. Run `cargo deny check` and `cargo tree -d`, and remove duplicates.
3. Update the documents:
   - `AGENTS.md` (App and Canvas paragraphs, commands);
   - `docs/dev/architecture.md`;
   - `docs/design/visual-system.md` and `docs/design/control-channel.md`;
   - the `ui_test.rs` module comment;
   - OQ-13 and OQ-14 in `docs/product/open-questions.md`. OQ-14 closes; OQ-13 now depends on our shell, not Iced.
4. Delete the text gate tool (`crates/loom-app/src/screenshot/text_gate_tests.rs`): it measures with Iced's paragraph.
5. Write an explainer in `docs/learn/` on how the UI layer works: the runtime, effects, layout, and the accessibility tree. It is the documentation Iced did not have.

## Stop points

Each phase is worth keeping on its own. The maintainer can stop after any of them.

- **After phase 1:** accessibility on macOS through the spike's adapter, and a canvas free of Iced types. Still on Iced.
- **After phase 2:** our own window and loop, accessibility on all three platforms, and a shell that can be adapted for plugin embedding. Iced is only a widget library.
- **After phase 3 without the multi-line editor:** Iced is used for one widget.

Stop and ask the maintainer if any of these happen:

- a parity gate fails and the fix is not clear;
- IME or text editing needs more than a few hundred lines on top of Parley, or the glyph atlas grows past 1,000 lines (the research note estimates about 680);
- an AccessKit gap blocks a phase and upstream has not answered;
- the idle CPU or frame rate gets worse and cannot be recovered in the same phase.

## Out of scope

- Plugin window embedding (OQ-13). The shell should not block it, but this plan does not build it.
- The web target (ADR-0026).
- Visual redesign. Every view looks the same after each phase.
- Any change to the engine crates. `loom-core`, `loom-dsp`, `loom-modules`, `loom-patch`, and `loom-rt` stay as they are and never depend on UI crates.

## Unverified facts

Entries marked † were not checked against source or release notes when this plan was written. Iced's integration example is checked ([research note](../research/2026-11-winit-shell-facts.md)), and step 2.2 is written from it. Check each before relying on it: `taffy`, `pulldown-cmark`, and `arboard`, including their licences and the versions that match `wgpu` 27 and `winit` 0.30.

Checked in step 2.4a ([research note](../research/2026-11-text-crate-choice.md)): `glyphon` (MIT OR Apache-2.0 OR Zlib; no release for `wgpu` 27), and the spec's two Parley claims in section 8, rule 8. Parley's AccessKit text properties existed up to 0.11.1 and were removed in 0.12.0. Its editor does track IME composition.
