# ADR-0028: App accessibility waits for AccessKit support in Iced

- **Status:** Deferred
- **Date:** 2026-10-28
- **Related requirements:** NFR-UX-06 (new), NFR-UX-01, NFR-UX-02, NFR-UX-03
- **Specification:** none yet. A spec is written when this ADR is taken up again.

## Context

The desktop app exposes no accessibility tree. A screen reader sees nothing in the window: not the toolbar, the inspector, the module browser, or the canvas. Desktop automation tools see only the window's close, minimise, and full-screen buttons, so they can click by pixel but find no control by name ([#190](https://github.com/brylie/LoomDSP/issues/190)).

The goal serves two groups:

- **People.** Blind and low-vision musicians, and people who cannot use a mouse, could work the app with a screen reader or the keyboard. Few modular tools allow this.
- **Testing.** The accessibility tree is what UI automation uses to find and press controls by name.

What the stack offers, checked on 2026-10-28:

- **Iced 0.14,** the version we use (ADR-0009), has no accessibility support. None of its crates depend on AccessKit, the cross-platform accessibility library for Rust UIs.
- **AccessKit** has an adapter for winit (`accesskit_winit`), which Iced runs on, and adapters for Windows, macOS, and Linux. Its design needs only a stable ID for each element, which our keyed widget identity already gives (ADR-0021).
- **Outside bridges** exist that build an accessibility tree beside an Iced app from their own description of the widgets. They do not read Iced's widget tree, so they can drift from what is drawn.

The testing goal is already met in another way: the control channel ([control-channel.md](../design/control-channel.md)) finds controls by name, presses them through the same messages as the UI, and reads the UI snapshot. Only people who need a screen reader or the keyboard are still left out.

## Options considered

1. **Build now, on a fork of Iced with AccessKit.** Reaches screen readers soonest. We would maintain a fork of a GUI toolkit, merge each upstream release into it, and own its bugs. Too costly for a small team in the early alpha.
2. **Build now, with an outside bridge.** No fork, but the tree is a second description of the UI, kept in step by hand. A tree that disagrees with the screen is worse for a screen reader user than none, because it says things that are not true.
3. **Wait for Iced, and prepare.** Adopt AccessKit through Iced when Iced supports it. Until then, keep what the user reads in the plain-data view models (ADR-0021), so the names and values an accessibility tree needs already exist in one place. The control channel covers automation.
4. **Out of scope.** Keep only exported panels in scope (NFR-UX-02). Leaves the app closed to blind musicians for good, against product principle 11 (LoomDSP is also a way to learn).

## Decision

Deferred: the app's accessibility tree waits for AccessKit support in Iced (option 3). Each view model that the app gains holds the accessible name, value, and state of what it shows, in our own words, so the tree can be built from it. NFR-UX-06 records the requirement for the desktop app.

Revisit this ADR at each Iced upgrade, and when Iced's release notes or roadmap name AccessKit support. Then write a specification and an ADR that supersedes this one.

## Consequences

- No new dependency now, and no fork to maintain.
- Screen reader users still cannot use the desktop app. The requirement says so, so it is not forgotten.
- New view models (ADR-0021) must carry the text a screen reader would read: a label, a value with its unit, and whether the control is enabled. The inspector model does this already.
- The canvas is one custom widget. When the time comes it must report one accessibility node per tile, port, and control, in the focus order of NFR-UX-01 ([#101](https://github.com/brylie/LoomDSP/issues/101)). Keyboard focus on the canvas (#101) is useful on its own and does not wait for this ADR.
- Automation by name keeps using the control channel.
