# Draft issue for AccessKit: macOS adapter has no spoken value beside a numeric value

**Status: draft, not filed.** Filing needs the LoomDSP maintainer's approval, and the VoiceOver behaviour below must first be confirmed by a listening check with the reproduction.

---

**Title:** macOS: a slider with both `numeric_value` and `value` exposes only the string, so it cannot be stepped and say its unit

## What happens

On macOS, `accessibilityValue` returns one object for a node. When a node has both a string `value` and a `numeric_value`, the string is returned and the number is not exposed. The adapter does not implement `accessibilityValueDescription`.

So a slider that shows "800 Hz" has two choices, and each loses something:

- **Number only** (`numeric_value` 800): VoiceOver can step the slider, and reads "800" with no unit.
- **Number and string** (`numeric_value` 800, `value` "800 Hz"): VoiceOver reads "800 Hz", and cannot step the slider. In our application no action reached the action handler.

*(The VoiceOver behaviour was observed in our application on one machine. To be confirmed with the reproduction below; macOS version to be added. In that session VoiceOver stepped a number-only slider by calling `setAccessibilityValue:` with a number it computed itself, which arrived as `Action::SetValue` with `ActionData::NumericValue`.)*

## What is expected

A slider that reports a number for stepping and a text with the unit for speech, as a native `NSSlider` does with `accessibilityValue` and `accessibilityValueDescription`.

## Where it is in the source

`accesskit_macos` 0.27.2, `src/node.rs` (the same on `main` at `dea43841`, 2026-11-09):

- `NodeWrapper::value` returns a boolean, a string, or a number. It returns the node's string `value` before it looks at `numeric_value`.
- `PlatformNode`'s `accessibilityValue` returns that one object.
- No method for the `accessibilityValueDescription` selector exists in the crate.

## Reproduction

One window, two slider nodes with the same range, on `winit` 0.30 with `accesskit_winit` 0.34.2:

<https://github.com/brylie/LoomDSP/tree/main/examples/accesskit-macos-repro> *(link valid once the folder is on `main`)*

```sh
cargo run --example slider_value
```

"Number only" has `numeric_value` 800. "With unit" has `numeric_value` 800 and `value` "800 Hz". Both have minimum 20, maximum 20000, step 10, and the Focus, Increment, Decrement, and SetValue actions. The terminal prints every action that arrives.

Turn VoiceOver on, move to each slider, interact with it, and try to change it.

The folder's `inspect.swift` prints both sliders' attributes through the accessibility API. Predicted from the source: `AXValue` is the number 800 for the first and the string "800 Hz" for the second, and `AXValueDescription` is absent for both.

## Versions

- `accesskit` 0.25.2, `accesskit_macos` 0.27.2, `accesskit_winit` 0.34.2, `winit` 0.30.13
- macOS: *to be added from the listening check*

## A possible direction

For a node with a numeric value and a string value, in a role that has a range (slider, spin button, scroll bar, progress indicator, meter), return the number from `accessibilityValue` and the string from a new `accessibilityValueDescription`. This changes what such nodes report today, so the rule is for the maintainers to choose, and the other platform adapters may want the same pairing. We are glad to send a pull request if the direction is right.
