# Text font probe

A small program that prints, for each text crate candidate of plan step 2.4, which font face it draws the system sans-serif with, and how wide it sets six tile labels at 9, 11, and 13 pixels. It uses no LoomDSP code.

It checks one criterion of the [UI layer spec](../../docs/design/ui-layer.md), section 8, rule 8: the new text stack must pick the same system font as today (rule 6), because a different font measures differently and moves layout, hit tests, and `locate` answers. The findings are in the [research note](../../docs/research/2026-11-text-crate-choice.md).

This folder is temporary. It is outside the LoomDSP workspace (the root `Cargo.toml` excludes it), so CI does not build it and its dependencies never reach the workspace. Its own `Cargo.lock` pins the versions the note names. Delete the folder and its `exclude` entry once plan step 2.4b lands.

## What it compares

| Path | What it calls |
|---|---|
| `today (Iced 0.14)` | Iced's paragraph, exactly as `loom-canvas` measures tile text: `Font::DEFAULT`, simple shaping, built without Iced's `fira-sans` feature, as the app is |
| `cosmic-text 0.15` | The crate under Iced 0.14, called directly |
| `cosmic-text 0.19` | Its newest release |
| `parley sans-serif` | Parley 0.12 with its default family, the generic sans-serif |
| `parley system-ui` | Parley 0.12 with the generic system UI family |

## Run

```sh
cd examples/text-font-probe
cargo run --release
```

Each line gives the path, the size in pixels, the label, its width in logical pixels, and the PostScript names of the faces its glyphs came from. It runs on any platform; the research note records what it printed on macOS and what is expected, from source, on Windows and Linux.

## Checks

The LoomDSP checks do not cover this folder. Run them here by hand:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
