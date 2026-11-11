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
4. **Do not invent text.** Shaping, font fallback, line breaking, glyph drawing, and the editing model come from the text crate and its glyph renderer. Which crate is chosen at step 2.4 ([spec](../design/ui-layer.md), section 8, rule 8). The steps below name `cosmic-text` and `glyphon`, today's stack under Iced; if Parley is chosen, steps 2.4, 3.3, and phase 4 change to name it and its glyph drawing.
5. **Licences.** Every new crate goes through `cargo deny check` before it is used. Only permissive licences (ADR-0006).
6. **Clean room.** The agent may read the source of dependencies accepted under ADR-0006, which includes Iced, `winit`, `wgpu`, `cosmic-text`, and AccessKit. It must not read the source of other audio applications or other UI frameworks that are not dependencies.
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

This turns the spike into supported code. It needs the phase 0 spec. UI IDs ([spec](../design/ui-layer.md), section 4) join `loom-ui-core` in this step, with the table of accessibility node IDs as their first user.

1. Split `a11y.rs` into:
   - `a11y/tree.rs`: builds an AccessKit `TreeUpdate` from `UiSnapshot` and the other view models. No platform code, so it builds and is tested on every OS.
   - `a11y/actions.rs`: turns AccessKit `ActionRequest`s into `Message`s.
   - `a11y/macos.rs`: the `SubclassingAdapter` attach code from the spike, kept only until phase 2 replaces it.
2. Remove the `a11y-spike` feature and the `LOOM_A11Y_*` environment switches. Use the value and name policy the spec chose.
3. Extend the view models so the tree covers the toolbar, status bar, inspector rows, module browser, and every overlay, not only tiles. Each gets an accessible name, role, value, and state in our own words, as ADR-0028 asked.
4. Undo: group screen reader adjustments into undo steps the same way arrow-key edits group (`docs/design/undo.md`).
5. Rebuild the tree after messages that change the snapshot, not on a timer. Send only changed nodes. Handle `deactivate_accessibility`.
6. Tests in `a11y/tests.rs`:
   - every interactive widget in every view has a node with a non-empty name and a role;
   - every node's bounds lie inside the window;
   - actions round-trip through `App::update` as one undo step;
   - the tree changes when the snapshot changes, and only then.

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

## Phase 2: Own the window and the event loop

The window and the loop become ours. Iced's widgets still draw the panels around the canvas, but through Iced's lower layers driven by our loop, as the `Ui` harness already does headlessly. This is the biggest single step. It removes `iced::application` and Iced's `winit` shell.

### 2.1 Our own runtime

1. Create the runtime in `crates/loom-ui`, generic over the app and the driver ([UI layer spec](../design/ui-layer.md), section 2, rule 9). The code that links it to Iced during phases 2 and 3 goes in `crates/loom-app/src/iced_bridge/` (spec section 1, rule 6). The runtime owns:
   - the message queue;
   - `fn handle(&mut self, event: Event) -> ...`: our own event enum covering window, pointer, keyboard, IME, redraw, and wake-ups;
   - effects, which replace `Task`: widget operations (focus a field, scroll to), clipboard read and write, exit, file dialogs, timers, window commands. Each is an enum variant, not a future, so the headless harness can see and run each one or panic on it.
2. Port `App::update` to return `Effects` instead of `Task<Message>`. Do it file by file, behind an adapter that turns effects into Iced tasks, so the app keeps running on `iced::application` until 2.2 lands.
3. Port subscriptions to event sources that wake the loop:
   - the control channel listener, the MIDI learn watcher, the input watcher, and audio watch send messages through a waker the driver gives (an `EventLoopProxy` in the `winit` shell);
   - timers (frame meter, alerts, autosave) become deadlines the runtime returns, which the `winit` shell waits on with `ControlFlow::WaitUntil`.

   While nothing is due, the loop sleeps (NFR-PERF-04, control channel rule 7).

### 2.2 The winit shell

