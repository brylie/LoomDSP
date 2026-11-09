# ADR-0040: Our own UI layer on winit, wgpu, and AccessKit

- **Status:** Accepted
- **Date:** 2026-11-09
- **Related requirements:** NFR-UX-01, NFR-UX-06, NFR-PERF-04, FR-SDK-06, OQ-13, OQ-14
- **Specification:** [UI layer](../design/ui-layer.md)
- **Supersedes:** [ADR-0009](0009-ui-stack-iced.md) for the window, runtime, and widgets (the canvas stays with [ADR-0010](0010-own-patch-canvas.md)), and [ADR-0028](0028-app-accessibility-waits-for-iced.md)
- **Background:** [Our own UI stack instead of Iced](../research/2026-11-own-ui-stack-tradeoffs.md), [the accessibility spike](../research/2026-11-a11y-beside-iced-spike.md), [how a UI layer decides what changed](../research/2026-11-ui-change-propagation.md)
- **Plan and issue:** [Leaving Iced, step by step](../dev/leaving-iced-plan.md), [#683](https://github.com/brylie/LoomDSP/issues/683)

## Context

ADR-0009 chose Iced for the app's window, widgets, and text. ADR-0028 then deferred the app's accessibility tree until Iced supports AccessKit. Three things have changed since.

1. **Iced's churn and single-author development are stated policy.** Its [philosophy chapter](https://book.iced.rs/philosophy.html) says breaking changes are welcome and that features are built by the author alone. So OQ-14 will not settle, and ADR-0028 waits on one person's priorities.
2. **The accessibility spike worked.** An AccessKit tree built from `App::ui_snapshot` attached to Iced's window without a fork. VoiceOver reached tiles and turned knobs, and every action became an ordinary, undoable edit. The spike also found that the tree must follow focus, layers, and node identity that Iced keeps to itself.
3. **We already own the hardest part.** The patch canvas is ours (ADR-0010). The plain-data view models (ADR-0021) already describe what the user sees, and the tests, the control channel, and the accessibility tree read them. Iced provides the window, the loop, layout, about twenty widget types, and text.

Forces:

- Screen reader users are blocked today (NFR-UX-06).
- Plugin embedding (OQ-13) needs a shell that can run inside a window the host owns.
- The headless tests, the control channel, and screenshots must keep working at every step.
- The UI layer should be reusable by other audio tools, and publishable later (FR-SDK-06).
- A small team in early alpha. Time spent on a toolkit is time not spent on modules.

## Options considered

1. **Stay on Iced, and wait.** No new work. Accessibility and embedding stay blocked on upstream, under the policy above. Each upgrade may break the harness, the screenshot path, and the control channel.
2. **Fork Iced and add AccessKit.** Reaches screen readers soonest on familiar code. We would merge every upstream release into a fork whose author does not take outside features, and own its bugs. ADR-0028 already judged this too costly.
3. **Vizia.** Built for audio plugins, with a `baseview` backend for host windows, and AccessKit. Its model and event dispatch are its own, so the view models, the harness, and the control channel would be rewritten against a second framework. Whether AccessKit works under `baseview` is unconfirmed †.
4. **Masonry** (the widget layer under Xilem). A retained widget tree with focus, layout, and AccessKit built in, on `winit` and `wgpu`, with Parley for text and editing †. It covers most of what phase 3 of the plan would build. Against it:
   - It is alpha and changing fast: a new layout system and a new rendering abstraction in early 2026 †. That is the same class of risk as OQ-14, though from a project that does take outside contributions †.
   - It owns the widget tree, focus, and the accessibility tree. Our view models would again be a second description beside a toolkit's own, which is the gap ADR-0028 and the spike worked around.
   - It draws through Vello †, while the canvas has its own `wgpu` pipeline. The two would need compositing, and Vello's GPU needs inside plugin hosts are unmeasured.
   - Embedding in a host window is not documented †.
5. **Our own thin layer** on `winit`, `wgpu`, an existing text crate, and AccessKit, built in phases that each leave the app working ([plan](../dev/leaving-iced-plan.md)). We own the loop, layout, focus, the accessibility tree, and a small set of widgets, and we take text shaping, font loading, glyph drawing, and text editing from existing crates. Most work of the five. In return, the accessibility tree, focus, and the test harness are built from the same view models the window draws, and no toolkit sits between us and a host window.

Entries marked † come from web search summaries and the projects' own prose, not from their source (ADR-0018). Check them before relying on them.

## Decision

Option 5. LoomDSP builds its own thin UI layer on `winit`, `wgpu`, an existing text crate, and AccessKit, in the phases of the plan. The layer is two crates, `loom-ui-core` and `loom-ui`, that know nothing about patches, so other tools can use them. How the layer behaves is in the [UI layer specification](../design/ui-layer.md).

Three parts of the decision, each for a reason:

1. **The app keeps the Elm architecture:** one `Message` type, one `update`, and effects as plain values. *Why:* every change goes through one place, which is what makes undo, the control channel, and the headless tests work.
2. **The view models stay the seam** (ADR-0021). Drawing, the accessibility tree, the control channel, and the tests all read them, and nothing reads a widget tree to learn what is on screen. *Why:* one source for the screen and the tree, so the two cannot disagree.
3. **We do not write text.** Shaping, font fallback, line breaking, glyph drawing, and the editing model come from existing crates. *Why:* text is the deepest part of a UI toolkit and the easiest to underestimate.

Stop the work, keep what the finished phases gave, and revisit this decision if any of these happen:

- IME or text editing needs more than a few hundred lines on top of the text crate.
- An AccessKit gap blocks a phase and upstream has not answered.
- Idle CPU or frame rate gets worse and cannot be recovered in the same phase.
- Masonry reaches a stable release with documented embedding in a host window. Then compare it again against the layer as built.

## Consequences

- Easier: accessibility on all three platforms on our own schedule, and a shell that can be adapted for plugin embedding. No toolkit upgrades that break the harness. One rendering model for panels and the canvas.
- Easier: the UI layer can be published for other Rust audio tools (FR-SDK-06).
- Harder: we own layout, focus, overlays, text fields, IME, and every accessibility bug. A tree that disagrees with the screen is worse than none, so the specification ties both to one source and the tests check it.
- Harder: new contributors learn a UI layer documented only in this repository. Phase 4 adds an explainer in `docs/learn/`.
- Done with this ADR: ADR-0009 and ADR-0028 are marked "Superseded by ADR-0040", NFR-UX-06 no longer waits for Iced, and OQ-14 is resolved. ADR-0010 still holds for the canvas; the canvas stays an Iced widget until phase 2 of the plan moves it to the custom element contract.
- Must do: AccessKit's macOS adapter has two gaps the spike found (no accessibility label from the name, no spoken value beside a number). They are reported upstream only with the maintainer's approval. A pinned fork through `[patch.crates-io]` is allowed until a release has the fix.
