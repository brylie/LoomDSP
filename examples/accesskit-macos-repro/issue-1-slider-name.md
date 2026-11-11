# Draft issue for AccessKit: macOS adapter never sets the accessibility label

**Status: draft, not filed.** Filing needs the LoomDSP maintainer's approval, and the VoiceOver behaviour below must first be confirmed by a listening check with the reproduction.

---

**Title:** macOS: a node's label is exposed only as `accessibilityTitle`, so VoiceOver does not read a slider's name

## What happens

On macOS, a node's `label` reaches the platform only through `accessibilityTitle`. The adapter does not implement `accessibilityLabel`.

With VoiceOver on a `Role::Slider` node that has a label and a numeric value, VoiceOver reads the value and the role, and not the label. *(Observed in our application on one machine, where a slider labelled "Tune" was read as "0.000 st, circular slider". To be confirmed with the reproduction below; macOS version to be added.)*

## What is expected

VoiceOver reads the slider's name before its value, as it does for a native `NSSlider` whose `accessibilityLabel` is set.

Apple's `NSAccessibility` protocol describes `accessibilityLabel` as the short description of the element, and `accessibilityTitle` as the title of an element such as a window or a button, which is usually its visible text.

## Where it is in the source

`accesskit_macos` 0.27.2, `src/node.rs` (the same on `main` at `dea43841`, 2026-11-09):

- `PlatformNode` implements `accessibilityTitle`, from `NodeWrapper::title`, and `accessibilityHelp`, from the node's description.
- No method for the `accessibilityLabel` selector exists in the crate, and `isAccessibilitySelectorAllowed:` lists `accessibilityTitle` and not `accessibilityLabel`.
- `NodeWrapper::label` exists, and is used only in `src/event.rs`, for live region announcements.

## Reproduction

One window, one slider node, on `winit` 0.30 with `accesskit_winit` 0.34.2:

<https://github.com/brylie/LoomDSP/tree/main/examples/accesskit-macos-repro> *(link valid once the folder is on `main`)*

```sh
cargo run --example slider_name
```

Turn VoiceOver on and move the VoiceOver cursor to the slider. The node is built in `src/model.rs`: `Role::Slider`, label "Cutoff", numeric value 50, minimum 0, maximum 100, step 1, with the Focus, Increment, Decrement, and SetValue actions.

The folder's `inspect.swift` prints the slider's attributes through the accessibility API. Predicted from the source: `AXTitle` is "Cutoff" and `AXDescription` is absent.

## Versions

- `accesskit` 0.25.2, `accesskit_macos` 0.27.2, `accesskit_winit` 0.34.2, `winit` 0.30.13
- macOS: *to be added from the listening check*

## A possible direction

Implement `accessibilityLabel` from the node's label, for the nodes where `accessibilityTitle` returns it today, and allow that selector. Whether `accessibilityTitle` should then stop returning the same text for some roles, so that VoiceOver does not say it twice, is a question for the maintainers. We are glad to send a pull request if the direction is right.
