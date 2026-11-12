# AccessKit's macOS Adapter: Two Gaps, and What `accesskit_winit` Brings

- **Kind:** Research snapshot (reading a dependency's source, and a dependency check). Non-normative.
- **Date:** 2026-11-11. `accesskit` 0.25.2, `accesskit_consumer` 0.39.2, `accesskit_macos` 0.27.2, `accesskit_winit` 0.34.2, `winit` 0.30.13. AccessKit's `main` at commit `dea43841` (2026-11-09). macOS 27.0.1.
- **Related decisions:** [ADR-0041](../adr/0041-own-ui-layer.md) (our own UI layer), ADR-0006 (permissive dependencies; AccessKit is an accepted dependency, so its source may be read).
- **Related:** [the spike that found the gaps](2026-11-a11y-beside-iced-spike.md), [UI layer spec](../design/ui-layer.md) section 7, rules 4 and 7, [plan](../dev/leaving-iced-plan.md) steps 1.5 and 2.2.
- **Code:** `examples/accesskit-macos-repro/`, a reproduction outside the workspace, with the two draft issue reports beside it.

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## Question

The spike found two gaps in AccessKit's macOS adapter. Are they still there, where are they in its source, can they be shown with no LoomDSP code, and what would a fix need? A second question came up while building the reproduction: what does `accesskit_winit` bring into a dependency tree?

## Summary

1. **Both gaps are in the source, in the newest release and on `main`.** The adapter never implements the accessibility label, and it has no spoken value description beside a number.
2. **Nothing upstream was found that reports either.** Four searches of AccessKit's issues and pull requests returned nothing. A search can miss a report under other words.
3. **What VoiceOver says is not confirmed here.** It rests on the spike's one session. A listening check with the reproduction is needed before anything is filed.
4. **`accesskit_winit` with its default features fails our `cargo deny check`,** through the Linux adapter's D-Bus stack. Without that one feature it passes. This is a risk for plan step 2.2.

## Gap 1: the name is exposed only as the title

In `accesskit_macos` 0.27.2, `src/node.rs`:

| What | Where | Finding |
|---|---|---|
| `accessibilityTitle` | `PlatformNode`, line 534, from `NodeWrapper::title`, line 316 | Returns the node's label, except for the root window and for roles whose label is their value |
| `accessibilityHelp` | line 543 | Returns the node's description |
| `accessibilityLabel` | nowhere in the crate | Not implemented |
| `isAccessibilitySelectorAllowed:` | `is_selector_allowed`, line 1234 | Lists `accessibilityTitle` and `accessibilityHelp`, and not `accessibilityLabel` |
| `NodeWrapper::label` | line 328 | Exists, and is used only in `src/event.rs` (lines 236 and 300) for live region announcements |

`main` at `dea43841` has the same file length and the same selectors.

**Observed, not confirmed here:** in the spike, VoiceOver read a slider as its value and role with no name.

**What a fix would need.** A method for `accessibilityLabel` that returns the node's label where `accessibilityTitle` returns it today, and that selector in the allowed list. It looks small. The open question is for AccessKit's maintainers: whether the title should then stop returning the same text for some roles, so that VoiceOver does not say a name twice. No change to the `accesskit` schema seems needed. That is a reading of one file, not a tested patch.

## Gap 2: one value, a string or a number

In the same file:

| What | Where | Finding |
|---|---|---|
| `NodeWrapper::value` | line 344 | Returns one of a boolean, a string, or a number. The node's string `value` is returned before `numeric_value` is looked at |
| `accessibilityValue` | line 561 | Returns that one object, as an `NSNumber` or an `NSString` |
| `setAccessibilityValue:` | line 580 | A number becomes `Action::SetValue` with `ActionData::NumericValue`; a string becomes `ActionData::Value` |
| `accessibilityValueDescription` | nowhere in the crate | Not implemented |

So a slider node with `numeric_value` 800 and `value` "800 Hz" reports only the string, and a node with only the number has nowhere to put its unit.

**Observed, not confirmed here:** in the spike, VoiceOver said the unit and could not change a slider whose value was text, with no action reaching the app. With a number it changed the slider by setting the value itself, and said no unit.

**What a fix would need.** For a node with both, in a role that has a range, return the number from `accessibilityValue` and the string from a new `accessibilityValueDescription`. The code is small. It is a change in what such nodes report, so the maintainers must choose the rule, and the other platform adapters may want the same pairing. No schema change seems needed, since a node can already hold both.

The UI layer spec's section 7, rule 4 is written around this gap: a slider's value is a number, and its text with the unit goes in the description. The gap is open, so the rule stands.

## The spec's rule 7, checked against the source

Rule 7 of section 7 makes three statements about these versions. All three hold:

1. **"The app's only callbacks are activation, actions, and deactivation."** `accesskit` 0.25.2 has three handler traits: `ActivationHandler::request_initial_tree`, `ActionHandler::do_action`, and `DeactivationHandler::deactivate_accessibility`.
2. **"`accesskit_macos` answers VoiceOver's value reads from its own copy of the tree, with no call to the app."** `accessibilityValue` goes through `resolve`, which borrows the adapter's tree and looks the node up by ID. No handler is called.
3. **"It posts a value-changed notification for every node whose value changes."** `src/event.rs`, `node_updated`, lines 270 to 288: when `NodeWrapper::value` differs between the old and the new node, it queues `NSAccessibilityValueChangedNotification`.

## What `accesskit_winit` brings

Plan step 2.2 puts `accesskit_winit` into the workspace. The reproduction uses it, so its tree was checked against the root `deny.toml`.

**With default features:** `advisories FAILED, bans ok, licenses ok, sources ok`. Three errors, each an "unmaintained" advisory:

| Crate | Advisory |
|---|---|
| `zvariant` 5.15.1 | RUSTSEC-2026-0370 |
| `zbus_names` 4.3.5 | RUSTSEC-2026-0371 |
| `zvariant_derive` 5.15.1 | RUSTSEC-2026-0372 |

Each advisory says the crate is no longer maintained as a standalone crate. They arrive through `accesskit_winit` → `accesskit_unix` 0.24.1 → `atspi` 0.29.0 and `accesskit_atspi_common` 0.21.1 → `atspi-common` 0.13.0 → `zbus` 5.19.0 and its companions. The lockfile also gains `async-io` or `tokio`. The workspace has no `zbus` today, and its `Cargo.toml` already turns off an Iced feature to keep `zbus` 5 out.

**With `default-features = false, features = ["rwh_06"]`:** `advisories ok, bans ok, licenses ok, sources ok`, and no `zbus`, `zvariant`, or `atspi` crate in the lockfile. `accesskit_unix` is an optional dependency behind a default feature. The `tokio` feature swaps the async runtime; it does not remove `zbus`. The reproduction itself builds with this reduced feature set: it runs on macOS only, so its committed lockfile should not hold the Linux adapter's crates and their advisories.

**What this means for step 2.2.** Linux screen reader support through AccessKit needs `accesskit_unix`, and with it these advisories. The choices are to add ignore entries to `deny.toml` with reasons, as the file already does for three crates in the UI stack, or to leave the Linux adapter out at first. That is a decision for step 2.2, not made here. `deny.toml` is unchanged.

**Not checked:** whether a newer `accesskit_unix` or `atspi` drops these crates; what the advisories recommend in their place; the Windows adapter's tree beyond what `cargo deny` reported (it passed).

## The reproduction

`examples/accesskit-macos-repro/` is a crate outside the workspace, so nothing above reaches the workspace's tree. It has one shared module for the sliders and their tree, a `winit` window with `accesskit_winit`, and two example programs, `slider_name` and `slider_value`. Its README lists what to listen for.

The two issue reports for AccessKit are drafted beside it, as `issue-1-slider-name.md` and `issue-2-value-description.md`. They live there, not here, so they are deleted with the reproduction once upstream has them.

## Found later, while building step 1.4 (2026-11-12)

Read in the same crate (`accesskit_macos` 0.27.2) while building the matrix (slice 3c) and the overlays (slices 4a and 4b). Each is from the source, not heard.

| What | Finding | What LoomDSP does |
|---|---|---|
| Tables | No columns, no row or column counts, no header cells, and `accessibilityRows` only for a selectable container | The matrix's assignments are a list of rows, not a table |
| `active_descendant` | Never read | A browser's highlighted row is also the list box's selected option, which the adapter does report |
| `accessibilityPerformShowMenu` | Not implemented, so `Action::ShowContextMenu` never arrives | A tile's menu, and a library row's, open by VoiceOver's secondary click; the action works on other platforms |

## Not verified

- **What VoiceOver says and does,** on any macOS version. Only the spike's one session, on one machine.
- **The accessibility attributes of the reproduction's sliders.** `inspect.swift` reads them through the accessibility API. It ran and walked Finder's tree, but for programs started from the automated shell that wrote this, each window answered as the application, for the LoomDSP app too. So the table in the README is a prediction from the source.
- **Apple's wording for the label and the title,** quoted in the first draft from memory. Check it against Apple's documentation before filing.
- **That no upstream report exists.** None was found.
- **The fix sketches.** Neither was written or tested.

## Next

1. A listening check with the reproduction, and a run of `inspect.swift` from an ordinary terminal.
2. With those results in the drafts, the maintainer decides whether to file them (plan step 1.5, item 2). Filing is an outward action.
