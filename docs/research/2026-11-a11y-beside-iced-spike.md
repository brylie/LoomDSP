# Spike: an Accessibility Tree beside Iced, from the View Models

- **Kind:** Research snapshot (spike results). Non-normative.
- **Date:** 2026-11-06. Built on commit `542006b`, Iced 0.14, `accesskit` 0.25.2, `accesskit_macos` 0.27.2, on macOS.
- **Related decisions:** [ADR-0028](../adr/0028-app-accessibility-waits-for-iced.md) (accessibility waits for Iced), [ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md) (plain-data view models)
- **Related:** [Our own UI stack instead of Iced](2026-11-own-ui-stack-tradeoffs.md) (phase 0 of the incremental path), NFR-UX-06
- **Code:** `crates/loom-app/src/a11y.rs`, behind the `a11y-spike` feature (macOS only, off by default)

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## Question

ADR-0028 rejected an accessibility bridge beside Iced (its option 2) because the tree would be "a second description of the UI, kept in step by hand", and a tree that disagrees with the screen is worse than none.

The spike asks whether that objection still holds when the tree is not hand-kept, but built from the same plain-data models the window draws from (`App::ui_snapshot`), and when its actions go through the same messages as the pointer. In short:

1. Can AccessKit attach to the window Iced 0.14 creates, without a fork of Iced?
2. Can the tree come from the snapshot alone, with correct names, values, and positions?
3. Does a screen reader action reach `App::update` as an ordinary edit?

## What was built

About 430 lines in `a11y.rs`, plus 18 lines of wiring in `lib.rs` and `subscriptions.rs`, all behind the feature.

- **Attach.** At start, a window task (`iced::window::run`) hands over the window's raw handle. AccessKit's `SubclassingAdapter` attaches to its `NSView`. The adapter is not `Send`, so it lives in a thread local on the main thread, where Iced runs `update`.
- **Tree.** Root window, the status line, one group per tile, and one node per visible panel widget. Knobs, faders, and numbers are sliders (0 to 1, with the value as the screen shows it, such as "800 Hz"). Toggles are switches, buttons are buttons, meters are meters. The rest is read only for now. Positions come from the snapshot's world rectangles and the canvas camera, found with the same kind of widget operation the control channel uses.
- **Actions.** The adapter's callbacks send events through a channel that a subscription reads, so every change still goes through `App::update`. Increment, Decrement, and SetValue become `CanvasEvent::SetControl` followed by `SliderReleased` (one undo step, as the control channel's `set`). Click on a switch or button becomes the same canvas event as a click. Focus on a tile selects it.
- **Refresh.** While a screen reader listens, a 500 ms timer rebuilds the whole tree. After an action, the tree is rebuilt at once.

## Results

| Check | How | Result |
|---|---|---|
| Adapter attaches to Iced's window | Release build, launched with `--fake-audio` | **Yes.** No panic, no error, after the window was already shown. AccessKit's docs ask for attachment before the window is first shown; it worked anyway here. |
| Controls exposed with names | Read the window's accessibility tree from another process (desktop automation, which uses the macOS accessibility API) | **Yes.** All 20 knobs of `subtractive.ron` appear as `AXSlider` with their labels ("Cutoff", "Resonance", "Attack", ...). Toggles appear as `AXSwitch`, the Reset button as `AXButton`. Before the spike, only the window's close, minimise, and full-screen buttons were visible. |
| Positions match the drawing | Compared reported frames with a screenshot | **Yes.** "Cutoff" and "Resonance" lie on the SVF tile, "Run" on the Transport tile, and so on. Scale factor handled. |
| Tiles group their controls | Element path reported by the automation tool | **Yes.** The Run switch was reached through `AXWindow > AXGroup > AXGroup`. |
| Action reaches `update` as an ordinary edit | `AXPress` on the Transport's Run switch, from outside the process | **Yes.** Run turned off, the title gained `*` (unsaved), Undo became available, and Undo restored it. |
| Increment and SetValue | Unit tests in `a11y/tests.rs` through `App::update` | **Yes**, as one undo step, clamped to 0..1. Not checked from outside the process (below). |
| Default build unchanged | `cargo clippy -p loom-app --all-targets -D warnings` with and without the feature; `cargo deny check` | Clean. `accesskit` and `accesskit_macos` are MIT OR Apache-2.0. |

### VoiceOver session (same day, by the maintainer)

A first try with VoiceOver on, with action logging added (`a11y: ...` lines on stderr):

