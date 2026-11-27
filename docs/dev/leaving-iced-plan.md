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

The app must build, pass `mise run check`, and behave the same for a user after every phase, apart from a change that is decided and listed in its pull request ([spec design goal 5](../design/ui-layer.md#design-goals)). Each phase is one pull request, or a short series. No phase leaves the app half on one stack and half broken.

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

Run all of these at the end of each phase and list the results in the pull request. They catch change no one meant, not change that was decided: a deliberate move away from today's look or behaviour is allowed once it is decided and listed in its pull request ([spec design goal 5](../design/ui-layer.md#design-goals)).

- `mise run check`, and CI's 80% coverage gate.
- All `loom-app` tests pass with no change to what they assert. Changes to harness internals are allowed. Changes to expected values need a reason in the pull request.
- The control channel examples in `docs/design/control-channel.md` give the same replies, run through `--control-script`. A reply that a decided change alters is listed in the pull request, with the change and the new reply; every other reply stays the same.
- `--screenshot-window` for `examples/patches/subtractive.ron` with `--select 4`, before and after, compared by eye and with a pixel difference. Small differences in text anti-aliasing are fine; layout shifts are not, except where a decided change moves something or changes how it looks: the pull request names it, shows the before and after, and the rest of the window still matches.
- `--measure-idle --bench-seconds 10` on `subtractive.ron`: CPU and redraw counts no worse than before (NFR-PERF-04).
- `--generate 300 --bench`: frame rate no worse than before.
- From phase 1 on: a VoiceOver check on macOS, recorded in the pull request (which controls were read, which could be changed).
- In phase 3, **each view's layout changes once, when it moves.** Our layout follows flexbox's defaults, not Iced's ([spec](../design/ui-layer.md), section 5, rule 5), so a view's sizes and places may move by a few pixels when it moves to our widgets. At that step, the `--screenshot-window` pixel diff is identical outside the moved view's bounds, except where the same pull request lists a decided change, and may differ inside them; every `locate` answer is recorded before and after, and the pull request lists each one that changed, with its old and new rectangle; and a changed expected value in a test is listed with its reason. That screenshot is then the new baseline, and the identical gate resumes from that step on. *Why:* a layout engine that is not Iced's places things differently, and a gate per view keeps each move visible and small, as the text gate does for text widths (spec section 8, rule 6).

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
- it reports no rows for a grid or a table, so the matrix stays a list to a screen reader while its keys are a grid's (plan 3.2, row 4e, commit 3).

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
   - **Done (2.1b, part b):** focus and the clipboard. A screen reader's canvas Focus action asks for a redraw through an effect, so it can move focus on its own (section 7, rule 20).
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

   **Not met at today's baseline:** phase 2's idle criterion asks for no redraws in 10 idle seconds, with the transport stopped or no light it drives on screen ([panels, Redraw](../design/panels.md#redraw)). On `main` at `085f217a`, `--measure-idle` on `subtractive.ron` drew 266 frames, 263 of them for meter messages that arrive while nothing plays. Since [#730](https://github.com/brylie/LoomDSP/issues/730), a scope on a still wave wakes nothing, and lights count apart from meters. On 2026-11-16, `--fake-audio --measure-idle --bench-seconds 6` on `subtractive.ron` draws 69 frames: `meters:5`, all in the first second, and `lights:56`, from the running transport's Beat light, which is a real change. With its transport stopped it draws 16 to 18 frames, none for meters or lights after the first second, and about one a second of cause `other`, which still stands between it and the criterion. Draw-time live values are still section 3, rule 7's work.

### 2.2 The winit shell

Checked against the dependencies' source at the versions we pin, in the [research note](../research/2026-11-winit-shell-facts.md) (2026-11-13). Four pull requests. 2.2b, 2.2c, and 2.2d start after step 2.1b part (d), because the shell needs an app that gives no `Task`, no outbox, and subscriptions mapped to event sources.

- **2.2a:** this text, the research note, and the decisions below. Docs only.
- **2.2b:** the shell, with Iced still drawing everything, the canvas included. Same pixels, a new owner of the window. Two pull requests, so the first does not wait for step 2.1b part (d):
  - **2.2b-i, done:** `loom_ui::shell` behind a `shell` feature, on by default: the `winit` handler, the GPU surface and capture (`shell/gpu.rs`), winit's events as the runtime's (`shell/events.rs`), and the redraw rule with no window in it (`shell/pacing.rs`). `Layers` takes winit's events, draws a frame, and reports the cursor, the input method, and its redraws; each call lends it the app, the target, and a waker (`shell::Parts`). A second trait, `shell::Services`, gives the clipboard and dialogs the shell does not own, so `loom-ui` needs no clipboard or dialog crate. `Options::first` holds the first turn's messages, since there is no waker before the window exists. `examples/window.rs` runs a countdown in a window with no LoomDSP crate, and `mise run ui-no-toolkit` builds and tests `loom-ui` without the shell.
  - **2.2b-ii:** `loom-app`'s `Layers` and `Services` (the bridge), `lib.rs::run` switched over, the preedit overlay, and the gates and checks by hand below. Two pull requests, so the switch-over has a review of its own:
    - **(i), done:** two parts the window's layers will share with the `Ui` harness, with no change in behaviour. `subscriptions::Listeners` gives what the app's subscriptions send as plain calls, taken as the app stood before a batch of events, and a test checks it against Iced's subscriptions for every listener. `iced_bridge/tasks.rs` runs an Iced task with no Iced runtime: widget operations, outputs, and the clipboard go to a `Sink`, and a task that waits or asks for anything else stops and says why. The harness and the control channel's pointer use both.
    - **(ii), done:** `loom-ui`'s `Layers::open` (the window and the GPU adapter, which `iced_wgpu::Engine::new` and Iced's clipboard need), `Layers::report` with the app's parts, a request in `Report` to deliver one message alone (the focus answer, as `Runtime::deliver` does in the bridge today), and the display's frame period on `WindowState`; then the window's `Layers`, `Services`, `run` on the shell, and the preedit overlay. As built: the alone request is `Report::alone` with a `Layers::alone` hook, since the target must take the focus answer between the queued turn and `FocusMoved`; `Report::cursor` is a `Pointer` that can hide, as Iced's could; `Frame::capture` marks a capture, which presents the last frame again, as Iced's screenshot did, and waits for the first frame to be shown. The app's frames follow the monitor's rate, so the cap of about 60 a second is gone.
- **2.2c, done:** our pass draws the canvas. The frame function is `loom-app/src/iced_bridge/paint.rs`: clear, then the canvas with `CanvasRenderer` in one encoder, then Iced's `present` with no clear. The window, its captures, `--screenshot-window`, `--screenshot`, and the harness's captures all draw through it; offscreen frames use a device the app makes (`offscreen.rs`), since the canvas and Iced's renderer must share it. The canvas widget is a stand-in that draws nothing and leaves its frame in `App::canvas_drawn`, which every draw empties first and takes after. The cable dragged across the window stays an Iced shader primitive, in a later layer. Measured on macOS against `833258dc`:
  - `--screenshot-window` (plain and `--select 4`), `--screenshot`, and `--dump-ui` (plain and `--frames 4800`) on the nine example patches: byte-identical, 46 of 46. The canvas's top edge falls between pixels (y = 39.9), and its background fill blends there exactly as before.
  - The control script with `--fake-audio`, two rounds per build: every reply the same, `locate` included in the middle of a pan, before the release. `target` and `rect` captures byte-identical. The whole window differs from main only in cells where two runs of main differ from each other.
  - `--fake-audio --record` and `loom-cli analyze` on `sine.ron` and `subtractive.ron` with a played note: the same results on both builds. The 100 ms DC window check fails on both, by the same amount, at the note's start.
  - `--generate 300 --bench` and `--measure`: 120 fps on both, worst frames in the same range. `--measure` CPU, six alternating pairs on a busy machine: 21.7% mean on main (19.7 to 24.6), 24.4% on this branch (19.7 to 28.1), higher in five of six pairs. The frame adds one queue submission (the clear and the canvas) beside Iced's. `--measure-idle`: 26 to 28 fps on both, scene geometry uploaded once.
- **2.2d, done:** `accesskit_winit`, in the shell, on every platform. The app hands the shell the tree as an effect and hears the screen reader as `App::accessibility_event`; `a11y/macos.rs` is gone. 2.2c and 2.2d do not depend on each other.

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
   2. from 2.2c, the canvas with `CanvasRenderer`, with an encoder of its own, and the viewport and scissor `iced_wgpu` sets for it today (its bounds times the scale, cut to the window). The canvas draws its own background first: it is `#1C1E23`, not the theme's `#22242A`, so the clear does not cover it, and Iced drew it as a box under the canvas until 2.2c;
   3. Iced's `present` with no clear colour, on the same view, for the rest of the window.

   This order matches what Iced drew only because nothing in the window's base layer draws over the canvas. Iced draws each layer's boxes before its shader primitives and its text after them, and the canvas is in the base layer: the first child of the window's stack. Everything that shows over the canvas is in a later layer. Until phase 3 that is a rule (spec section 9, rule 8).

   Then the shell calls `pre_present_notify` and presents the frame. The three steps are one `loom-app` function that takes any texture view. `Layers::draw` calls it for the window. The capture effect and `--screenshot-window` call it with an offscreen texture of the surface's format and read it back. So from 2.2c `screenshot.rs::draw_window` stops using Iced's own screenshot call, which opens its own encoder and target and would leave the canvas out once Iced no longer draws it, and the capture no longer needs `iced::window::screenshot`.
3. Route events through one `UserInterface::update` per batch in the bridge, then the app's input listeners with each event's status, as `iced_winit` orders them today. The canvas keeps a stand-in widget in Iced's tree until phase 3 (2.2c): it takes the canvas's input and hands it to `CanvasInput`, and draws nothing. Its draw call builds the canvas's frame from the state it keeps in Iced's tree and leaves it in a slot the app lends it (`loom_canvas::CanvasSlot`), which the frame function takes. Its widget operations do not change: `locate` reads the canvas's bounds and live camera from them, and the app's own copy of the camera lags during a pan. *Why:* a `UserInterface` has no public hit test; Iced decides inside its tree which layer gets the pointer, so the stand-in keeps overlays above the canvas exactly as today. Routing the canvas area outside Iced waits for our own layout (phase 3).
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
   - **Done (items 2 and 3).** The app's renderer type, `crate::Renderer`, is Iced's fallback renderer over `iced_wgpu` and `Measure` (`iced_bridge/measure.rs`), and Iced's `tiny-skia` feature is off.
     - `Measure` uses the same text `Paragraph` and `Editor` as `iced_wgpu`: cosmic-text over Iced's one font system, on the CPU. So layout on it is the layout the window draws, and the test `measure_and_wgpu_lay_out_alike` checks every named part and the canvas, with a module selected and with the browser open. The canvas measures with Parley, as before. Every draw call does nothing.
     - The harness lays out on `Measure` always. A capture makes a wgpu renderer on its first use (`iced_bridge/offscreen.rs`, shared with `--screenshot-window`), runs a redraw update before drawing, as the window does before each frame (Iced 0.14 draws a newly built button as disabled until one; #460, #776), and answers `NoGpu` with no adapter.
     - The Linux Test job installs Mesa's software Vulkan (`mesa-vulkan-drivers`, lavapipe on llvmpipe), as the coverage job already did, and `LOOM_REQUIRE_GPU=1` now holds on every test job. The result and the job's time before and after are in the pull request.
     - *Note:* while the harness draws a frame, Iced's fallback renderer logs a warning that the canvas's shader primitive "is not supported with this renderer". That is the measure renderer skipping it, as intended, and is not an error.
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
   - `--measure-idle` on `subtractive.ron`, two runs each: CPU 7.4% and 7.8% before, 5.3% and 5.6% after; frame causes unchanged (meters, before #730 split lights from them and stopped a still scope waking the UI); one text upload per run.
   - The atlas: about 840 lines with its shader, 670 without comments.

### 2.5 The agent tools on the new shell

1. `--control` and `--control-script`: the listener thread wakes the loop through the proxy. `locate` finds targets from our layout data and the view models, not from an Iced widget `Operation`. The JSON protocol does not change.
   - **Done, the wake.** The listener runs on the control channel's watcher thread and hands each command to the watcher's outlet, which wakes the loop through the runtime's waker: the event loop's proxy under the shell (spec section 2, rule 7). The thread and channel from Iced's `futures` in between are gone, so `control/` names no `iced::futures`.
   - **`locate`: from our data where that data exists; the rest moves with phase 3** (spec section 10, rule 6, ruling of 2026-11-15). A module, a control, a port, a light, a meter, and a badge come from the scene and the camera, and a canvas UI ID from the accessibility tree. The canvas's place and live camera, the parts Iced draws, and text in the window still come from the `Find` window question until each view moves (step 3.2). No reply changed.
2. `screenshot` (the command) and `--screenshot-window`: draw the same frame offscreen.
   - Done in step 2.2c, which draws the window with a frame function of our own.
3. `--dump-ui`: unchanged, since it comes from `ui_snapshot`.
   - **Checked.** Plain and with `--select 4`, on `subtractive.ron`, the output is byte-identical to `main` before this step.
4. `scripts/bundle-macos.sh` and desktop automation: now see a real accessibility tree. Update `docs/design/control-channel.md`, which still says the window has none.
   - **Done.** control-channel.md says the window exposes the tree on macOS, Windows, and Linux, and that finding, reading, and acting on a control through it is checked on macOS only. The script needs no change, since the adapter needs nothing in `Info.plist`; its header says what automation finds. Checked on the bundled app with `examples/accesskit-macos-repro/inspect.swift`.
5. Run every command listed in the Commands section of `AGENTS.md` and record that each works.
   - **Done, on macOS, 2026-11-15, after step 2.2c.** Every command ran and exited 0:
     - `mise run check` (which runs the coverage step), `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`, `cargo deny check`, `typos`, `cargo bench -p loom-modules`, and `mise run help-pages` (no change to `docs/modules`);
     - the two `LOOM_UPDATE_REFERENCES=1` runs. They are deterministic here: two runs write the same files. Three committed renders (`dynamics_compress`, `dynamics_expand`, `dynamics_limit`) differ from this machine's renders within the test's 1e-3 tolerance, and the reference test passes against them. The committed files were kept;
     - `loom check`, `loom render`, and `loom analyze` on the recorded WAV;
     - `loom midi-port "Loom Pad"`, which opened the port, took the typed lines, and ended with its input;
     - every `loom-app` line:
       - the window, ended by a script that quits;
       - `--bench`: 118 fps with 300 modules;
       - `--measure`;
       - `--measure-idle`: frames only from meters, as #730 said before its fix (since #730, a still scope draws nothing, and a running transport's Beat light counts as lights);
       - `--measure-tree`: 20 tree builds, 1.7 ms each on average;
       - `--fake-audio --record`;
       - `--preferences` with `--device MOTU --buffer 128`: no MOTU device here, so the app starts on the default output;
       - the control socket: `set` answers 800, and `audio` answers its `--fake-audio` error;
       - `--screenshot` with `--zoom`, `--scale`, `--focus`, `--pointer`, and `--wire`;
       - `--screenshot-window --select`;
       - `--dump-ui` with `--select` and with `--frames`;
       - `--test-modules`;
     - `scripts/bundle-macos.sh`.
   - Two notes from the run:
     - `--control` needs a short socket path. The operating system's limit is about 100 bytes, and a longer path is refused with that error (control-channel rule 13).
     - A release build warns that `iced_bridge::measure::Measure::new` is never used, since only tests call it. Clippy's `--all-targets` run does not see it.
   - The pan check after 2.2c: in the running app, through `--control-script`, `locate` on module 4 moved by (+40, +25) during a middle-button pan of (+40, +25), before the release, and stayed there after it. That matches the harness test `locate_follows_a_pan_before_release`.

### Phase 2 done

The parity gates pass. `iced::application`, Iced's `winit` shell, and `iced_runtime`'s task runner are no longer used. Screen readers work on macOS, and the tree is exposed on Windows and Linux (checked with Narrator and Orca if a machine is at hand; otherwise listed as not checked).

**Done, 2026-11-15, except Brylie's VoiceOver listening check.**
- **The parity gates:** each step's pull request records its gate, and this step's are above. For example, `--dump-ui` and `locate` are byte-identical before and after 2.5.
- **No Iced application or shell:**
  - `rg 'iced::application|iced::daemon|iced::run\b|iced::winit' crates` finds nothing. The app starts through `loom_ui::shell::run` (`loom-app/src/lib.rs`).
  - `iced_winit` is named only in `iced_bridge/window.rs`, for its event and pointer conversions and its clipboard, not its shell.
  - `iced_runtime` is used for `UserInterface`, which still lays out and draws the views until phase 3. It also turns an Iced `Task` into a stream (`task::into_stream`, `task::widget`) that our runner in `iced_bridge/tasks.rs` drives. Nothing runs Iced's own runtime or its event loop.
- **Screen readers:**
  - macOS: VoiceOver's tree and actions were checked by script (step 2.2d's gate, again here on the bundled app). Brylie's listening check with VoiceOver is still to do, by hand.
  - Windows and Linux: the tree is exposed through `accesskit_winit`. Narrator and Orca are not checked, since no machine was at hand.

## Phase 3: Our own widgets, one view at a time

Now replace Iced's widgets with a small widget layer of our own, view by view. Iced's `UserInterface` keeps drawing the views not yet moved.

### 3.1 The widget layer

1. Create `crates/loom-ui` with:
   - **Elements:** a tree built each frame from view models, keyed by stable IDs (ADR-0021 keyed identity).
   - **Layout:** row, column, stack, padding, alignment, fixed and fill sizes, and a wrapping row (the tag chips). Decided 2026-11-14: `taffy`, with flexbox's defaults and its rounding to whole logical pixels ([spec](../design/ui-layer.md), section 5, rules 5 and 6).

     **Done (first part):** elements keyed by UI ID, layout of one panel, and hit-testing (`crates/loom-ui/src/element.rs`, `layout.rs`, `hit.rs`, and `examples/layout.rs`), with `TextStyle`, `Weight`, and `TextMeasure` moved to `loom-ui-core` so layout measures text with Parley's `CanvasText` (now `Typesetter`). Nothing in the app uses them yet.

     **Done (second part):** panels compose (`crates/loom-ui/src/window.rs`, [spec](../design/ui-layer.md), section 5, rule 8): a panel's slot holds another panel, the window is laid out from the roots down when the driver calls `Runtime::lay_out`, each panel keeps its own layout, and `Runtime::bounds` and `Runtime::hit` read the composed window. Still unused by the app. Next: drawing panels.
   - **Drawing:** our instanced rounded-rectangle pipeline (the canvas already has one in `canvas.wgsl`) plus the glyph renderer from 2.4. Clipping by scissor rectangles.

     **Done (first part):** the canvas's painting moved into `loom-ui` as `loom_ui::paint`, with nothing changed in what it draws: the `Painter` types, the `Typesetter` (once `CanvasText`), the paint list, the glyph atlas, the plan of draw steps, and `PaintRenderer`, behind a new `gpu` feature that the shell turns on. `paint.wgsl` holds the globals, the shape stage, and the glyph stage once, and the canvas builds its module from it followed by `canvas.wgsl`; a test validates the two together, and one validates `paint.wgsl` alone. The canvas takes `loom-ui` with `gpu` and without `shell`, which `crates/loom-canvas/tests/purity.rs` checks with `cargo tree`.

     **Done (second part):** panels draw ([spec](../design/ui-layer.md), section 5, rule 9). Elements set a background, a border that takes room, a radius, a rectangular clip, and a text colour; `Runtime::paint` walks one layer's panels in draw order into a `TextPainter`, with `PanelStyle` for the text colour the app gives. Hit-testing clips as drawing does, and a panel is cut to its slot. The shell lends the panels to the layers (`Parts::panels`), so a layer lays out and paints with its own typesetter; `examples/window.rs` draws its card that way. A headless test paints a panel on the GPU and checks its fill, border, corner, text, and clip. The app paints no panel yet. Next: stacking by layer (item 2), then row 1.
   - **Hit-testing and focus:** a focus order, focus rings, Tab and Shift+Tab, and focus that the accessibility tree reports.

     **Done:** `Element::focusable` makes a keyed element a focus stop. The runtime keeps the focus among its own elements, in reading order (spec section 6, rule 2, as decided 2026-11-16), and moves it with Tab and Shift+Tab, which a focused element passes on with Ctrl held. A router reads `Runtime::focus_stops` and `Runtime::focus_step`, which says `PastEnd` past either end, and calls `Runtime::focus_at` for a press its layer took, which shows no ring. The ring is data (`Runtime::focus_ring`, and `RingStyle`, which the app fills from `style.rs`, checked at 3:1 against the panel backgrounds); `Runtime::paint` draws it. No view uses any of it yet.
   - **Accessibility:** every widget gives its AccessKit node from the same element, so the tree and the screen cannot disagree.

     **Done:** `Element::access` takes an `Access` (`access.rs`), our own type, checked against spec section 7, rule 2 at layout. `Context::access_nodes` gives each laid-out panel's nodes, nested as the elements are, with IDs from the app's table, bounds in physical pixels, and the focus. The app splices them into its tree until phase 4. No view uses them yet.
   - **Cursors:** a cursor type of our own in `loom-ui-core`, in place of winit's `CursorIcon`, which `shell::Report` carries until then.

     **Done:** `loom_ui_core::Cursor` holds the shapes the app shows today: the canvas's, which were `CursorShape` in `loom-canvas`, and the text beam and not-allowed sign that Iced's text fields and turned-off switches show. `shell::Pointer` carries it, and the shell alone turns it into winit's `CursorIcon` (`shell.rs`, `icon`). The bridge maps Iced's `mouse::Interaction` onto it, and Iced's shapes no part of the app shows become the arrow. A shape is added when a view needs it.
   - **Style:** the tokens in `style.rs` (text sizes, button roles, palette) carry over unchanged. The test that forbids inline `.size(N)` keeps working.
   - **Slider steps:** our slider has a drag step apart from its arrow-key step, so dragging stays fine while the arrow keys move further. The inspector's slider then steps 1% of travel on an arrow key and on a screen reader's Increment, 0.1% with Shift, as the tile's knob does: Brylie's choice of 2026-11-11, deferred until here because Iced's slider shares one step between both ([UI layer spec](../design/ui-layer.md), the decisions row on how an inspector slider steps).
2. Stacking follows the layers the user sees, not which toolkit draws them. While both toolkits draw, keep today's order: canvas, then panels (toolbar, inspector, stats bar), then overlays, then the question on top. A converted toolbar must not cover an Iced question. So draw and route by layer: each layer is drawn by its toolkit in turn, and an event goes to the topmost layer first, then down to lower layers only if it is not taken. Converting the overlays early (row 3 below) keeps this simple, because the top layers then belong to `loom-ui`. A test checks that the question takes a click over the toolbar at every step.

   **Done:** drawing and routing by layer ([spec](../design/ui-layer.md), section 5, rule 10; section 6, rule 2). The runtime's Canvas and Panels layers draw after the canvas and before Iced, and its Overlays and Question layers after Iced, each side with its own `PaintRenderer`; a side with nothing painted draws nothing. A pointer event goes to the runtime's layers above Iced, then Iced, then the layers below: Iced answers late (`Routed::Later`), so the bridge hands what it leaves unused to `Panels::pointer_below` after its update, and keeps what a layer above takes from Iced (`iced_bridge/pointers.rs`, shared with the `Ui` harness). One layer holds the hover; a click is a press and a release on the same element (Brylie's decision, 2026-11-17), heard through `App::clicked`; the runtime's focus reaches the app through `App::focus_moved`. Tab with nothing focused enters the runtime's first stop, and a Tab it takes never reaches Iced. This also fixes a bug on main from step 3.1's focus work: a focus the runtime took from an Iced field was released only after Iced had heard the next events, so the next key could still land in the field. The release now runs first (`Target::release_focus`). The app shows no panel of the runtime's yet; tests add two through `Hosted`'s test hook. Next: row 1.

### 3.2 The order of views

Move the simplest views first. One pull request per row, each passing the parity gates. Each row also moves its parts' bounds, for `locate` and the accessibility tree, from Iced's named containers to our layout by UI ID. The canvas is not a row of its own, since it already draws itself (step 2.2c), but its place and live camera still reach `locate` through `Find`. They move in the last row, before `Find` goes: our layout places the canvas in the window, so its bounds come from that layout, and the camera comes from the canvas's own input state (`CanvasInput`), which then lives in our widget, not in an Iced widget's state. The last row removes the `Find` and `Places` window questions and `places.rs` (spec section 10, rule 6).

| Order | View | Widgets it needs |
|---|---|---|
| 1 | Stats bar, status messages | text, container, tooltip |
| 2 | Toolbar and breadcrumb | button (four roles, with hovered, pressed, and off looks), hover text, space, rule |
| 3 | Overlays: question, probe menu, recent, licence chooser | container, opaque, pin, button, radio, scrolling list |
| 4 | Inspector and matrix | slider, checkbox, radio, pick_list, number entry, scrollable |
| 5 | Module browser | text_input with focus, scrollable list, keyboard navigation |
| 6 | Help pages | markdown: parse with `pulldown-cmark` †, lay out with our text |
| 7 | Patch info | Folded into row 4: the inspector draws patch info when nothing is selected, so its Tags field, Licence button, and Authors box move with that row, and the multi-line field is built there (item 3.3.3). Nothing is left for a row of its own |
| 8 | Preferences | **Done:** a radio group, check boxes, selects (with the step's start, spec section 5, rule 18), a field, buttons, and a scroll container (`settings/panel.rs`). Its keys and focus: [preferences](../design/preferences.md#keys-and-focus). It opens the licence chooser, so it lands after row 3c. Its parts moved under `preferences/body`, a listed break |

**Row 1 done:** the stats bar and the status line are `loom-ui`'s (`stats_bar.rs`; [spec](../design/ui-layer.md), section 3, rule 4; section 5, rule 11; section 6, rule 2; section 7, rules 2 and 22). Two panels in the Panels layer: `bottom` fills the window and holds `stats` in a slot 24 px high (`style::STATS_BAR_HEIGHT`, Brylie's choice), and Iced keeps an empty space of that height. Row 1 also builds a plain button (the ×) and a switch (the developer switches), Brylie's choice, so the bar moves whole. `loom-ui` gains keyboard activation (Enter on key down; Space on key up, cancelled by Escape, a focus move, or a pointer press), hover text painted by the runtime (`hint.rs`: at once, on hover and on keyboard focus, the latest input wins, Escape hides it, and it stays while the pointer is on it), `Still` for offscreen captures, and `Context::{bounds, text_bounds, laid_out}`. The bar's nodes come from its elements and are spliced into the app's tree; `locate` reads the bar from the runtime's layout, after the draw when a live value has just rebuilt it. The status line's UI IDs move under the bar's panel (`stats/status`, `stats/kept`, and their `dismiss`). Tab: the canvas keeps Tab from nothing, as before; Shift+Tab from nothing enters the bar; patch info's fields hand Tab on to the bar at either end while it has a stop (Brylie's decisions, 2026-11-17).

Gates (macOS, release): `--dump-ui` and `--dump-ui --frames 4800` for all 11 example patches, and `--screenshot` of each canvas, are byte-identical with main. In `--screenshot-window`, the toolbar and the right column are identical; the bar's text differs, as it changes toolkit; and the canvas differs only because it is 0.4 px shorter (24 px against 23.6), which moves the camera's fit by a fraction of a pixel: with the bar at 23.6 the canvas is identical. With a 5% fuzz, 1,000 to 6,000 canvas pixels differ per patch. Text widths, by `locate` on `subtractive.ron`: the patch, audio, dropouts, and MIDI labels change by −0.54 to +0.07 px (median −0.17 px), since our layout rounds to whole pixels; the status line takes the width left, 6.3 px more. Idle on `subtractive.ron`, 6 s runs alternating with main: 8 runs plain, 66.0 frames (64 to 70) at 4.92% CPU (3.2 to 5.5) against main's 65.6 (60 to 68) at 4.81% (3.2 to 5.6); 4 runs with `--dev`, 67.0 frames (66 to 68) at 4.97% (3.5 to 5.6) against 66.0 (65 to 69) at 4.35% (2.8 to 5.7). The first build cost about 0.6 points of CPU, from preparing the bar's paint lists on every frame; a list that matches the last is no longer prepared again (`PaintList::same_as`). The bar's panel is built again only when its model changes: at most twice a second while audio plays (the load), never with no audio, and every frame only with `--dev` while Measure is on. These screenshots are the new baseline.

**Row 2 done:** the toolbar and, inside a sub-patch, the breadcrumb are `loom-ui`'s (`toolbar.rs`, `breadcrumb.rs`, `chrome.rs`; [spec](../design/ui-layer.md), section 4, the breadcrumb's key; section 6, rule 3 and its "From row 2" order; section 7, rule 22 and the breadcrumb's node row). One root panel, `window`, replaces row 1's `bottom` and holds the slots `toolbar` (`style::TOOLBAR_HEIGHT`, 40 px), `breadcrumb` (`style::BREADCRUMB_HEIGHT`, 32 px, only inside a sub-patch), a fill that paints nothing, and `stats`; Iced keeps one space of `style::top_height(inside)` above the canvas. The table's "toggler" was stale: no part of the app uses one, and MIDI learn is a button whose role shows its state. The button roles take tokens of our own (Brylie's choice; [visual system](../design/visual-system.md), Role tokens), which Iced's role style functions read too, so every button in the app matches. `loom-ui` gains `Element::looks` (a fill and a text colour for normal, hovered, pressed, and off, chosen when painting), a frame whenever what shows as pressed changes, `App::tab_from` for the ends of each run of stops, and an `aria_current` state; a button or a switch takes no focus from a pointer press (Brylie's decision, 2026-11-19). The toolbar's nodes match a snapshot taken from Iced's toolbar before the move (`toolbar/nodes.txt`), with Focus added. The bar's rule stays on Iced and moves the next time the bar is touched. The role style functions that map Iced's button states onto our tokens are a bridge, deleted in phase 4 with Iced; `Looks` and its states are named for what our buttons need, not for Iced's.

What row 2's choices assume, by target (Brylie's direction of 2026-11-19: the browser, the CLAP and VST3 plugins, and the stand-alone app on Windows, macOS, and Linux are all first class):

- **The root panel and its slots** assume only a window of some size, which every target gives: a plugin's window, a page's canvas, or the app's own.
- **The role tokens** are set for the one dark theme. A light theme, or a host or page that asks for one, needs a table of its own.
- **Tab's wrap** from the bar back to the toolbar assumes the app owns its window. In a plugin or a page, focus may need to go back to the host or the page: an open item in the spec (section 6, rule 3), decided when each target is built.
- **A click takes no focus** on every platform: macOS's convention, which Brylie chose for all (Windows focuses a push button on click).
- **The breadcrumb's current mark** (`aria_current`) reaches screen readers through AccessKit on Windows and Linux; `accesskit_macos` 0.27.2 does not read it, hence the "Open now" description. On the web the mark is the same ARIA attribute AccessKit is named for.
- **Text** is measured and drawn by `loom-ui`'s typesetter (Parley), the same on every target, so widths no longer depend on the platform's toolkit.

Gates (macOS, release, against main 447bb01f): `--dump-ui` and `--dump-ui --frames 4800` for all 11 example patches, and `--screenshot` of each canvas, are byte-identical. `--screenshot-window`, with the toolbar band and every role button outside it masked (on the main view, patch info's Choose and Keyboard shortcuts), is identical for all 11 patches when the heights are Iced's own, 39.9 and 31.9 px; at the whole-pixel tokens, 40 and 32, the canvas and the right column move down 0.1 px, which changes 12,000 to 49,000 pixels per patch, as row 1's bar did. Inside a sub-patch, with the toolbar and breadcrumb masked, the right column is identical; only the bar's live load and dropout readings differ, which move with time. Text widths, measured with Iced's paragraph and the runtime's `Typesetter`: 12 px text within 0.5% (the bar), 13 px labels 1.3% to 3.2% narrower, and the 15 px patch name 3.8% to 5.3% narrower, so the toolbar's buttons move 6 to 8 px left on the left and up to 2.5 px narrower on the right (`locate`). Checked against the font itself, not against Iced: the system face (".SF NS", 2048 units per em) gives "Preferences" at 13 px 65.514 px by its `hmtx` advances, and "subtractive.ron" at 15 px 93.618 px, which is exactly Iced's width, since Iced sets this text unshaped. Ours shapes it, and the shaper applies the font's own `GPOS` kerning ("Pr" and "re" pull in by 0.293 px at 12 px, 0.318 at 13, 0.366 at 15, in proportion to size) and its `trak` table's size-specific tracking (each glyph 0 px at 12 px, −0.076 at 13, −0.234 at 15). Those two account for the whole gap: "Preferences" at 13 px is 65.514 − 1.271 (kerning) − 11 × 0.076 (tracking) = 63.407 px, our measured width. So ours follows the font's own spacing, as the system's own text does, and the gap is Iced's: no size unit, hinting, or scale is wrong.

**Row 4, as planned** (approved 2026-11-20): five pull requests, since the row is the largest.
- **4a, `loom-ui`:** the scroll container and the radio group ([spec](../design/ui-layer.md), section 5, rule 12; section 6, rule 9; section 7, rules 28 and 29). Built first, for the inspector and for row 3's licence chooser (3c), so neither builds its own. Built: `Element::scroll` with its `ScrollStyle`, `Element::radio_group` and `Element::radio` with `.note`, and `Key::PageUp` and `Key::PageDown`; the app's tokens are `style::scroll()`. No view uses them yet.
- **4b, `loom-ui`, done:** the slider, with a drag step apart from its key step, and the check box (#882); the select with its list, which is the runtime's own overlay panel, placed by 3a's placement (spec section 5, rule 18).
- **4c, `loom-ui`:** the multi-line field (spec section 8, rule 7).
- **4d, app:** the inspector, with patch info (row 7, folded in) and the sub-patch inspector. One pull request in three commits (Brylie, 2026-11-25), each moving whole variants of what the inspector shows. **4d.1 built:** the root panel's slot for the column, below the tabs Iced still draws (`style::RIGHT_TABS_HEIGHT`, `RIGHT_COLUMN_WIDTH`), and `inspector/panel.rs`, which draws patch info, a selection of several, a port tile, and nothing, while `App::inspector_ours` says the runtime draws the column. Patch info's Authors box is the multi-line field; every control of patch info is a Tab stop, the tags one toolbar stop (ui-layer.md, section 6, rule 3); its nodes are the runtime's. Every part keeps its UI ID but one: the second hint, `inspector/patch/hint/add`, is `inspector/patch/add_hint`, since a text holds no child. A loom-ui fix came with it: a focus report runs before a message that waited in the queue. **4d.2 built:** a module's, an unknown module's, and a slot's inspector are `loom-ui`'s (`inspector/panel/rows.rs`, routed by `inspector/panel/routes.rs`): parameter rows with the runtime's slider and its ghost mark, radios, selects, the value field, Range with Full first, the MIDI line and its settings, modulation cards with their depth, Add modulation and its sources, Drives, the ports, and the slot's part-only group. A click sends what the part's accessibility item sends for Click, so a pointer, a key, and a screen reader agree. Inspector slider keys step 1% of travel, 0.1% with Shift, as the decisions row of 2026-11-11 planned. IDs moved to follow the nesting (the decisions row on how the inspector's parts are keyed, a listed break). With it, in `loom-ui`: the check box look as one token (`PanelStyle::check`), a slider's ghost mark and a hover's end (`Slider::ghost`, `App::unhovered`), focus and select all for its fields, and a screen reader's slider steps grouped as keys are. **4d.3 built:** a sub-patch's inspector is `loom-ui`'s too: its name field, Open, the Mode select, a layer's or a select's members with Open, Up, Down, and Remove, Add member, Copies, Stereo spread, each spread's slider, the exposed rows, and its Authors box as the multi-line field. Iced draws no part of the inspector any more: `inspector::view` and its helpers are gone, and the right column's tabs, the Matrix, and the Outline are what Iced still draws there, until 4e. A layer's members moved to `<t>/members/<member>`, inside their group (a listed break).
- **4e, app:** the matrix, as a WAI-ARIA grid, whose Depth cell is entered with Enter or F2. One pull request in three commits (Brylie, 2026-11-25): the right column's tabs, then the Outline with the cable jump list, then the matrix.
  - **Commit 1, the tabs, built:** a panel of `loom-ui`'s (`tabs.rs`) in a slot at the top of the right column (`chrome.rs`), by the WAI-ARIA Tabs pattern: one stop, Left and Right that wrap, Home and End, the panel following at once, and Tab into the open panel ([spec](../design/ui-layer.md), section 6, rule 2's table; section 7, the role table). The tree groups the runtime's tabs in a tab list and the open content in a tab panel, so every tab keeps its ID.
  - **Commit 2, the Outline and the cable jump list, built:** panels of `loom-ui`'s in the column's content slot (`outline/panel.rs`). The tree is one stop, its current row, by the WAI-ARIA Tree View pattern; its list takes the tree keys first and hands them to the app's tree logic, and the app groups its rows' nodes under a Tree node. The jump list is a list box (the WAI-ARIA Listbox pattern). Rows move to `outline/list/<row>`, one key each, a listed break. `loom-ui` gains `Access::level`.
  - **Commit 3, the matrix, built:** a panel of `loom-ui`'s in the column's content slot (`matrix/panel.rs`), a grid by its keys (the WAI-ARIA data grid pattern) and a list to a screen reader ([spec](../design/ui-layer.md), the decisions row of 2026-11-27; [modulation](../design/modulation.md), slice 3). The grid is one Tab stop, its chosen cell; its card list takes the grid's keys first, including F2, Home, End, Ctrl+Home, Ctrl+End, Page Up, and Page Down, and the focus moves from cell to cell. Each cell is its widget: On a check box, Source, Target, and Polarity the runtime's select in a cell (`Select::in_cell`), which opens on Enter or F2, and Depth the runtime's slider, with a field Enter or F2 enters. The filter, Selected module only, Sort by, and the Add form are the runtime's field, check box, select, and buttons. The matrix's flag of its own (`matrix.focused`), its Iced key listener, and its item list are gone: the runtime's focus says who has the keys, which also fixes #871. `loom-ui-core` gains `Key::F2`. The card list moves from `matrix/assignments` to `matrix/assignment`, a listed break. Iced now draws no part of the right column, and no field or button anywhere.

**Rows 5 and 5b done** (plan 3.2, 2026-11-24): the module browser and Your patches are `loom-ui`'s (`browser/panel.rs`, `patches/panel.rs`; [module browser](../design/module-browser.md), Focus and Tab; [patch metadata](../design/patch-metadata.md), Keys; [spec](../design/ui-layer.md), section 5, rules 13, 14, and 19; section 6, rules 3 and 8). Each field takes its list's keys first and points at the highlighted row; the lists are the runtime's scroll containers, so row IDs moved under them (`browser/list/<kind>/<id>`, `patches/list/row/<path>`). The runtime gained hover and context menu reports, an anchored root told when its anchor stops showing, and stacked roots that close on a press outside. A control channel's scripted keys now go as the keyboard's (`Report::input`).

### 3.3 Text input and editing

1. Single-line input (browser search, number entry, names) builds on Parley's `PlainEditor`, with Parley's `complex-scripts` feature for dictionary word breaking in Thai, Lao, Khmer, Burmese, Chinese, and Japanese, behind a field type of our own: cursor, selection, word movement, and clipboard come from it, and undo inside the field is ours, by snapshots of text and selection (about 80 lines).
   - **`complex-scripts` is on by default, through a Cargo feature** (Brylie, 2026-11-16). `loom-ui` has a `complex-scripts` feature that turns on Parley's, and `loom-app` has one of the same name in its `default` features; a smaller build, such as a plugin, leaves it out with `--no-default-features` and saves about 3 MiB. It cannot be on for fields alone: it picks `parley_engine`'s word and line segmenters when Parley is built, and Cargo turns a feature on for every crate in the build, so the canvas has it too. Checked on macOS with it off and on: `--screenshot-window` and `--screenshot` for all 11 example patches, and `--dump-ui` for each, are byte-identical (two runs of the baseline matched first), so the canvas's text does not move. `Cargo.lock` is unchanged. The release `loom-app` grows from 28,573,336 to 31,664,968 bytes (+3,091,632, about 2.9 MiB, +10.8%), the segmenters' dictionaries. A test that runs only with the feature (`thai_word_movement_stops_at_a_dictionary_word`) shows it reaches Parley: one word step in "สวัสดีครับ" stops after "สวัสดี", 18 bytes in, where it stops after one character without the feature.
   - **Done, in `loom-ui` (first part):** `Element::field` and `FieldEvent`, with editing by Parley's `PlainEditor` on contexts of its own (the process's one font scan), undo by snapshots, the keys of spec section 6, rule 6, scrolling to keep the caret in view, a steady caret and a selection drawn from `PanelStyle`, and a node with the field's text as its value (`crates/loom-ui/src/field.rs`, `examples/field.rs`). **Done (second part):** the text runs, the caret and selection for a screen reader, and SetValue, SetTextSelection, and ReplaceSelectedText, routed by `App::access_target`, with `App::access_changed` telling the app when what a field reports changed (spec section 7, rules 7 and 10). **Paste (3.3.2) must land before any view moves to the field:** until then, paste in a field goes on to the app, which has no field to paste into.
2. IME: send `winit` IME events to the focused field through `PlainEditor`, and report `ime_cursor_area` to the window so the candidate window appears in the right place. `winit` ends a composition with an empty `Preedit` and then a `Commit` that carries the final text, so the mapping is:
   - `Preedit` with text: `set_compose(text, cursor)`.
   - `Preedit` with empty text: `clear_compose()`. This drops only the preedit text, never committed text.
   - `Commit(text)`: insert `text` at the selection (`insert_or_replace_selection`). `finish_compose` is not used for it: it would keep the preedit as text, and the commit would then insert it a second time.
   - Focus leaving the field while composing: `finish_compose()`, so what the user saw stays.

   A test sends the sequence `Preedit("にほ")`, `Preedit("")`, `Commit("日本")` and checks the field holds "日本" once, with the caret after it. Also test with a Japanese or Chinese input method on macOS and record the result.

   **Done (plan step 3.3.2):** the mapping above, in `loom-ui`'s field, with the field's own paste through the driver (`Driver::clipboard_paste`) on the same clipboard copy and cut use, made one line by `one_line`. The answer is a `Paste` reply, not the `DriverEvent::Pasted` this plan's review first named: the driver gives the text to a runtime channel and calls the waker's wake function, so it stays asynchronous and comes on a later turn (the 2026-11-11 decision), and no driver's event loop changes. It goes in only if focus has not moved since the paste was asked for, not even away and back. The by-hand check with a Japanese input method on macOS is for the Manual QA round. This fulfils the note in 3.3.1 that paste must land before any view moves to the field.
3. The multi-line editor (patch info authors) is built with row 4, the inspector (Brylie, 2026-11-19), not last: the inspector draws both Authors boxes, patch info's when nothing is selected and a sub-patch's, so row 7's widget arrives with row 4 and lands with its one view and that view's parity gate. Its rules are in the spec, section 8, rule 7. It is the single-line field's core with a setting for many lines, so most of it is Parley's: wrapping, vertical moves that keep the caret's place, and line geometry. The new parts are the keys per platform, growing with its lines, text runs per line, and `multi_line` for pasted text. Until row 4 it stays on Iced's `text_editor`, and a screen reader can read the authors but not edit them. **Built in `loom-ui` (row 4c):** `Element::text_area`, the single-line field's `FieldState` with `Lines::Many`: wrapping to its width, growing with its lines, Parley's moves by line, the keys of rule 7 for each platform, Enter inserting a break, `multi_line` for text that comes in, runs per visual line linked along each line, and a `MultilineTextInput` node. Page Up and Page Down off a Mac go to the text's start and end, since one page is the whole growing box. On a Mac, Home and End scroll the containers around the field to its start or end through 4a's scroll container (`Window::scroll_to_show_edge`), and leave the caret. The Authors boxes move to it in 4d.
4. Clipboard: use a permissive clipboard crate such as `arboard` †, behind the effect from 2.1, so the harness keeps its in-memory clipboard. Not in step 3.3.2: the field's paste reads through the app's existing clipboard, the one copy and cut write (Iced's, until the shell takes the clipboard from it). Reads and writes move to `arboard` together, so one backend serves both ways: two can disagree, mostly on Linux, where the app that last wrote the text holds it. From `arboard` 3.7.0's docs, for then: on Windows the clipboard may be opened on one thread at a time; on X11 and Wayland a `Clipboard` must stay alive to keep what it wrote; macOS says nothing about threads. So one long-lived `Clipboard` on one thread. **Done in step 3.3.4** (spec section 8, rule 2): the shell's clipboard moves to `loom-ui`'s own `clipboard` feature (`arboard`, `default-features = false`, so no images), on one thread, which the `shell` feature turns on and a plugin's driver can use with no winit; `Services` loses its clipboard methods (a listed break); Iced's widgets use the same clipboard through a bridge adapter until phase 4; reads answer `ClipboardText` (text, empty, or refused). The primary selection on Linux is for later.

Each view's move changes its text widths once (spec section 8, rule 6). The view's step uses the gate of step 2.4b, item 3, masked to that view's text, and its screenshot becomes the new baseline.

### Phase 3 done

No view uses an Iced widget. The parity gates pass. The VoiceOver check covers the toolbar, inspector, browser, and every overlay, with keyboard focus that the screen reader follows.

## Phase 4: Remove Iced

Written 2026-11-25 from a read-only breakdown at `323ead6a` (rows 4d and 4e still open). The rules are in [the spec](../design/ui-layer.md): the host contract (section 9), the keys (section 1, Keys in `loom-ui-core`), the key order (section 6, rule 6), the end of the bridge (section 2), the harness (section 10), and the [phase 4 criteria](../design/ui-layer.md#acceptance-criteria). Brylie's decisions of 2026-11-25 and 2026-11-26 are in its decisions table.

Phase 4 is more than removing dependencies. The canvas is still an Iced widget at the window's root, and `loom-ui` cannot host it yet. That is the main new work (P1). The canvas's place and camera, the `Places` and `Find` window questions, and the window's root move here, not in row 4e (Brylie, 2026-11-25).

Order: P0, then P1, P2, and P3 side by side, then P4, P5, P6, P7, and P8. P6 needs only P4, so it can go beside P5. Each pull request says `Part of #683`, updates the specs it changes, and passes the parity gates. P8 closes #683.

| Step | What | Size | Starts after |
|---|---|---|---|
| P0 | This spec and plan (docs only) | S | now |
| P1 | `loom-ui` hosts a custom element | L to XL | row 4d |
| P2 | The app's keys and events on `loom-ui-core` | M | row 4e |
| P3 | Colours off Iced's theme | M | row 4e |
| P4 | The cut-over: the window, the harness, the canvas, the control channel | XL | P1, P2, P3, all of phase 3 |
| P5 | Remove the Iced dependencies | S to M | P4 |
| P6 | The runtime owns the whole accessibility tree | L | P4 |
| P7 | Docs and the explainer | M | P5, P6 |
| P8 | The final check; closes #683 | S | P7 |

### P0. The spec and this plan

Docs only. The phase 4 criteria, section 9 as a host contract, the key types, section 6, rule 6's order once Iced's listeners go, the end of the "until phase 3" text, and section 10, rule 2's allowance for a mechanical type change. *Why first:* P1 and P2 would otherwise guess (spec first).

### P1. `loom-ui` hosts a custom element

1. Build the host of spec section 9, in `loom-ui`, with no patch words: layout bounds, pointer routing with capture during a drag and the cases that cancel it, hover out, the wheel, keys in both cases (focused, and the home element while nothing has focus), one Tab stop, the cursor, deadlines and animation frames, both ways to draw (painting with no GPU, and the GPU callback, in paint order), nodes under its UI ID, focus requests into inner parts, and drawing above a layer.
2. The host gets the app's model through the `App` trait: the element's trait gets `&App` on every call, which needs `App: 'static` (spec section 9, the model path; Brylie, 2026-11-26).
3. It is additive. The routing target, `Routed::Later`, and the canvas's place in the bridge stay as they are until P4, and a test shows that an app with no custom element behaves exactly as before.
4. It replaces nothing yet. Proof: an example program with no LoomDSP (spec section 1, rule 3) and two custom elements, a level meter and an XY pad, each giving its nodes. The meter and the piano roll stay the design cases the contract is checked against (spec section 9, A contract for every app); the XY pad is the small, buildable example, with no musical types; a table in the pull request of how each host part holds on the desktop, in a plugin window, and on the web; headless tests for routing, capture, focus, and nodes; `loom-ui`'s `tests/purity.rs`.
5. One pull request, ordered commits (Brylie, 2026-11-26, "One PR, ordered commits"): first everything with no GPU (state by key, layout, routing, capture, the wheel, keys, focus, cursor, ticks, nodes, and the paint path), then the GPU path (the GPU callback, the paint list cut at each element, a lost device, and an offscreen order test). Each commit is tested headless on its own.
6. Risks: the order of render passes, and who owns the element's state across a level change (spec section 9, rule 1).

### P2. The app's keys and events on `loom-ui-core`

1. Add `Key::F1`, `Key::F2`, and the physical key (spec section 1, Keys in `loom-ui-core`), and map them in the shell.
2. `Message::Key`, `Message::Input`, the matrix's keys, `menus.rs` (`step`, `menu_key`), `panic.rs`, the Outline's keys, the shortcuts in `lib.rs`, the listeners in `controls.rs`, and the control channel's `key()` take `loom-ui-core` values. Iced still delivers until P4, so the bridge converts once (`event_to_core`).
3. A dropped file and a close request go through `App::window_event` where the shell path allows.
4. Commits: the types and the shell's mapping; the app; the tests, mechanical and alone (spec section 10, rule 2).
5. Gates: the key order test (spec section 6, rule 6); the control channel's `key` examples give the same replies; Panic works under a field, a question, and a drag.
6. Risks: the platform's shortcut key (`Platform`) must give Cmd on a Mac and Ctrl elsewhere for every shortcut; non-US layouts for `.`, `,`, `[`, and `]`.

### P3. Colours off Iced's theme

1. `style.rs` tokens give `loom_ui_core::Color` from our own palette constants; `boot.rs` drops Iced's `Theme`; the Iced role style functions with no user left are deleted.
2. **Clean room:** about 20 files read the shades Iced works out from its palette. Do not port how Iced works them out. Record each shade the app uses as a named constant with its resolved value.
3. Gates: `--screenshot-window` byte-identical; the contrast tests in `style/tests.rs` unchanged.

### P4. The cut-over

One pull request with ordered commits (Brylie, 2026-11-25), since the window and the harness must switch together (spec section 10, rule 1).

1. The window's root as a panel of the runtime's, and the canvas as a custom element around `CanvasInput` (`PatchCanvas`, `loom-canvas`'s `widget.rs`, `widget/`, and `iced_convert.rs` go).
2. The app's own shell layers, in spec section 9, rule 8's order; the app implements `loom_ui::App` itself.
3. The harness on `loom_ui::headless`, laid out with the `Typesetter`; captures on an offscreen `wgpu` texture; `screenshot.rs` loses Iced's `UserInterface`.
4. The control channel: pointer commands as `DriverEvent`s, and `locate` from the runtime's layout and the canvas's camera (spec section 10, rule 6). `places.rs`, `window_question.rs`, and the pointer injector go.
5. Delete the bridge. Its native dialogs (`services.rs`) move into the app; Iced's futures and its copy of `wgpu` become `pollster` or `std` and `wgpu` itself.
6. The tests' mechanical type changes, alone (spec section 10, rule 2).

Delete the text gate tool (`crates/loom-app/src/screenshot/text_gate_tests.rs`) and the measuring renderer (`iced_bridge/measure.rs`): both measure with Iced's paragraph.

Before the cut, if it shrinks P4, move `services.rs` out and give offscreen captures a device of their own, as small commits or a small pull request.

Risks to check, each in the pull request:

- **Render passes:** the canvas's order, the dragged cable above the panels, and the canvas's scissor.
- **Colour:** Iced's colour features decide the surface format and blending today; the shell's format and both paint renderers must give the same pixels.
- **Layout:** the window's root moves to our layout, so `--screenshot-window` may differ once by part of a pixel; `--screenshot` stays identical.
- **IME:** `preedit.rs` goes; every field draws its own composition, and `Runtime::ime` places the candidate window. A Japanese or Chinese check by hand.
- **Accessibility:** focus requests into the canvas, its typed value field, and Focus on inner parts.
- **Clipboard:** the bridge's blocking read goes; nothing else may read synchronously.
- **Tests:** a bridge test that also tests behaviour is moved, not deleted. The pull request lists every deleted test and why.

Gates: every parity gate; `--measure-idle` (counts may fall, since the bridge's redraw after any update goes); `--generate 300 --bench`; the `--control-script` examples; every `locate` answer before and after, for every kind of target; VoiceOver.

### P5. Remove the Iced dependencies

1. Drop `iced`, `iced_wgpu`, `iced_renderer`, `iced_winit`, and `iced_runtime` from the workspace and every crate. #683 names three; all five go (the spec wins).
2. Set `wgpu`'s and `winit`'s features directly, where Iced turned some on. Check Linux window decorations survive: `cargo tree --target x86_64-unknown-linux-gnu -e features -i winit`.
3. Purity: the Iced rules in `loom-app`'s and `loom-canvas`'s `tests/purity.rs` go, with no source scan in their place. `deny.toml` bans the `iced*` crates (`[bans] deny`), so `cargo deny check` catches Iced coming back, and `rfd` keeps its own rule (spec section 1, rule 7; Brylie, 2026-11-26).
4. `deny.toml`: remove each ignored advisory that left with Iced, and reword the OQ-14 comment.
5. Gates: `cargo deny check`; `cargo tree -d` against the baseline below, on macOS, Linux, and Windows; CI on all three.

### P6. The runtime owns the whole accessibility tree

1. The runtime builds the tree from panels and custom elements' nodes and keeps the only `IdTable` (spec section 7). The app's tree code in `a11y/` is left only for the canvas's nodes; `a11y/items.rs` goes.
2. Risks: node IDs stay stable; hiding the layers under an overlay or a question; the 250 ms cap on live values (spec section 7, rule 7); `controls` relations across the canvas and the panels.
3. Gates: `Ui::a11y_tree()` dumps identical before and after, recorded for every view and overlay; node ID stability tests; VoiceOver.

### P7. Docs and the explainer

- `AGENTS.md` (the App, Canvas, UI runtime, Panels, and accessibility tree paragraphs, and any command that changed); `docs/dev/architecture.md`; `docs/design/visual-system.md`; `docs/design/control-channel.md` (say which limits the injector and Iced's overlay cursor set, and that they are gone); the `ui_test.rs` module comment.
- OQ-13 and OQ-14 in `docs/product/open-questions.md`: OQ-14 closes; OQ-13 now depends on our shell and driver trait.
- `README.md`'s crate table, `roadmap.md`, and the specs that name Iced (`panels.md`, `undo.md`, `telemetry.md`, `modulation.md`, `preferences.md`).
- `ui-layer.md`: tick the phase 4 criteria and remove the bridge's text. This plan: mark phase 4 done.
- An explainer in `docs/learn/` on how the UI layer works: the runtime's turn, effects, layout, custom elements, and the accessibility tree. Link it from the spec and from `loom-ui`. It is the documentation Iced did not have.
- ADRs and research snapshots are history and keep their text.

### P8. The final check

1. `cargo deny check` and `cargo tree -d`.
2. Run every command in `AGENTS.md`'s Commands section, the `--measure*`, `--bench`, `--control*`, `--screenshot*`, `--dump-ui`, `--fake-audio --record`, `midi-port`, and `scripts/bundle-macos.sh` ones included, and a real device `audio` command where one is at hand.
3. The parity gates and VoiceOver.
4. Close #683. Step 1.5 (filing the AccessKit gaps upstream and taking the fix) and the Narrator and Orca checks move to issues of their own first (Brylie, 2026-11-25).

### The duplicate baseline

`cargo tree -d -e normal --workspace`, macOS, at `6ffb482e` (main with #896; the same list as at `323ead6a`): bitflags 1.3.2 and 2.13.2; block2 0.5.1 and 0.6.2; core-foundation 0.9.4 and 0.10.1; core-graphics-types 0.1.3 and 0.2.0; font-types 0.10.1 and 0.12.6; harfrust 0.3.2 and 0.12.0; hashbrown 0.16.1 and 0.17.1; miniz_oxide 0.8.9 and 0.9.1; objc2 0.5.3 and 0.6.5; objc2-app-kit 0.2.2 and 0.3.2; objc2-foundation 0.2.2 and 0.3.2; read-fonts 0.35.0 and 0.41.0; rustc-hash 1.1.0 and 2.1.3; skrifa 0.37.0 and 0.44.0; syn 2.0.119 and 3.0.9.

The older `harfrust` and `skrifa` should leave with Iced, and likely `read-fonts` and `font-types` (through `cosmic-text`). The `objc2` 0.5 family stays, with `winit` 0.30. P5 records the same list on Linux and Windows before it removes anything.

### Other phase 4 decisions

- Binary size and build time are recorded before and after P5, on each platform, and are not a gate (Brylie, 2026-11-26). They matter for plugins, since a DAW may load many instances.
- P1 builds the host for every app on `loom-ui`, not only LoomDSP's window ([spec section 9, A contract for every app](../design/ui-layer.md#a-contract-for-every-app-built-on-loom-ui)): its example has a level meter and an XY pad beside nothing of LoomDSP's (the piano roll stays a design case), and its pull request says how each host part holds for a plugin window and the web.

### After #683

Follow-up issues, filed when P0 merges (Brylie, 2026-11-26). None is part of phase 4.

- **A plugin driver** (CLAP, VST3): the host owns the loop and the parent window; repaint and deadlines come from its idle or timer callback. After the OQ-13 spike.
- **A web driver** (`wasm32`): a canvas element, DOM events, and the accessibility tree mirrored to the DOM. OQ-23 is decided first.
- **The `wgpu` upgrade, then Vello** weighed against our glyph atlas, now that Iced no longer fixes the version (rule 7 above held it).
- **Research: AccessKit in a plugin window.** Whether AccessKit works inside a window a DAW hosts, where the host owns the window and may answer the screen reader itself.

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
- Visual redesign. Every view looks the same after each phase, unless a change to it is decided and listed in its pull request ([spec design goal 5](../design/ui-layer.md#design-goals)). A redesign is still out of scope.
- Any change to the engine crates. `loom-core`, `loom-dsp`, `loom-modules`, `loom-patch`, and `loom-rt` stay as they are and never depend on UI crates.

## Unverified facts

Entries marked † were not checked against source or release notes when this plan was written. Iced's integration example is checked ([research note](../research/2026-11-winit-shell-facts.md)), and step 2.2 is written from it. Check each before relying on it: `taffy`, `pulldown-cmark`, and `arboard`, including their licences and the versions that match `wgpu` 27 and `winit` 0.30.

Checked in step 2.4a ([research note](../research/2026-11-text-crate-choice.md)): `glyphon` (MIT OR Apache-2.0 OR Zlib; no release for `wgpu` 27), and the spec's two Parley claims in section 8, rule 8. Parley's AccessKit text properties existed up to 0.11.1 and were removed in 0.12.0. Its editor does track IME composition.
