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
LOOM_UPDATE_REFERENCES=1 cargo test -p loom-modules --test reference   # write module reference renders
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
- **Telemetry** (`crates/loom-core/src/telemetry.rs`). The UI probes an output port with `Controller::add_probe`; after each block the engine pushes meter readings and scope chunks into two bounded queues, read through `Telemetry`. When a queue is full, telemetry is dropped and counted, never waited for. Spec: [docs/design/telemetry.md](docs/design/telemetry.md).
- **Modules** implement `Module::process` over a `PortIo`. Their ports and parameters are described by a static `ModuleDescriptor`. An unconnected input reads its `PortSpec` default: zeros, a fixed value, or a transport signal. Use `io.is_connected(i)` for fallbacks; it reports cables only. Test each module with the `loom-test` harness (`crates/loom-modules/tests/reference.rs`): it checks block sizes 1, 77, and 512 and compares with a reference WAV file. Spec: [docs/design/module-test-harness.md](docs/design/module-test-harness.md).
- **Panels** (ADR-0013, `crates/loom-core/src/descriptor.rs`, `display.rs`). A `ModuleDescriptor` declares its tile's `Panel`: widgets on the grid, bound by name to parameters (controls) or display values (lights). The registry checks every panel. Modules publish up to 4 display values in `Module::displays`, once per block, into a latest-value table the UI reads with `Controller::display`. Spec: [docs/design/panels.md](docs/design/panels.md).
- **Transport** (ADR-0015, `crates/loom-core/src/transport.rs`). The engine owns one transport per patch: tempo, time signature, running, and position in `f64` beats. The `Controller` changes it; modules read it through `ProcessContext::transport`. Position is computed from an anchor, never by adding a step per block, so it is the same at any block size. Use `TransportInfo::position_at(i)` inside a call. Spec: [docs/design/clock.md](docs/design/clock.md).
- **Signals** (ADR-0007). Every cable is `f32` at the sample rate. Types are hints only. Pitch is 0.1 per octave, 0.0 = middle C. Gates are high above 0.5. A trigger output is high for one sample, and trigger inputs count rising edges (`signal::trigger`, `signal::EdgeDetector`). "1.0 = 10 V" for Eurorack users.
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

## Spec first

Before writing code for a feature, read its specification in `docs/design/` (the issue links it).

- Build only what the spec says, from sections marked **Agreed**. Do not build what it lists as out of scope.
- If the spec does not answer a question about behaviour (a gesture, a default, what the user sees), do not guess. Ask, or propose a spec change first.
- If the code shows the spec is wrong, update the spec in the same pull request and say so in the description.
- Check every acceptance criterion: as a test where possible, otherwise as a manual check listed in the pull request.
- No spec is needed for bug fixes, refactoring, tests, or changes that do not change behaviour.

Why: a gap in the spec gets filled with a guess, and a well-built guess is costly to undo. See [docs/design/README.md](docs/design/README.md#spec-first).

## Where documentation goes

- A **decision** (with its options and reasons) goes in an ADR. Keep it to the decision. Once accepted, do not rewrite it; write a new ADR that supersedes it.
- **How a feature works** (modules, ports, rules, examples) goes in a specification in `docs/design/`, linked from the ADR. Update it in the same change as the code.
- **Who needs it and why** goes in use cases and requirements in `docs/product/`. **Tasks** go in GitHub issues.

Why: decisions stay trustworthy only if they do not move, while details must be free to change. See [docs/README.md](docs/README.md).

## Writing style

Docs and comments use short, direct sentences and plain words, because many readers use English as a second language. Explain the reason behind a rule. Do not use em dashes. Avoid filler words such as "delve", "journey", "landscape", "realm", and "embark".

## Where things are

- Requirements with IDs: `docs/product/requirements.md`, `docs/product/non-functional-requirements.md`
- Decisions: `docs/adr/` (one decision per ADR; frozen once accepted)
- Specifications: `docs/design/` (living; update with the code they describe)
- Benchmarks: `docs/benchmarks/`
- Example patches: `examples/patches/`
