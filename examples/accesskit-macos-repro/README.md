# AccessKit macOS adapter: two gaps, reproduced

Two small programs that show two gaps in `accesskit_macos` 0.27.2 with VoiceOver. Each opens one window whose accessibility tree holds slider nodes. They use `winit` and `accesskit_winit`, and no LoomDSP code.

- **`slider_name`:** a slider's name is not read. The adapter gives the name to macOS as the accessibility title and never as the accessibility label.
- **`slider_value`:** a slider can be stepped or say its unit, not both. The adapter reports a node's string value when it has one and its number otherwise, and has no spoken value description beside a number.

The findings, with where each gap is in AccessKit's source, are in [the research note](../../docs/research/2026-11-accesskit-macos-gaps.md). The two issue reports drafted for AccessKit are [issue-1-slider-name.md](issue-1-slider-name.md) and [issue-2-value-description.md](issue-2-value-description.md). They are drafts. Nothing has been filed.

This folder is temporary. It is outside the LoomDSP workspace (the root `Cargo.toml` excludes it), so CI does not build it and its dependencies never reach the workspace. Its own `Cargo.lock` pins the versions the reports name. Delete the folder and its `exclude` entry once AccessKit has the fixes.

## Run

macOS only.

```sh
cd examples/accesskit-macos-repro
cargo run --example slider_name
cargo run --example slider_value
```

The window is blank on purpose: the sliders exist only in the accessibility tree. Tab moves the focus between sliders, the arrow keys move the focused slider, and Escape quits. The terminal prints each slider's value, its string value, and every action that assistive technology sends.

## What to listen for

Turn VoiceOver on (Cmd+F5), click the window, and move the VoiceOver cursor to a slider (VO+Right, where VO is Control+Option).

1. **`slider_name`, the slider.**
   - Expected: "Cutoff", then the value, then "slider".
   - Reported gap: the value and "slider", with no "Cutoff".
2. **`slider_value`, "Number only".**
   - Listen for: "800" with no unit.
   - Then interact with it (VO+Shift+Down) and press VO+Up or VO+Down a few times. Does the value change? The terminal prints an action for each step that arrives.
3. **`slider_value`, "With unit".**
   - Listen for: "800 Hz".
   - Then the same keys. Does the value change, or does nothing arrive in the terminal?
4. **For contrast, a native slider with a unit,** such as the Sound volume in Control Centre or a slider in System Settings. Its name, its value with the unit, and stepping all work.

Note the macOS version with what you heard.

## Check without VoiceOver

`inspect.swift` prints what the macOS accessibility API reports for every slider in a running program: its title, description, value, value description, range, and actions. It shows the facts the adapter gives macOS, with no screen reader.

```sh
cargo run --example slider_value &
swift inspect.swift $!        # run it twice: the first read asks AccessKit for its tree
swift inspect.swift --tree $! # also prints every element on the way down
```

What the source predicts for a slider with a label:

| Attribute | Predicted | Why |
|---|---|---|
| `AXTitle` | the label | the adapter implements `accessibilityTitle` |
| `AXDescription` | absent or empty | it does not implement `accessibilityLabel` |
| `AXValue` | a number, or the string when the node has one | `NodeWrapper::value` prefers the string |
| `AXValueDescription` | absent | it does not implement `accessibilityValueDescription` |

**Permission.** The script reads another program's accessibility tree, so macOS requires the Accessibility permission for the terminal that runs it: System Settings, Privacy & Security, Accessibility. Grant it yourself. Without it the script says so and exits with status 2.

**Not yet run on a slider.** When this was written, the script ran and walked Finder's tree, but it could not read the windows of programs started from the automated shell that wrote it: each window answered as the application, for the LoomDSP app too. Run it from an ordinary terminal. Xcode's Accessibility Inspector shows the same attributes.

## Checks

The LoomDSP checks do not cover this folder. Run them here by hand:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
