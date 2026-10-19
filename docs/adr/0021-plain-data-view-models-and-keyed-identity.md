# ADR-0021: Plain-data view models and keyed widget identity

- **Status:** Proposed
- **Date:** 2026-10-19
- **Issue:** #224
- **Related requirements:** NFR-UX-02 (a plain model of what a panel shows is a natural source for screen-reader text)
- **Background:** [UI view models and widget identity](../research/2026-10-ui-view-models-and-identity.md) (research snapshot)
- **How to:** [Architecture, View models for app panels](../dev/architecture.md#view-models-for-app-panels)
- **Builds on:** [ADR-0009](0009-ui-stack-iced.md) (Iced), [ADR-0010](0010-own-patch-canvas.md) (our own canvas)

## Context

The desktop app (`crates/loom-app`) uses Iced 0.14. App tests need to check what the user sees: the inspector rows, the tile values, the status bar. Before PR #219 (issue #206) they could only send events and look for text in the widget tree. PR #219 split the inspector into a plain-data model (`inspector.rs`) that is turned into widgets, and added `App::ui_snapshot()` and `loom-app --dump-ui`, which read the same model. The question followed: should the app build a separate widget tree of its own, so every part of the UI has a testable description?

A second, related problem is **widget identity**. Iced rebuilds the `Element` tree on every update and keeps widget state (the canvas camera, a scroll offset, an open pick list) in a separate `widget::Tree`. When the structure of `view` changes, state can land on the wrong widget or be lost. The app has a few workarounds for this, written before we had looked closely at how Iced matches state.

What we confirmed in the Iced 0.14 source (details and line numbers in the research snapshot):

- `Tree::diff` keeps a widget's state when the new widget has the same `Tag` (the `TypeId` of its state type). Otherwise it recreates the whole subtree.
- `Column`, `Row`, and `Stack` are stateless and diff their children by index. `Container` is transparent: it forwards tag, state, and children to its content.
- `keyed_column` stores keys and, when they change, finds the first changed index and inserts or removes one contiguous run of children there. It does not follow a moved item, and when the number of items is the same it diffs by index even if the keys differ. There is no keyed row.
- `widget::Id` is never read by `Tree::diff`. It addresses a widget for an `Operation` (focus, scroll, or a custom operation) run as a `Task`.

So Iced already has what a separate widget tree would give us: an immutable description rebuilt each update, and a retained tree of state matched against it. The canvas `Scene` is our retained render layer on top.

## Options considered

### How tests and tools see the UI

1. **Read the Iced widget tree only** (the `Ui` harness in `ui_test.rs`). Good for "this click reaches this widget". Poor for "this row shows 440 Hz": the test has to search text and knows nothing about meaning.
2. **A full UI description layer of our own**, independent of Iced, from which every panel is drawn. One model for every panel and every future front end. A large framework to build before we know what a second front end needs, and it would duplicate Iced's `Element` and `Tree`.
3. **A plain-data view model per panel, added where tests need one.** The app builds a small struct from its state. `App::view` turns it into widgets, and `App::ui_snapshot()` returns it. The inspector already works this way. Cheap, and grows one panel at a time. Panels without a model can still be tested through the `Ui` harness.

### How widgets keep their identity

A. **Positional workarounds**, as today: keep the structure of `view` stable by hand, and pass counters into stateful widgets.

B. **Iced's own tools where they fit:** `keyed_column` for lists whose items are inserted or removed, `widget::Id` with operations for focus, scrolling, and commands to a stateful widget. Keep a workaround only where Iced has no replacement.

C. **Our own identity layer**, such as keys checked by a wrapper widget. More control, but it fights Iced instead of using it.

## Decision

1. **View models, one panel at a time.** A panel that needs tests gets a plain-data view model, built from app state by a function on `App` (for the inspector, `App::inspector_model`). `App::view` and `App::ui_snapshot` both read that model, so a test of the snapshot checks what the window draws. Models are plain structs and enums with `Serialize`, hold display text and positions rather than widgets, and live next to their panel in `loom-app`. We do not build a framework or a generic UI description.
2. **Identity through Iced's own tools.** Use `keyed_column` for a list of stateful rows whose items can be added or removed, keyed by a stable ID (for example `AssignmentId`). Use `widget::Id` with operations for focus, scrolling, and queries. Keep the structure of `view` stable around stateful widgets. Do not add our own identity layer.
3. **Revisit when a second front end exists.** A plugin editor, or a web or WASM UI, would need the same models without Iced. The trigger to revisit is starting work on one. Then the view models move into a crate with no Iced dependency, and each front end turns them into its own widgets.

### The current workarounds

| Workaround in `crates/loom-app/src/main.rs` | What it does | Iced replacement | Verdict |
|---|---|---|---|
| `view` always wraps the window in `stack![main]`, even when the module browser is closed | Keeps the canvas at the same path in the state tree, so its camera survives opening the browser | None. Identity is position within the parent, and Iced has no way to move state to a new parent. `keyed_column` only orders siblings in one column. | **Stays.** It is the correct idiom: keep the structure stable and vary the children. |
| `frame_key`, a counter passed to `PatchCanvas::frame_key` | When it changes, the canvas frames the whole patch on its next redraw. It survives the app being rebuilt after loading a patch, so it must keep counting up. | A custom `Operation` addressed by a canvas `widget::Id`, run with `iced::advanced::widget::operate` (a re-export of `iced_runtime::task::widget`). The canvas can hand its state, or a view of it, to `Operation::custom`. | **Stays for now.** It is not an identity workaround but a one-shot command into widget state. The counter needs no task round trip, it waits until the canvas has a size, and it survives the app being rebuilt. Reconsider if the canvas gets a third command of this kind. |
| `browser_key`, a counter passed to `PatchCanvas::browser_key` | When it changes, the canvas sends `CanvasEvent::OpenBrowser` at the centre of its view, because only the canvas knows its camera | The canvas `operate` already reports its bounds and camera to `Operation::custom`. The app could query them with an operation and open the browser itself. | **May move.** A real replacement exists. Change it when this code is next touched, not as a task of its own. |

## Outcomes we want

We will know the decision works when:

- App tests for a modelled panel compare snapshot fields ("the cutoff row shows 1.2 kHz"), not text found in the widget tree.
- `loom-app --dump-ui` shows every modelled panel, so a person or an agent can check a patch without a window.
- The widget code of a modelled panel makes no decisions. To review what a panel shows, you read its model builder.
- Opening and closing the browser, or adding and removing a modulation assignment, never resets or moves widget state such as the canvas camera, a slider drag, or an open pick list.
- `loom-app` holds no UI framework code of its own, only models and the functions that draw them.
- Model types use no Iced types (true today), so moving them to their own crate stays a simple move.

## Consequences

- Easier: tests check meaning ("the cutoff row shows 1.2 kHz") instead of searching widget text. `--dump-ui` gives people and agents the same view without a window. The pattern is plain Rust with no new dependencies.
- Easier: when a second front end comes, the models are already separated from Iced. Moving them to their own crate is mostly a move.
- Harder: each modelled panel has two steps (state to model, model to widgets). A field shown in the window but missing from the model is not tested. Keep the widget step thin, with no logic beyond layout.
- Limits to remember: `keyed_column` does not keep state when rows swap places, and a changed key does not reset state when the length stays the same. If we ever need either, write a small widget that diffs by key, or rebuild the list with fresh state.
- Must do: when a panel gains a model, document it as the inspector is documented (see [the architecture note](../dev/architecture.md#view-models-for-app-panels)).

## Prior art

Other declarative toolkits make the same split between a description rebuilt on each change and a retained tree of state, and also match state by type and position, with keys for lists. The [research snapshot](../research/2026-10-ui-view-models-and-identity.md) compares them, with sources.
