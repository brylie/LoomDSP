# Contributing to LoomDSP

Thank you for helping. This guide explains how to build, test, and send changes.

## Before you start

- Look for an issue first. If none exists, open one and describe what you want to change.
- Issues reference requirement IDs from [docs/product/requirements.md](docs/product/requirements.md), for example `FR-MIDI-02`. Mention the ID in your pull request too. This links code to the reason it exists.
- Big design changes need an [ADR](docs/adr/README.md) first.
- AI-assisted contributions are welcome. Read the [AI contribution policy](AI_POLICY.md).

Good starting points: issues labelled [`good first issue`](https://github.com/brylie/LoomDSP/labels/good%20first%20issue) or [`help wanted`](https://github.com/brylie/LoomDSP/labels/help%20wanted). New modules are often a good first contribution: they are small, self-contained, and easy to test. The current phase's work is in the [P1 milestone](https://github.com/brylie/LoomDSP/milestone/1), roughly in priority order.

To understand the code first, read [docs/dev/architecture.md](docs/dev/architecture.md).

### Labels

| Kind | Labels |
|---|---|
| Type | `feature`, `bug`, `docs`, `research`, `adr`, `chore` |
| Area | `area:engine`, `area:modules`, `area:ui`, `area:midi`, `area:crates`, `area:infra`, `area:export` |
| Priority | `must`, `should`, `could` (same as the requirement priority) |
| Newcomers | `good first issue`, `help wanted`, `hacktoberfest` |

Comment on an issue before you start, so two people do not do the same work.

## Build and test

You need Rust 1.88 or newer. On Linux, install the ALSA development package (`libasound2-dev`).

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo bench -p loom-modules          # optional, takes a few minutes
```

Run the desktop app and the command-line tool:

```sh
cargo run --release -p loom-app -- examples/patches/fm-feedback.ron
cargo run --release -p loom-cli -- play examples/patches/subtractive.ron
```

### Git hooks (recommended)

The repository uses [prek](https://prek.j178.dev) to run quick checks before each commit, so you hear about problems while the change is fresh. Tools come from [mise](https://mise.jdx.dev):

```sh
mise install    # Rust, prek, typos, cargo-deny (from mise.toml)
prek install    # installs the pre-commit and pre-push hooks
```

| When | Checks |
|---|---|
| Every commit (a few seconds) | Whitespace, line endings, TOML/YAML syntax, large files, private keys, `cargo fmt`, `cargo clippy`, `typos`, and no direct commits to `main` |
| Before push | `cargo test`, `cargo deny check` |

`cargo fmt` and the whitespace hooks fix files for you. When they do, the commit stops: review the changes, stage them, and commit again. Run everything by hand with `prek run --all-files`. The configuration is in `prek.toml`.

### What CI checks

Every pull request runs these jobs. All must pass before merging (the "CI success" check).

| Job | What it checks | Run it locally |
|---|---|---|
| Format | `rustfmt` style | `cargo fmt --all --check` |
| Clippy | Lint groups set in the workspace `Cargo.toml`: correctness, suspicious, complexity, perf, style, cargo | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | All tests on Linux, macOS (Apple Silicon), and Windows, plus benchmarks compile | `cargo test --workspace` |
| Coverage | Line coverage, shown in the job summary | `cargo llvm-cov --workspace` (install [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)) |
| Docs | API docs build without warnings | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` |
| MSRV | Still builds with Rust 1.88 | `cargo +1.88 check --workspace` |
| WebAssembly | Engine crates build for `wasm32-unknown-unknown` | `cargo build --target wasm32-unknown-unknown -p loom-core -p loom-modules -p loom-patch -p loom-rt` |
| Licences | Only permissive licences, no known vulnerabilities | `cargo deny check` (install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)) |
| Spelling | Typos in code and docs | `typos` (install [typos-cli](https://github.com/crate-ci/typos)) |

Why warnings fail CI but not your local build: a warning while you are still editing is noise, but a warning on `main` is a problem someone else inherits.

Dependabot opens weekly pull requests for dependency updates.

## Rules for audio code

The audio thread has a hard deadline. At 48 kHz with 128-frame buffers, each buffer must finish in 2.67 ms, every time. So code that runs inside `Module::process` or `Engine::process` must not:

- allocate or free memory (no `Vec::push` past capacity, no `Box::new`, no `String`),
- take a lock or wait on another thread,
- make system calls (no printing, logging, or file access).

Why: each of these can take an unpredictable amount of time, and one late buffer is an audible dropout. The test `crates/loom-modules/tests/no_alloc.rs` aborts if the audio path allocates.

Modules must also give the same output whether they process 1 frame or 128 frames at a time. Inside feedback loops the engine calls them one frame at a time (ADR-0002). The test `output_does_not_depend_on_host_buffer_size` checks this.

## Adding a module

1. Add a `static` `ModuleDescriptor` and a struct implementing `Module` in `crates/loom-modules/src/`.
2. Add it to `ALL` in `crates/loom-modules/src/lib.rs`.
3. Give it a stable, namespaced ID (`loom.<category>.<name>`) and version 1. Never rename a released ID, port, or parameter. Add a new version instead.
4. Mark whole-number parameters (channels, counts, seeds) with `.integer()` on their `ParamSpec`, so they round and the inspector snaps to whole values.
5. Write a unit test with `loom_core::testing::run`.
6. Run the full test suite, then try the module in the app: `cargo run --release -p loom-app`, pick it in the toolbar, and click **Add**.

## Working on the canvas and app

The canvas (`crates/loom-canvas`) must stay fast on high-resolution displays. Its cost should follow the number of things drawn, not the number of pixels. So:

- Do not rebuild GPU data every frame. It is rebuilt only when the scene's generation changes; pan and zoom change one uniform.
- Do not add shaders that run over the whole window for each shape.
- In `canvas.wgsl`, avoid names that are reserved in Metal or HLSL (`half`, `in`, `out`, `sample`, and others). The `shader_names` test checks the list.

To check performance, run `cargo run --release -p loom-app -- --generate 300 --bench` and read the bottom bar. Results and the method are in [docs/benchmarks/p1-canvas.md](docs/benchmarks/p1-canvas.md). For UI changes, add a screenshot to the pull request.

## Dependencies

Only permissively licensed dependencies are allowed (MIT, Apache-2.0, BSD, ISC, Zlib, and similar). No GPL, LGPL, AGPL, or other copyleft licences. See [ADR-0006](docs/adr/0006-dependency-and-licence-policy.md). Add a short dependency review to the pull request for any new dependency.

## Pull requests

- Keep them focused: one change per pull request.
- Include tests.
- Explain what changed and why, in plain language.
- CI must pass.

## Writing style for docs

Short, direct sentences. Plain words. Explain the reason behind a rule, not only the rule. Many contributors read English as a second language.

## Licence

By contributing, you agree that your contribution is licensed under Apache-2.0.