1. Replace `iced::application` in `lib.rs::run` with a `winit` 0.30 `ApplicationHandler` that:
   - creates the window hidden;
   - creates the AccessKit adapter through `accesskit_winit` (all three platforms), then shows the window. This fixes the spike's attach order;
   - decides first how the Linux adapter comes in. With its default features, `accesskit_winit` brings `accesskit_unix` and a D-Bus stack that fails `cargo deny check` on three "unmaintained" advisories; without that feature the check passes, and Linux has no screen reader support ([research note](../research/2026-11-accesskit-macos-gaps.md#what-accesskit_winit-brings)). Either add ignore entries to `deny.toml` with reasons, or leave the Linux adapter out at first and say so;
   - creates the `wgpu` surface;
   - sends winit events to the runtime and redraws only when the runtime asks.
2. Draw each frame in this order:
   1. clear;
   2. the canvas, through the phase 1.3 API, straight into the frame;
   3. the Iced `UserInterface` for the rest of the window, drawn with `iced_wgpu`, the same way `ui_test.rs` and `screenshot.rs` build it today.

   Iced's own repository has an integration example for this pattern †. Read it; it is a dependency.
3. Route events: overlays and panels go through Iced's `UserInterface`; the canvas area goes to `CanvasInput` directly. Keep the hit-testing order the same as today (overlays above the canvas).
4. Delete `a11y/macos.rs`. The tree now goes through `accesskit_winit`.
5. Keep the window size, title, minimum size, high-DPI behaviour, file drop, and close confirmation exactly as they are. List each in the pull request as checked.

### 2.3 The headless harness on the same runtime

1. Rewrite `ui_test.rs` internals so `Ui` drives the phase 2.1 runtime, not Iced's runtime. Its public API (`Ui::new`, pointer and key methods, `output`, `input`, `control`, `text`, `exited`, the in-memory clipboard) does not change. The 15 files that use it should not need edits.
2. Separate layout and hit-testing from drawing, so the harness needs no GPU for event tests. Text measurement needs only `cosmic-text`'s font system, which runs on the CPU.
3. Drawing tests (`screenshot.rs`, its smoke test) render to an offscreen `wgpu` texture. Without a GPU adapter they skip with `NO_GPU`, as today. Iced's `tiny-skia` fallback goes away; nothing should depend on it once events need no renderer. If CI has no GPU, try a software `wgpu` adapter (such as llvmpipe through Vulkan, or WARP on Windows) and record whether it works.
4. Add an accessibility test driver to the harness: `Ui::a11y_tree()` returns the current tree, and `Ui::a11y_action(name, action)` performs an action by node name. Agents and tests can then check the tree the way a screen reader sees it.

### 2.4 Text drawing for the canvas

1. Replace Iced's text renderer for tile text with a glyph atlas renderer on `cosmic-text`. `glyphon` † is the usual choice; Iced's `cryoglyph` is a fork of it. Check its licence with `cargo deny`.
2. Give it a `Painter` and a `TextMeasure` (phase 1.3), in place of the Iced ones. Keep the level-of-detail rules in `docs/design/panels.md` (text close up, bars further out).
3. Share one `cosmic-text` `FontSystem` between the canvas and the Iced panels if Iced allows it; otherwise accept two for now and note it.

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

1. Single-line input (browser search, number entry, names) builds on `cosmic-text`'s `Editor`: cursor, selection, word movement, clipboard, undo inside the field.
2. IME: send `winit` IME events (preedit and commit) to the focused field, and report the cursor area to the window so the candidate window appears in the right place. Test with a Japanese or Chinese input method on macOS and record the result.
3. The multi-line editor (patch info authors) comes last. If it costs too much, keep it on Iced's `text_editor` and finish phase 3 without it. Phase 4 then waits until it is done.
4. Clipboard: use a permissive clipboard crate such as `arboard` †, behind the effect from 2.1, so the harness keeps its in-memory clipboard.

### Phase 3 done

No view uses an Iced widget. The parity gates pass. The VoiceOver check covers the toolbar, inspector, browser, and every overlay, with keyboard focus that the screen reader follows.

## Phase 4: Remove Iced

1. Remove `iced`, `iced_wgpu`, and `iced_runtime` from `Cargo.toml` files. Depend on `winit`, `wgpu`, `cosmic-text`, the glyph renderer, `accesskit`, and `accesskit_winit` directly.
2. Run `cargo deny check` and `cargo tree -d`, and remove duplicates.
3. Update the documents:
   - `AGENTS.md` (App and Canvas paragraphs, commands);
   - `docs/dev/architecture.md`;
   - `docs/design/visual-system.md` and `docs/design/control-channel.md`;
   - the `ui_test.rs` module comment;
   - OQ-13 and OQ-14 in `docs/product/open-questions.md`. OQ-14 closes; OQ-13 now depends on our shell, not Iced.
4. Write an explainer in `docs/learn/` on how the UI layer works: the runtime, effects, layout, and the accessibility tree. It is the documentation Iced did not have.

## Stop points

Each phase is worth keeping on its own. The maintainer can stop after any of them.

- **After phase 1:** accessibility on macOS through the spike's adapter, and a canvas free of Iced types. Still on Iced.
- **After phase 2:** our own window and loop, accessibility on all three platforms, and a shell that can be adapted for plugin embedding. Iced is only a widget library.
- **After phase 3 without the multi-line editor:** Iced is used for one widget.

Stop and ask the maintainer if any of these happen:

- a parity gate fails and the fix is not clear;
- IME or text editing needs more than a few hundred lines on top of `cosmic-text`;
- an AccessKit gap blocks a phase and upstream has not answered;
- the idle CPU or frame rate gets worse and cannot be recovered in the same phase.

## Out of scope

- Plugin window embedding (OQ-13). The shell should not block it, but this plan does not build it.
- The web target (ADR-0026).
- Visual redesign. Every view looks the same after each phase.
- Any change to the engine crates. `loom-core`, `loom-dsp`, `loom-modules`, `loom-patch`, and `loom-rt` stay as they are and never depend on UI crates.

## Unverified facts

Entries marked † were not checked against source or release notes when this plan was written. Check each before relying on it: Iced's integration example, `glyphon`, `taffy`, `pulldown-cmark`, and `arboard`, including their licences and the versions that match `wgpu` 27 and `winit` 0.30.