1. **The name was not read.** On the Phase tile's Tune knob, VoiceOver said the value and the role ("0.000 st, circular slider"), but not "Tune". AccessKit's macOS adapter gives the name to macOS only as the accessibility title, and never sets the accessibility label, which Apple documents as a control's name. VoiceOver seems to skip the title on a slider. `LOOM_A11Y_NAME=value` and `=help` try two workarounds; neither has been listened to yet.
2. **The knob could not be changed while its value was text.** VoiceOver seemed to move the value and then fall back, and the log showed that the app received no action at all. AccessKit reports the text value in place of the numeric one when both are set, so VoiceOver had no number to step.
3. **With a number-only value (`LOOM_A11Y_VALUE=number`), it works.** VoiceOver sent about 300 `SetValue` requests while the knob was turned up and down. Every one became a `SetControl` on the right module and widget. The knob reached 0 and 1, and values stayed in range.

What the log shows about how VoiceOver adjusts a slider:

- It uses `SetValue` with a number it computes itself, not `Increment` or `Decrement`.
- It steps by 1% of the range (0.01), and ignores the node's step (0.05).
- When a key is held, the step doubles on each repeat (0.01, 0.02, 0.04, 0.08, 0.16, ...), so a held key crosses most of the range in under a second.
- Requests often arrive in bursts of three to eight before the app's reply, so VoiceOver keeps its own running value.

What this means for a real version:

- **Spoken value against stepping.** With a number, VoiceOver can step the knob but cannot say the unit ("800 Hz"). With text, it says the unit but cannot step. Native AppKit sliders solve this with a separate spoken value description. AccessKit's macOS adapter does not expose one. Both this and the missing name are gaps in AccessKit's macOS adapter, and fixing them upstream would help every Rust app that uses it.
- **Undo.** Each request is its own undo step: one VoiceOver session made about 300. Screen reader edits should group like arrow-key edits (`docs/design/undo.md`), which close a group 500 ms after the last step.
- **Step size.** 1% steps are fine for a continuous knob. Stepped parameters (octave, note value, a mode switch) need their own step, or a role that VoiceOver steps one option at a time.

### Not checked

- **The name workarounds** (`LOOM_A11Y_NAME`), and how a number-only value sounds.
- **Values and Increment from outside.** Scripting the accessibility API from a shell needs the "assistive access" permission in System Settings, which was not granted. In-process tests cover the logic; the platform side is unverified.
- **Windows and Linux.** AccessKit has adapters for both. The Windows adapter also attaches to an existing window; the Linux one runs beside it. Not tried.

## Findings

1. **The ADR-0028 objection is much weaker in this form.** The tree is a second projection of `ui_snapshot`, not a second description written by hand. A new control appears in the tree as soon as it appears in the snapshot. The snapshot is already what the app's tests check, so drift between screen and tree would show up as a failing snapshot test, not silently.
2. **No fork of Iced was needed.** Iced 0.14 gives the raw window handle through `window::run`, and the scale factor through `window::scale_factor`. That is enough for AccessKit's subclassing adapter.
3. **Actions are cheap to add.** Each one maps to a canvas event that already exists, so undo, autosave, MIDI feedback, and the "unsaved" mark work with no extra code.
4. **The view models are not complete enough yet.** The toolbar, the inspector, the module browser, and overlays are not in `ui_snapshot` in a form the tree can use (the Undo button could only be clicked by position). ADR-0028 already asks each view model to carry an accessible name, value, and state; the spike shows which are missing.

## Open problems

1. **Keyboard focus.** The tree always reports focus on the window. Canvas focus (`#101`, the focus enum from #659) and Iced's text-field focus must map to a node, or a screen reader will not follow the keyboard.
2. **Focus selects.** Mapping AccessKit's Focus action on a tile to selection is convenient, but an assistive tool that only inspects may change the selection. In the test, the Transport tile was selected afterwards; the cause was not pinned down.
3. **Refreshing.** The 500 ms rebuild is polling, against "redraw only on visible change" (NFR-PERF-04). A real version would rebuild only after a message that changes the snapshot, send only changed nodes, and stop when the screen reader leaves (`deactivate_accessibility`, not handled yet).
4. **Node IDs** pack the module ID and widget index into 64 bits. They are stable while a module exists, but they would need care for inspector rows, overlays, and the browser.
5. **Attach order.** AccessKit documents that the adapter must attach before the window is first shown. It worked after, but a real version should open the window hidden, attach, and then show it, so it does not rely on luck.
6. **Iced's own support later.** If Iced adds AccessKit, its adapter and ours would compete for the same view. A real version needs a way to switch to Iced's when it arrives.

## Suggested next step

The spike answers questions 1 to 3 with yes on macOS. Before any decision, one session with VoiceOver on, by a person, should check that the tiles and knobs are read and can be changed with the keyboard. If that holds, the decision to build this properly belongs in a new ADR that supersedes ADR-0028, with a specification that covers focus, refresh, node IDs, and the views still missing from the snapshot.
