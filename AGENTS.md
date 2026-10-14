# Agent Guide for LoomDSP

Context for AI coding agents (and new human contributors). Read this before changing code.

## What this project is

LoomDSP is an open-source modular audio environment in Rust, built for live performance. Users connect DSP modules in a graph, play it with MIDI controllers, and later export it as a plugin. Read [docs/product/vision.md](docs/product/vision.md) for goals and [docs/product/roadmap.md](docs/product/roadmap.md) for the current phase.

## Commands

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                       # licences and advisories
typos                                  # spelling
cargo bench -p loom-modules
cargo run --release -p loom-cli -- check examples/patches/fm-feedback.ron
cargo run --release -p loom-cli -- render examples/patches/subtractive.ron -o /tmp/out.wav --note 60
cargo run --release -p loom-app -- examples/patches/fm-feedback.ron   # desktop app
cargo run --release -p loom-app -- --generate 300 --bench            # canvas frame-rate test
```

## Architecture in one page

The longer version, with diagrams, is [docs/dev/architecture.md](docs/dev/architecture.md).

- **Two threads, two halves** (ADR-0001). `Controller` runs on a control thread, owns the editable `Graph`, compiles it, and sends commands. `Engine` runs on the audio thread and only executes. They talk through bounded SPSC queues (`rtrb`). Memory is allocated on the control thread, moved to the audio thread, and moved back for freeing.
- **Compilation** (ADR-0002, `crates/loom-core/src/compile.rs`). The graph is turned into a `Schedule`: every output port gets a buffer, Tarjan's algorithm finds feedback loops (SCCs), acyclic parts run a block at a time, and loops run one sample at a time. `loom.time.delay1` modules break loops. If a cycle has none, the compiler inserts one and reports it.
- **Schedule swap.** A new schedule replaces the old one at a buffer boundary. Module instances live in engine slots and keep their state across swaps. Unit delay state is carried over by key.
- **Modules** implement `Module::process` over a `PortIo`. Their ports and parameters are described by a static `ModuleDescriptor`. Unconnected inputs read zeros; use `io.is_connected(i)` for fallbacks.
- **Signals** (ADR-0007). Every cable is `f32` at the sample rate. Types are hints only. Pitch is 0.1 per octave, 0.0 = middle C. Gates are high above 0.5. "1.0 = 10 V" for Eurorack users.
- **Patches** (ADR-0008) are RON files, loaded by `loom-patch`.
- **Canvas** (ADR-0010, `crates/loom-canvas`). An Iced widget drawn by one custom wgpu primitive (`canvas.wgsl`): instanced rounded rectangles and Bezier cable strips. The app builds a `Scene` from the graph; the canvas never edits it, it reports `CanvasEvent`s. GPU data is rebuilt only when the scene's generation changes, and pan or zoom only updates a uniform. Keep it that way: do not rebuild GPU data every frame, and do not add shaders that run over the whole window for each shape.
- **App** (`crates/loom-app`). Owns the `Controller`, module positions, and selection. UI crates are desktop only; the engine crates never depend on them.

Before finishing a change, run `prek run --all-files` (fmt, clippy, typos, file checks) and `cargo test --workspace`. CI runs the full list in CONTRIBUTING.md. Never bypass the hooks with `--no-verify`; fix the cause instead.

## Hard rules

1. **Real-time safety.** Nothing reachable from `Engine::process` or `Module::process` may allocate, free, lock, wait, print, or do I/O. `tests/no_alloc.rs` enforces allocation rules.
2. **Block-size invariance.** A module must give identical output whether called with 1 frame or many. Feedback loops call modules one frame at a time.
3. **Determinism.** Same patch and input give bit-identical output. Random sources must be seeded.
4. **Licences.** Only permissive dependencies (ADR-0006). Never add GPL, LGPL, or AGPL code or crates. Check `deny.toml`.
5. **Stable IDs.** Never rename a released module ID, port name, or parameter name. Patch files depend on them.
6. **Engine core stays portable.** `loom-core`, `loom-modules`, `loom-patch`, and `loom-rt` must build for `wasm32-unknown-unknown`: no threads, files, or audio APIs there.
7. **`unsafe` needs a `// SAFETY:` comment** that explains why it is sound.

## Writing style

Docs and comments use short, direct sentences and plain words, because many readers use English as a second language. Explain the reason behind a rule. Do not use em dashes. Avoid filler words such as "delve", "journey", "landscape", "realm", and "embark".

## Where things are

- Requirements with IDs: `docs/product/requirements.md`, `docs/product/non-functional-requirements.md`
- Decisions: `docs/adr/`
- Benchmarks: `docs/benchmarks/`
- Example patches: `examples/patches/`
