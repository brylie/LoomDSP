# LoomDSP

**Patch it, play it live, share it.** An open-source modular synthesizer and sound lab, written in Rust, built for the stage.

[![CI](https://github.com/brylie/LoomDSP/actions/workflows/ci.yml/badge.svg)](https://github.com/brylie/LoomDSP/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/brylie/LoomDSP/graph/badge.svg?token=3XZBL6OICG)](https://codecov.io/gh/brylie/LoomDSP)
[![Licence: Apache-2.0](https://img.shields.io/badge/licence-Apache--2.0-blue.svg)](LICENSE)
[![Status: early alpha](https://img.shields.io/badge/status-early%20alpha-orange.svg)](docs/product/roadmap.md)
<!-- ALL-CONTRIBUTORS-BADGE:START - Do not remove or modify this section -->
[![All Contributors](https://img.shields.io/badge/all_contributors-2-orange.svg?style=flat-square)](#contributors-)
<!-- ALL-CONTRIBUTORS-BADGE:END -->

![A subtractive synth patched from primitives in the LoomDSP desktop app: MIDI note, phase, saw, filter, two envelopes, and a VCA, with the filter's parameters in the inspector](docs/images/readme-subtractive.png)

LoomDSP lets you build instruments and effects by connecting small modules on a canvas: oscillators, filters, envelopes, logic, and more. You hear every change as you make it. Then you play the result with a MIDI keyboard, and share the patch file with anyone.

The goal is bigger than a synth. LoomDSP is for **living musical systems**: patches that play, evolve, and arrange themselves with sequencers, loopers, counters, and logic, all patched the same way. Read the [vision](docs/product/vision.md).

## Why LoomDSP

- **Edit while it plays, with no dropouts.** Add, remove, and rewire modules during a performance. The audio thread never allocates, locks, or waits, and a test enforces it. In a 10-minute stress test, 11,026 live edits caused zero dropouts ([benchmark](docs/benchmarks/p0-engine.md)).
- **Patch anything to anything.** Every cable carries the same kind of signal, in the spirit of Eurorack. Audio can modulate a filter, an envelope can drive pitch, and a gate can be audio. Colours guide you, but never block a connection.
- **Real feedback loops.** Patch an oscillator back into itself. The engine finds every loop and runs it one sample at a time, so feedback FM and filter feedback sound right. If a loop needs a delay, the engine adds one and shows you where.
- **Same sound, every time.** A patch gives bit-identical audio at any buffer size and on every run. Renders are repeatable, and random sources are seeded.
- **Safe for your ears and speakers.** A DC blocker, a limiter, and a short fade-in sit before the output, so a wild patch never blasts you ([how it works](docs/learn/output-safety.md)).
- **Fast, native, GPU-drawn.** Our own canvas runs at 120 fps with 300 modules on screen, on an M1 laptop ([benchmark](docs/benchmarks/p1-canvas.md)). No web view, no Electron.
- **Learn as you patch.** Every module and control explains itself on hover. Behind the code are [explainers](docs/learn/README.md) on the theory: phase, feedback, stereo cables, and more.
- **Open, all the way down.** Apache-2.0, with only permissive dependencies. Patches are plain text files that never run code, so sharing them is safe. Patches you make are yours.

![Feedback FM: the sine's output feeds back into its own pitch. The dashed cable shows where the engine broke the loop with a one-sample delay](docs/images/readme-fm-feedback.png)

## What you can do today

LoomDSP is in **early alpha** (Phase 1 of the [roadmap](docs/product/roadmap.md)). It is not a finished instrument yet, but you can already:

- **Patch on a canvas.** Add modules from a searchable browser, or type to add one at the pointer. Drag cables, move tiles, copy, paste, and duplicate.
- **Turn knobs on the modules themselves.** Each tile has its own panel of knobs, buttons, and lights. The inspector shows every parameter of the selected module.
- **Modulate any parameter.** Assign any output to a knob, and watch the range arc and the live value move.
- **Play it.** Every connected MIDI keyboard works at once. Map any control to a MIDI knob with MIDI learn. No keyboard? Click **Hold note**.
- **Work without fear.** Undo and redo for every edit, open and save patches, and autosave with crash recovery.
- **Choose your audio device** and buffer size in the preferences window.
- **Render and test from the command line.** Play a patch, render it to WAV, or find its feedback loops with the `loom` tool.

Modules so far: phase generator, sine, band-limited saw, noise, state variable filter, ADSR, add, multiply, constant, one-sample delay, clock divider, phase trigger, level meter, button, MIDI note input, and audio output. Around 170 more are planned in the [module catalogue](docs/product/module-catalogue.md), and many are open for contributors.

## Try it

There are no installers yet, so you build from source. You need [Rust](https://rustup.rs) 1.99 or newer. On Linux, also install the ALSA development package (`libasound2-dev` on Debian and Ubuntu).

```sh
git clone https://github.com/brylie/LoomDSP
cd LoomDSP
cargo run --release -p loom-app -- examples/patches/subtractive.ron
```

The first build takes a few minutes. Connect a MIDI keyboard before you start, or click **Hold note** in the toolbar. Then turn the filter's **Cutoff** knob while the note plays. [Getting started](docs/guide/getting-started.md) walks you through your first patch.

More things to try:

```sh
# Feedback FM: an oscillator that modulates itself
cargo run --release -p loom-app -- examples/patches/fm-feedback.ron

# Start from an empty canvas
cargo run --release -p loom-app

# List every module
cargo run --release -p loom-cli -- modules

# Play a patch from the terminal with your MIDI keyboard, or hold one note
cargo run --release -p loom-cli -- play examples/patches/fm-feedback.ron --note 57

# Render a chord to a WAV file
cargo run --release -p loom-cli -- render examples/patches/subtractive.ron -o out.wav --note 48 --note 55 --note 60

# Show where the engine found feedback loops
cargo run --release -p loom-cli -- check examples/patches/fm-feedback.ron

# Rewire the patch 20 times a second while it plays, and count dropouts
cargo run --release -p loom-cli -- stress examples/patches/subtractive.ron --seconds 600
```

## Where it is going

| Phase | What you get |
|---|---|
| **P0. Engine** (done) | A real-time engine that plays patch files with live MIDI. |
| **P1. Desktop patcher** (now) | Build and play a synth from primitives, with MIDI controllers. |
| P2. Panels and performance | Sub-patches with their own front panels, polyphony, presets, and a locked performance mode. |
| P3. Living systems | Loopers, sequencers, scenes, counters, and logic: patches that arrange themselves on stage. |
| P4. Sharing | Self-contained patch files, a patch library, and a module SDK for community modules. |

Plugin export (CLAP, VST3) and a browser version may come later. The engine core already builds for WebAssembly, so the door stays open. See the full [roadmap](docs/product/roadmap.md).

**Early alpha means breaking changes.** Until the architecture settles, module names, the patch format, and the Rust API can change.

## Get involved

LoomDSP is a good project to learn modular synthesis, real-time audio, or Rust, and to have your work heard on stage.

- **Write a module.** Most [`good first issue`](https://github.com/brylie/LoomDSP/labels/good%20first%20issue) tasks are new modules, each with a spec and a test harness that checks it for you.
- **Try it and tell us.** Patch something, then open an [issue](https://github.com/brylie/LoomDSP/issues) with what felt good and what got in the way.
- **Improve the docs and explainers.** If something confused you, it will confuse the next person too.

Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started. AI-assisted contributions are welcome under our [AI contribution policy](AI_POLICY.md). During October, issues labelled [`hacktoberfest`](https://github.com/brylie/LoomDSP/labels/hacktoberfest) count for Hacktoberfest.

## Documentation

- [Getting started](docs/guide/getting-started.md): run the app and patch your first sound
- [Learn](docs/learn/README.md): the theory behind the design, for musicians and developers
- [Architecture](docs/dev/architecture.md): how the code fits together
- [Vision](docs/product/vision.md), [requirements](docs/product/requirements.md), [roadmap](docs/product/roadmap.md), and [module catalogue](docs/product/module-catalogue.md)
- [Architecture decisions](docs/adr/README.md) and [specifications](docs/design/README.md)
- Benchmarks: [Phase 0 engine](docs/benchmarks/p0-engine.md), [Phase 1 canvas](docs/benchmarks/p1-canvas.md)
- [Everything else](docs/README.md)

<details>
<summary>Repository layout</summary>

| Path | What it is |
|---|---|
| `crates/loom-core` | Engine core: module API, graph, compiler, real-time engine. No I/O, builds for WASM. |
| `crates/loom-modules` | Built-in modules, integration tests, and benchmarks. |
| `crates/loom-patch` | RON patch format: load and save. |
| `crates/loom-rt` | Small real-time helpers (denormal flushing, smoothing). Reusable outside LoomDSP. |
| `crates/loom-host-desktop` | Audio (cpal) and MIDI (midir) host for desktop. |
| `crates/loom-cli` | The `loom` command-line tool. |
| `crates/loom-canvas` | The patch canvas: a GPU-drawn Iced widget ([ADR-0010](docs/adr/0010-own-patch-canvas.md)). |
| `crates/loom-app` | The desktop app: canvas, inspector, live audio and MIDI. |
| `examples/patches` | Example patches. |
| `docs` | Guides, explainers, requirements, roadmap, ADRs, specifications, and benchmarks. |

</details>

## Licence

Apache-2.0. See [LICENSE](LICENSE). Patches you create are yours, under any licence you choose.

## Contributors ✨

Thanks goes to these wonderful people ([emoji key](https://allcontributors.org/docs/en/emoji-key)):

<!-- ALL-CONTRIBUTORS-LIST:START - Do not remove or modify this section -->
<!-- prettier-ignore-start -->
<!-- markdownlint-disable -->
<table>
  <tbody>
    <tr>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/brylie"><img src="https://avatars.githubusercontent.com/u/17307?v=4?s=100" width="100px;" alt="Brylie Oxley"/><br /><sub><b>Brylie Oxley</b></sub></a><br /><a href="https://github.com/brylie/LoomDSP/commits?author=brylie" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/montionugera"><img src="https://avatars.githubusercontent.com/u/5601426?v=4?s=100" width="100px;" alt="Pasit Nusso"/><br /><sub><b>Pasit Nusso</b></sub></a><br /><a href="https://github.com/brylie/LoomDSP/commits?author=montionugera" title="Code">💻</a></td>
    </tr>
  </tbody>
</table>

<!-- markdownlint-restore -->
<!-- prettier-ignore-end -->

<!-- ALL-CONTRIBUTORS-LIST:END -->

This project follows the [all-contributors](https://github.com/all-contributors/all-contributors) specification. Contributions of any kind welcome!
