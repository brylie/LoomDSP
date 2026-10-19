# UI View Models and Widget Identity in Declarative UI Toolkits

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-10-19. Iced claims were checked against the Iced 0.14 source on this date.
- **Used by:** [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md)

This snapshot compares how five declarative UI toolkits answer two questions that came up while testing the desktop app (issue #206, PR #219):

1. **A plain-data description of the UI.** Is there a layer of plain values between the app's state and the widgets? Who builds it, and who can read it?
2. **Widget identity across rebuilds.** When the UI is described again after each change, how does the toolkit decide that a new description is "the same widget" as an old one, so that its state (focus, scroll offset, an open menu, a camera) survives?

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## How to use this document

- It describes other toolkits in our own words, as facts about how they work. It copies no text or code from their documentation.
- It holds evidence, not decisions. The decision is in ADR-0021.
- Entries marked † come from the author's general knowledge and were not checked against a source page on the snapshot date. The Iced entry was checked line by line against the source in `~/.cargo/registry` (crate versions below).

## Method

For Iced we read `iced_core` 0.14.0 (`src/widget.rs`, `src/widget/tree.rs`, `src/widget/id.rs`, `src/widget/operation.rs`), `iced_widget` 0.14.2 (`src/keyed/column.rs`, `src/column.rs`, `src/stack.rs`, `src/container.rs`, `src/helpers.rs`), and `iced_runtime` 0.14.0 (`src/user_interface.rs`, `src/widget/operation.rs`, `src/task.rs`). For the others we summarised the published model from memory and link the official docs for checking. We did not read any other toolkit's source code.

## Findings

1. **Every toolkit already separates description from state.** Each one rebuilds a cheap, immutable description on every change and keeps the long-lived state in a second structure that the toolkit owns. Iced is no exception: `Element` is the description and `widget::Tree` holds the state. A second, custom "widget tree" on top of Iced would duplicate what Iced does.
2. **Plain-data view models are an app pattern, not a toolkit feature.** No toolkit here requires a serialisable layer between app state and widgets. Where one exists (Elm's model, a SwiftUI view model, a Flutter "view model" class), the app writes it for its own reasons, usually testing or sharing logic between platforms.
3. **Identity is structural by default everywhere.** "Same type at the same place in the parent" means "same widget". Changing the structure around a widget (wrapping it, adding a branch before it) changes its identity and drops its state.
4. **Explicit identity is opt-in and local.** Keys disambiguate siblings in a list. Only Flutter (GlobalKey) lets a subtree keep its state when it moves to another parent †.
5. **Iced's keyed list is weaker than React's or Flutter's.** It handles one run of inserted or removed items, not arbitrary reordering (details below).
6. **Iced's `widget::Id` is not used for diffing.** It addresses widgets for operations (focus, scroll, custom queries). It does not affect which state a widget gets.

## Toolkits

### Elm architecture, and Iced 0.14

**Elm** † has one model, a pure `view` function from model to a virtual DOM, and messages that update the model. The view output is plain data, so it can be compared and tested, but it is HTML, not a domain model. Components keep no private state; all state lives in the model, except what the browser keeps in the DOM (focus, scroll, text selection). Lists can use a keyed node so the virtual DOM diff matches children by key. Sources: [Elm guide, The Elm Architecture](https://guide.elm-lang.org/architecture/), [Html.Keyed](https://package.elm-lang.org/packages/elm/html/latest/Html-Keyed).

**Iced** follows the same loop (`update`, `view`), but widgets may keep private state. What we read in the 0.14 source:

- **Description and state are separate.** `view` returns an `Element` tree, built fresh each time. The runtime keeps a `Tree` (`iced_core/src/widget/tree.rs`) across frames. `UserInterface::build` takes the previous `Tree` from a `Cache` and calls `state.diff(root)` against the new root (`iced_runtime/src/user_interface.rs`, line 102).
- **State is matched by tag, then by position.** `Tree::diff` compares the stored `Tag` with the new widget's `tag()`. A `Tag` is the `TypeId` of the widget's state type. If the tags match, the widget's own `diff` reconciles its children. If not, the whole subtree is recreated with fresh state.
- **Layout widgets are stateless and positional.** The default `Widget::tag` is `Tag::stateless()` (`iced_core/src/widget.rs`, line 78). `Column` and `Stack` call `tree.diff_children(&self.children)` (`column.rs` line 207, `stack.rs` line 153). `diff_children_custom` truncates or extends the child list at the end and diffs the rest pair by pair, by index. So child 2 always gets the state that child 2 had last frame.
- **Containers are transparent.** `Container` forwards `tag`, `state`, `children`, and `diff` to its content (`container.rs`, lines 236 to 251). Wrapping a widget in a container does not add a level to the state tree.
- **Keyed column.** `keyed_column` (`helpers.rs` line 510, `keyed/column.rs`) stores the keys in its state. Its `diff` calls `diff_children_custom_with_search` with a "maybe changed" test that compares old and new keys at each index. That function finds the first index whose key changed and removes or inserts one contiguous run of children there. It does not search for a moved key elsewhere in the list. So it keeps state right when items are added or removed in one place, but if two items swap places, each position keeps its old state and gets the other item's widget. There is no keyed row in 0.14.
- **`widget::Id` is for operations.** `Id` is a static or owned string, or a unique counter (`iced_core/src/widget/id.rs`). `Tree::diff` never reads it. Widgets that accept one (`container`, `scrollable`, `text_input`, and others) report it to an `Operation`, which the app runs as a `Task`: `focus(id)`, `scroll_to(id, offset)`, `snap_to(id, offset)`, and so on (`iced_runtime/src/widget/operation.rs`), or any custom `Operation` through `task::widget` (`iced_runtime/src/task.rs`, line 423), which the `iced` crate re-exports as `iced::advanced::widget::operate`. A custom widget calls `Operation::custom` with any value it chooses, as `&mut dyn Any`, and its `Id` if it has one. Our canvas passes its bounds and camera this way.
- **Laziness.** `iced_widget` has a `lazy` widget behind a feature flag, which skips rebuilding a subtree while its input is unchanged. LoomDSP does not enable it.

What this means for us: our app is the Elm loop plus widget state in `Tree`. The canvas `Scene` is already a plain-data model of the patch, and the canvas widget keeps its camera in its `Tree` state.

### React

React † describes the UI as plain element objects (type, props, children), produced again on each render. A separate internal tree (the "fiber" tree) holds component state and hooks. Reconciliation rules:

- Same type at the same position in the parent: state is kept and props are updated.
- Different type: the old subtree is unmounted and a new one is mounted with fresh state.
- Among siblings, a `key` replaces position as identity, so items can be reordered, inserted, and removed and each keeps its state. React matches keys anywhere in the sibling list, not just one run.
- Changing a component's `key` on purpose is the usual way to reset its state.

The element objects are plain data, but they describe components, not the domain. Apps that want a testable, toolkit-free description write their own selectors or view models. Sources: [Preserving and resetting state](https://react.dev/learn/preserving-and-resetting-state), [Rendering lists](https://react.dev/learn/rendering-lists).

### Flutter

Flutter † has three trees:

- **Widgets:** immutable configuration objects, rebuilt often and cheap to create.
- **Elements:** the long-lived instances that give a widget its place in the tree. A `StatefulWidget`'s `State` object hangs off its element.
- **Render objects:** layout, painting, and hit testing, updated in place by the elements.

On rebuild an element is kept when the new widget has the same runtime type and the same key (`Widget.canUpdate`). Keys come in two kinds. Local keys (`ValueKey`, `ObjectKey`, `UniqueKey`) only need to be unique among siblings, and the child-list update matches them across the whole list, so reordering keeps state. A `GlobalKey` is unique in the whole app: a subtree with one keeps its element and state even when it moves to a new parent, and the key gives access to that state from outside. Global keys cost more and are used sparingly.

Flutter itself does not define a plain-data view model. Popular state libraries (Provider, Riverpod, Bloc) encourage one, mainly for testing. Sources: [Widget](https://api.flutter.dev/flutter/widgets/Widget-class.html), [Key](https://api.flutter.dev/flutter/foundation/Key-class.html), [GlobalKey](https://api.flutter.dev/flutter/widgets/GlobalKey-class.html).

### SwiftUI

SwiftUI † views are value types that describe the UI. The framework stores state (`@State`, `@StateObject`) outside them, keyed by the view's identity. It names two kinds of identity:

- **Structural identity:** the view's type and its position in the view hierarchy. The result builder encodes structure in the type: an `if`/`else` produces a conditional type whose two branches are different views, so toggling the condition resets the state below it. The usual advice is to keep the structure stable and vary a modifier (opacity, a parameter) instead of switching branches. Type-erasing with `AnyView` hides the structure and makes identity harder to keep.
- **Explicit identity:** `ForEach` over `Identifiable` data, or the `.id(_:)` modifier. A new `.id` value gives a new view with fresh state, which is the deliberate way to reset.

Many SwiftUI apps put an observable view model between the domain and the views, which the framework supports but does not require. Sources: [Demystify SwiftUI, WWDC 2021](https://developer.apple.com/videos/play/wwdc2021/10022/), [`id(_:)`](https://developer.apple.com/documentation/swiftui/view/id(_:)).

### Xilem

Xilem † is a Rust UI framework (Apache-2.0) with a reactive layer over the Masonry widget toolkit. It has:

- A **view tree**: short-lived, statically typed values rebuilt from app state on each cycle, like SwiftUI's. Each view type knows how to `build` its widget the first time and `rebuild` it by comparing the previous view value with the new one and changing the widget in place.
- A **widget tree** (Masonry): long-lived, retained widgets that do layout, paint, and accessibility.
- **View state**: per-view data that the view layer keeps beside the widgets between rebuilds.
- **Id paths**: each view gets a `ViewId`, and the path of ids from the root addresses it. Events from a widget come back along its id path to the view that made it, which turns them into app actions.

Identity is mostly structural through the static types, as in SwiftUI. Sources: [Xilem repository](https://github.com/linebender/xilem), [Xilem: an architecture for UI in Rust](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html).

## Comparison

| | Plain description rebuilt each change | Long-lived state lives in | Default identity | Explicit identity | Moves between parents keep state |
|---|---|---|---|---|---|
| Elm † | Virtual DOM (HTML) | App model, plus the DOM | Position | Keyed node, matched across siblings | No |
| Iced 0.14 | `Element` tree | `widget::Tree` | State type, then position | `keyed_column`: one insert or remove run; no reorder | No |
| React † | Element objects | Fiber tree | Type, then position | `key`, matched across siblings | No |
| Flutter † | Widget tree | Element tree (`State`) | Runtime type, then position | Local keys across siblings; `GlobalKey` app-wide | Yes, with `GlobalKey` |
| SwiftUI † | View values | Framework storage by identity | Structure (type and position) | `ForEach` ids, `.id(_:)` | No |
| Xilem † | View tree | Widget tree plus view state | Structure (static types) | Id paths route events; not checked for keyed lists | Not checked |

In every toolkit, a plain-data model of the domain (what the user sees, in the app's own terms) is something the app adds when it needs one.
