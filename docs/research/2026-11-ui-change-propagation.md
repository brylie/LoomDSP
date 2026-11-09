# How a UI Layer Decides What Changed

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-09. Repository facts were checked at commit `d0539429`.
- **Related decisions:** [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md) (plain-data view models), [ADR-0040](../adr/0040-own-ui-layer.md) (our own UI layer)
- **Related:** [UI layer specification](../design/ui-layer.md), [the plan for leaving Iced](../dev/leaving-iced-plan.md), [#683](https://github.com/brylie/LoomDSP/issues/683)

This snapshot collects what three articles and two neighbouring projects teach about one question: how a UI layer knows what changed, and what it rebuilds as a result. It feeds the "Change and cost model" and "Identity" sections of the UI layer specification. It decides nothing.

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## How to use this document

- Entries marked † come from web search summaries or the articles themselves. They were not checked against any project's source or release notes. Check them before relying on them.
- Only prose was read: blog posts, READMEs, and release notes. No other project's source code was read (ADR-0018). GPUI, Xilem, Masonry, and Parley are not LoomDSP dependencies, so their source stays off limits unless one of them is accepted under ADR-0006.

## Sources

1. Zortax, [A critique of GPUI](https://blog.zortax.de/posts/a-critique-of-gpui) (2026). The author builds a competing framework based on signals, and the benchmarks come from his own harness, not reviewed by GPUI's maintainers. Read the numbers as examples.
2. Raph Levien, [Towards a unified theory of reactive UI](https://raphlinus.github.io/ui/druid/2019/11/22/reactive-ui.html) (November 2019).
3. Raph Levien, [Xilem: an architecture for UI in Rust](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html) (May 2022). Xilem has changed a great deal since. Read it for its ideas, not as a description of Xilem today.
4. Linebender, [Linebender in 2026 Q1](https://linebender.org/blog/tmil-25/), and the [Parley releases](https://github.com/linebender/parley/releases) †.

## The question every UI layer must answer

The GPUI critique puts it in one line: every UI architecture has to answer "what changed, and who depends on it?" If the framework cannot answer, each application has to, by hand, at every call site.

Rust makes the question harder than in garbage-collected languages. A write through `&mut` leaves no trace a framework can see. So a Rust UI layer has three ways to learn what changed:

| Way | How it knows | Cost |
|---|---|---|
| **Notify by hand** | Code that changes state calls `notify` | A missed call is a silent bug. The framework cannot tell an equal write from a real change. |
| **Compare** | Rebuild a description and compare it with the last one | Comparing costs time in proportion to what is compared. Cheap if the unit is small or the comparison is a pointer check. |
| **Track reads** (signals) | Each value records who read it, and wakes them on write | Precise, but a second state model to learn, and memory for the graph of readers. |

LoomDSP already has a fourth property that helps: every change goes through one `Message` and one `App::update`. So the app always knows **when** something may have changed, and only needs to learn **what**.

## What the GPUI critique found †

- In GPUI, `notify` carries only the entity, not the field. The framework cannot see whether anything really changed. Zed's own notes call a missed notification an application bug.
- The cost of an update follows the size of the tree, not the size of the change. With 3,000 cached rows, one label change took about 8.4 ms. A hover, a keystroke, and a write of an equal value cost about the same.
- A hand-written equality guard cut an equal write from 7.9 ms to 0.9 µs. The point is not the number. It is that every call site had to remember the guard.
- Zed feels fast partly because its window is simple: a few buttons, a sidebar, and an editor. The editor is a custom element that bypasses GPUI's layout. Speed on a simple window does not show that a design scales to a complex one.
- Retaining subtrees between frames helped little in Zed's own measurements, because the state did not line up with the retained units. In a benchmark where many panels read one shared model, retaining made the frame slower.

The general lessons the author draws:

1. **A cost built into the API cannot be fixed later.** A linear cost inside the engine can be optimised behind the same API. A contract like "call `view` for the whole window on every change" cannot.
2. **The data must match the reads.** Fine-grained boundaries do nothing if every boundary reads one large shared struct.
3. **Measure creation cost and memory,** not only update cost.
4. **Use effects sparingly.** Chains of effects that trigger more updates are hard to predict.

## What Levien's articles add

### A UI is a pipeline of trees

Levien describes a UI as a chain of tree-to-tree stages: app data, then widgets, then layout, then drawing. Each stage either **keeps** its tree between frames or **rebuilds** it each time. Keeping costs memory and code. Rebuilding costs time. Each stage chooses for itself.

The question to ask of any design is therefore: for each stage, is its tree kept or rebuilt, and where does the state of that stage live? (A splitter's position, for example, belongs to the layout stage, not to the app.)

### Skip what did not change, by comparing

Levien's preferred approach is to rebuild from the root and skip any subtree whose input is equal to last time. In the Xilem post this is a memoize node: it takes data that can be cloned and compared, and runs its builder only when the data changed. Wrapping large data in `Arc` and comparing with `Arc::ptr_eq` makes the check a pointer comparison.

His warning against the other direction comes from his own text editor, xi-editor: computing explicit deltas by hand, for each kind of change, made the code much more complex. That is the cost a hand-written change system tends to grow into.

### Identity

He lists three ways to identify a node from one frame to the next:

1. **Object references.** Awkward in Rust, and they tie the tree to one data structure.
2. **Key paths:** the route from the root, by position or by explicit key.
3. **Unique IDs,** usually integers.

Xilem uses an **id path**, and routes events by it: an event carries the path of the node it came from, and each level passes it to the child on that path.

### Events the framework keeps to itself

In Xilem, platform events such as hover and resize are handled inside the framework. Only meaningful events, such as a click, reach application logic. This is the same lesson as GPUI's hover cost, seen from the other side.

### The Elm architecture

Levien finds that the Elm architecture (Iced, Vizia) works well, but needs a long global message type and handles components less cleanly. Elm's `Html.map`, Xilem's `Adapt`, and lenses are three ways to let a component use its own message or state type inside a parent's.

### Escape hatches and accessibility

- Real frameworks always need escape hatches from the pure pipeline. Build them in from the start.
- In 2022 Levien expected a retained widget tree to suit AccessKit, but had not tried it.

### Async

In Xilem, a view node that holds a future polls it itself, which Levien calls a tiny executor of its own, and says needs much more work. A design where effects are plain values, run by the shell, avoids that problem and lets a test harness see every effect.

## Neighbouring projects on the same foundations †

Xilem and its widget layer, Masonry, are built on the same kinds of crates as the plan for leaving Iced:

| Layer | Xilem and Masonry | The plan for leaving Iced |
|---|---|---|
| Window and events | `winit` | `winit` |
| GPU | `wgpu`, drawn through Vello, now behind an abstraction called imaging | `wgpu`, drawn by our own instanced shapes |
| Text | Parley and Fontique | `cosmic-text` |
| Accessibility | AccessKit | AccessKit |

What the sources say about each:

- **Masonry** is a retained widget tree with accessibility built in. In the first quarter of 2026 it gained a new layout system and many widgets (switch, radio buttons, step input, split, collapse panel, and others), and moved its rendering behind a new abstraction. Xilem's crate page still calls it alpha software.
- **Parley** is a rich text layout engine. Its README says it includes text selection and editing. Its `PlainEditor` tracks an IME composition range. Its release notes list support for AccessKit's text properties, which a screen reader needs to read and move through the text in a field.

What this means for LoomDSP:

1. **The plan's foundations are the common ones.** Two independent projects reached the same set: `winit`, `wgpu`, a Rust text stack, and AccessKit. That lowers the risk of the plan's choice of crates.
2. **Masonry is a real option, not only a source of ideas.** It offers what phase 3 would build: widgets, focus, layout, and an accessibility tree. It is also alpha and changing fast, which is the risk that made OQ-14 a question for Iced. ADR-0040 weighs it.
3. **Parley is a real candidate for text.** Accessible text fields and IME are the two parts of phase 3 the plan names as the most likely to overrun. A text crate that already reports AccessKit text properties may save more work than staying on the crate Iced uses. The UI layer specification lists this as an open point.

## What this means for the UI layer specification

1. **Choose the unit of rebuild on purpose.** The per-panel view models of ADR-0021 are a natural unit. The framework compares each panel's model with the last one and skips the panel when they are equal.
2. **Put the comparison in the framework,** not at each call site.
3. **Keep transient widget state out of `update`.** Hover, pressed, focus rings, the caret, and scroll offsets redraw a widget but do not rebuild panels.
4. **Build panel models from the narrowest input** that gives what they show.
5. **One identity scheme** for widget state, focus, accessibility node IDs, event routing, and the control channel's `locate`.
6. **Effects are plain values.** One message gives one update. An effect never runs `update` again in the same turn.
7. **A supported escape hatch** for elements like the patch canvas.
8. **Keep the Elm architecture, and write down why.** For LoomDSP, one message path is what makes undo, the control channel, and the headless tests work. A later contributor who "fixes" the long message enum could break all three.
9. **Measure what small windows hide:** an equal message, hover over a busy panel, and one change in a large list.

## Not proposed

- **Signals or another read-tracking system.** They answer the question most precisely, but add a second state model, and ADR-0021 chose not to build a generic UI layer. Comparing panel models gives most of the benefit at LoomDSP's size, and a panel can be split into smaller boundaries later without changing the contract.
