# Contributing to LoomDSP

Thank you for helping. This guide explains how to build, test, and send changes.

## Before you start

- Look for an issue first. If none exists, open one and describe what you want to change.
- Issues reference requirement IDs from [docs/product/requirements.md](docs/product/requirements.md), for example `FR-MIDI-02`. Mention the ID in your pull request too. This links code to the reason it exists.
- New features start with a [specification](docs/design/README.md#spec-first): what the user sees, the rules, acceptance criteria, and what is out of scope. Agree on the spec, then build to it, and update it in the same pull request when the code teaches you something.
- Big design changes need an [ADR](docs/adr/README.md) first. The ADR records the decision only, and it merges only once it is decided (Accepted, Deferred, Rejected, or Superseded), not while it is Proposed. Details that will change (ports, parameters, examples) go in a [design specification](docs/design/README.md), which you update with the code. See [kinds of documents](docs/README.md#kinds-of-documents).
- AI-assisted contributions are welcome. Read the [AI contribution policy](AI_POLICY.md).

Good starting points: issues labelled [`good first issue`](https://github.com/brylie/LoomDSP/labels/good%20first%20issue) or [`help wanted`](https://github.com/brylie/LoomDSP/labels/help%20wanted). New modules are often a good first contribution: they are small, self-contained, and easy to test. Pick an issue without `needs spec agreed`: the others still have open questions ([ready to build](docs/design/README.md#ready-to-build)). The current phase's work is in the [P1 milestone](https://github.com/brylie/LoomDSP/milestone/1), roughly in priority order.

To understand the code first, read [docs/dev/architecture.md](docs/dev/architecture.md). To run the checks that need audio or MIDI hardware, see [docs/dev/test-devices.md](docs/dev/test-devices.md).

### Labels

| Kind | Labels |
|---|---|
| Type | `feature`, `bug`, `docs`, `research`, `adr`, `chore` |
| Area | `area:engine`, `area:modules`, `area:ui`, `area:midi`, `area:crates`, `area:infra`, `area:export` |
| Priority | `must`, `should`, `could` (same as the requirement priority) |
| Newcomers | `good first issue`, `help wanted`, `hacktoberfest` |
| Readiness | `needs spec agreed`: not ready to build yet (see [Ready to build](docs/design/README.md#ready-to-build)) |

Comment on an issue before you start, so two people do not do the same work.

## Build and test

You need Rust 1.99 or newer. On Linux, install the ALSA development package (`libasound2-dev`).

The tools come from [mise](https://mise.jdx.dev), at the versions in `mise.toml`. CI installs them the same way, so your machine and CI run the same versions. Once per clone:

```sh
mise trust      # let mise read mise.toml (it asks once per folder)
mise install    # Rust with clippy, rustfmt, llvm-tools, and the wasm target; prek, typos, cargo-deny, cargo-llvm-cov, cargo-nextest
```

If you use git worktrees, each new worktree is a new folder, and mise asks to trust it again. To trust every worktree under one folder, add it to your global mise settings once:

```sh
mise settings add trusted_config_paths ~/path/to/your/worktrees
```

To let worktrees build at the same time, choose an existing directory on a drive with enough free space and run this once in each worktree:

```sh
scripts/setup-worktree-target.sh /path/to/target-root
```

For an external drive, create the target root on that drive first and keep it mounted while Cargo runs. The command writes an ignored `.cargo/config.toml` that points to a unique, stable folder under the chosen root. It also marks that folder as managed by this repository. Cargo uses it for normal builds and `mise` tasks. Run `cargo clean` in a live worktree to remove its build output, then rerun the setup command to restore the marker before building again. Other worktrees continue using their existing Cargo settings until you run the command there.

Run `scripts/setup-worktree-target.sh /path/to/target-root --all` to configure all existing worktrees at once. To inspect space used by active and removed worktrees, run from any worktree of this repository:

```sh
scripts/worktree-targets.sh /path/to/target-root          # list sizes and show what could be pruned
scripts/worktree-targets.sh /path/to/target-root --prune  # delete marked targets of removed worktrees
```

The first command is a dry-run and changes nothing. The prune command checks the ownership marker and worktree status again before each deletion. It never deletes active, unmarked, or unrelated directories. Targets created before the marker was added are adopted when setup runs again in their live worktree. Old targets whose worktree is already gone show as `unknown`; review those paths manually before removing them. Do not assume a hash-shaped folder belongs to this tool.

Then build and run the checks:

```sh
cargo build --workspace
mise run check        # every check CI runs, except the minimum Rust version and the Windows and Linux tests
mise run test         # just the tests
mise run coverage     # tests with line coverage; fails under the minimum
mise run coverage-html   # browse which lines the tests reach
cargo bench -p loom-modules          # optional, takes a few minutes
```

`mise tasks` lists every check.

### Where tests live

Tests live in their own files, never inline in a code module. Why: a code file then shows only the code, so it is shorter to read and review, and a change to the tests does not show up as a change to the code.

- Unit tests for `foo.rs` go in `foo/tests.rs`, declared at the end of `foo.rs` with `#[cfg(test)] mod tests;`. For `lib.rs`, `main.rs`, or `mod.rs`, the file is `tests.rs` in the same folder.
- Test fakes and helpers follow the same rule, for example `#[cfg(test)] pub mod fake;` in `devices.rs` with the code in `devices/fake.rs`.
- A test file can use private items through `use super::*;`, as an inline module can.
- Integration tests go in the crate's `tests/` folder, as usual.

`mise run test-placement` checks this (`scripts/check-test-placement.sh`, tested by `scripts/test-check-test-placement.sh`). It fails on an inline `#[cfg(test)] mod name { ... }`, and on a `#[test]` in a file that is not a test file. The commit hook and CI run it.

Run the desktop app and the command-line tool:

```sh
cargo run --release -p loom-app -- examples/patches/fm-feedback.ron
cargo run --release -p loom-cli -- play examples/patches/subtractive.ron
```

### Git hooks (recommended)

The repository uses [prek](https://prek.j178.dev) to run the checks before each commit and push, so you hear about problems while the change is fresh, not minutes later from CI:

```sh
prek install    # installs the pre-commit and pre-push hooks
```

| When | Checks |
|---|---|
| Every commit (a few seconds) | Whitespace, line endings, TOML/YAML syntax, large files, private keys, `cargo fmt`, `mise run clippy`, `typos` on the changed files, ADR status lines, tests in their own files, and no direct commits to `main` |
| Before push | `mise run coverage` (every test, and the coverage minimum), `mise run doctest` (examples in the API docs, which coverage does not run), `mise run docs`, `mise run wasm`, `mise run deny` |

`cargo fmt` and the whitespace hooks fix files for you. When they do, the commit stops: review the changes, stage them, and commit again. Run everything by hand with `prek run --all-files` and `prek run --hook-stage pre-push --all-files`. The configuration is in `prek.toml`.

The first coverage run before a push builds the workspace again with coverage instrumentation, which takes a few minutes. Later runs only rebuild what changed.

### What CI checks

Every pull request runs these jobs. All must pass before merging: the main branch ruleset requires the "CI success" check, which passes only when every job passed. Each job runs a task from `mise.toml`, so the local command is exactly what CI runs.

| Job | What it checks | Run it locally |
|---|---|---|
| Format | `rustfmt` style | `mise run fmt` |
| Clippy | Lint groups set in the workspace `Cargo.toml`: correctness, suspicious, complexity, perf, style, cargo | `mise run clippy` |
| Test | All tests on Linux, macOS (Apple Silicon), and Windows, plus benchmarks compile | `mise run test` (on your OS) |
| Coverage | Line coverage of all tests (run with [nextest](https://nexte.st)), at least the minimum. Sends coverage and test results to Codecov, and coverage to GitHub | `mise run coverage` |
| Docs | API docs build without warnings | `mise run docs` |
| MSRV | Still builds with Rust 1.99 | `mise run msrv` |
| WebAssembly | Engine crates build for `wasm32-unknown-unknown` | `mise run wasm` |
| Licences | Only permissive licences, no banned crates, only crates.io as a source | `mise run deny-policy` |
| Security advisories | No dependency has a RustSec advisory. On a pull request, only when it changes `Cargo.toml`, `Cargo.lock`, or `deny.toml`; always on `main` and in the weekly run | `mise run deny-advisories` |
| Spelling | Typos in code and docs | `mise run typos` |
| ADR status | Every ADR has one allowed status that matches the index, and none is Proposed | `mise run adr-status` |
| Test placement | Tests are in their own files, not inline in code modules ([Where tests live](#where-tests-live)) | `mise run test-placement` |

To change a check, change its task in `mise.toml`. The hooks and CI both pick it up.

`mise run deny` runs both cargo-deny checks at once, as the pre-push hook does when a dependency file changed.

**When the advisories check fails.** A new advisory is published against the RustSec database, not against your change, so CI checks advisories only where fixing one is part of the work: on `main`, in the weekly run, and in a pull request that changes the dependencies. Why: otherwise one new advisory fails every open pull request, docs-only ones too (#143). To fix a failure:

1. Upgrade the crate to a version without the advisory (`cargo update -p <crate>`), or remove it, or replace it with a maintained one.
2. If none of these is possible yet, add the advisory to `[advisories] ignore` in `deny.toml` as `{ id = "RUSTSEC-...", reason = "..." }`. The reason says why it does not affect LoomDSP, or what we wait for. Open an issue to remove the entry later.

Why warnings fail CI but not your local build: a warning while you are still editing is noise, but a warning on `main` is a problem someone else inherits. `mise run clippy`, `docs`, and `wasm` treat warnings as errors, as CI does, so the checks catch them before you push.

### Coverage and code quality gates

Besides "CI success", the main branch ruleset has two gates from [GitHub Code Quality](https://docs.github.com/en/code-security/concepts/code-quality/code-quality).

**Line coverage.** A pull request is blocked when its line coverage is under 80%, or more than 1 percentage point below `main`. The Coverage job uploads a Cobertura report (`target/cobertura.xml`) with [`actions/upload-code-coverage`](https://github.com/actions/upload-code-coverage), and a bot comments on the pull request with the total and the files that changed. It also sends the coverage (`target/lcov.info`) and the test results to [Codecov](https://codecov.io/gh/brylie/LoomDSP), which comments on the pull request too.

- The Coverage job itself fails under 80%, so the minimum holds even when the upload does not happen. *Open point:* GitHub Code Quality is only on GitHub Team and Enterprise Cloud plans. On this repository's plan the upload returns "404 Not Found", so the step only warns, and the "1 point below `main`" rule and the pull request comment do not work yet.
- `mise run coverage` checks the minimum on your machine, and the pre-push hook runs it. The minimum is `LOOM_COVERAGE_MIN` in `mise.toml`. It must stay equal to the ruleset's value.
- The "no more than 1 point below `main`" rule is checked only on GitHub, because it needs the coverage of `main` to compare with. If your change adds code, add tests for it in the same pull request.
- Code that needs audio or MIDI hardware cannot be reached by tests. Keep it thin, and put the logic it calls where tests can reach it, as the app does with the `devices::Host` trait.

**Pull requests from forks.** GitHub gives a fork's workflow a read-only token, so the Coverage job cannot upload its report. It still measures coverage, fails under the minimum, and attaches the report as the `coverage` artifact, so the minimum is still checked. A maintainer checks the rest when reviewing. *Open point:* GitHub's documentation does not yet say whether the coverage rule blocks a pull request with no uploaded report or lets it through. We will record what it does on the next pull request from a fork.

**Code quality findings.** A pull request is blocked by any Code Quality finding of severity "error". Code Quality's rule-based analysis uses CodeQL, which covers C#, Go, Java, JavaScript, Python, Ruby, and TypeScript, but not Rust. So for LoomDSP's Rust code, the quality gate is Clippy: CI runs it with warnings as errors, and the hooks run the same task. The CodeQL checks run only on GitHub, so there is no local command for them. If one blocks your pull request, its finding is shown inline on the pull request.

Dependabot opens weekly pull requests for dependency updates.

### Fuzzing

`fuzz/` holds a [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) target for the patch loader (#44). It reads random input as a patch, loads it into an engine with every built-in module, and plays a few buffers. Any input must give an error or a working engine, never a panic or a hang. cargo-fuzz needs a nightly compiler, so `fuzz/` is its own workspace, CI does not run it, and the engine crates still build on stable and for WebAssembly.

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz
cd fuzz
mkdir -p corpus/patch_loader && cp ../examples/patches/*.ron corpus/patch_loader/
cargo +nightly fuzz run patch_loader -- -max_total_time=300 -dict=patch.dict
```

Run it after a change to the patch format, the loader, or the controller. When it finds an input that breaks the loader, fix the cause, then copy the input from `fuzz/artifacts/` into `crates/loom-patch/tests/malformed/` with a comment that says what it found. `crates/loom-patch/tests/malformed.rs` loads every file there, plus edge cases and seeded random changes of the example patches, on stable in every CI job.

## Rules for audio code

The audio thread has a hard deadline. At 48 kHz with 128-frame buffers, each buffer must finish in 2.67 ms, every time. So code that runs inside `Module::process` or `Engine::process` must not:

- allocate or free memory (no `Vec::push` past capacity, no `Box::new`, no `String`),
- take a lock or wait on another thread,
- make system calls (no printing, logging, or file access).

Why: each of these can take an unpredictable amount of time, and one late buffer is an audible dropout. The test `crates/loom-modules/tests/no_alloc.rs` aborts if the audio path allocates.

Modules must also give the same output whether they process 1 frame or 128 frames at a time. Inside feedback loops the engine calls them one frame at a time (ADR-0002). The test `output_does_not_depend_on_host_buffer_size` checks this for whole patches, and the module test harness checks it for each module.

## Adding a module

Start from the module's entry in the [module catalogue](docs/product/module-catalogue.md), and name its catalogue ID (such as `MT-FILTER-02`) in the issue and pull request. If the module has no entry, propose one first. Design the module in a clean room ([ADR-0018](docs/adr/0018-clean-room-module-development.md)): work from its purpose and from textbooks, papers, and standards, and cite them. Do not read other audio projects' source code, except the source code of a dependency accepted under ADR-0006 when using that dependency. Do not copy their help text, control names, or panel layouts. Why: LoomDSP's modules must be its own, so every patch can be shared under a permissive licence without doubt.

1. Add a `static` `ModuleDescriptor` and a struct implementing `Module` in `crates/loom-modules/src/`.
2. Add it to `ALL` in `crates/loom-modules/src/lib.rs`.
3. Give it a stable, namespaced ID (`loom.<category>.<name>`) and version 1. Never rename a released ID, port, or parameter. Add a new version instead. During the early alpha nothing is released, so renames are allowed when they improve the design ([AGENTS.md](AGENTS.md#project-stage-early-alpha)).
4. Write each port and parameter as a struct literal, with a short `label` and a one-sentence `description`. These are text for people, not IDs, so you can reword them later. The crate does not compile if one is empty ([port and parameter text](docs/design/port-and-parameter-text.md)).

   ```rust
   inputs: &[PortSpec {
       name: "reset",
       label: "Reset",
       signal: SignalType::Gate,
       description: "Starts the count again.",
       default: PortDefault::Value(0.0),
       polarity: Polarity::Unipolar,
       channels: Channels::Downmix,
   }],
   params: &[
       ParamSpec {
           name: "n",
           label: "Division",
           description: "Fires on every Nth trigger.",
           min: 1.0,
           max: 64.0,
           default: 2.0,
           unit: "",
           options: ParamOptions::DEFAULT,
       }
       .integer(),
   ],
   ```

   Every port names its `polarity` ([port ranges](docs/design/port-ranges.md)) and what it does with stereo, its `channels` ([stereo cables](docs/design/stereo.md#declaring-stereo-behaviour)). Most modules write mono code and need no more than the usual choice: audio inputs are `Channels::PerChannel`, so the engine runs the module once per side when stereo arrives; other inputs are `Channels::Downmix`; outputs are `Channels::PerChannel`. Use `Channels::Stereo` only for a module that must see both sides at once, such as a panner.
   Mark whole-number parameters (channels, counts, seeds) with `.integer()` on their `ParamSpec`, so they round and the inspector snaps to whole values.
   Mark frequencies and times with `.logarithmic()`, so equal steps of the control give equal ratios. Controls and modulation both use this mapping ([parameter mapping](docs/design/modulation.md#parameter-mapping)).
5. Choose what each input reads while no cable is connected: its `default`. `PortDefault::Value(x)` is a fixed number, and `PortDefault::Value(0.0)` is silence. An input can also follow the patch transport with `PortDefault::BarPhase`, `PortDefault::BeatPhase`, or `PortDefault::Playing`, so a phase-driven module runs in time with no cable. To share one port definition with a different default, use `PortSpec::with_default`. In `process`, `io.is_connected(i)` tells you whether a cable is there, for example to fall back to a parameter. Rules: [clock and transport](docs/design/clock.md), ADR-0015.
6. To read the transport inside `process`, use `ctx.transport()`. Take the position at frame `i` with `TransportInfo::position_at(i)`, or use `beat_phase_at(i)` and `bar_phase_at(i)`. Do not add a step per frame to the position yourself: it rounds differently at different block sizes, and the module would break block-size invariance.
7. Give it a panel: a `Panel` in its descriptor with a knob for each parameter a performer would turn, and lights for display values. The registry refuses a panel that does not fit (see [module panels on tiles](docs/design/panels.md#module-panels-on-tiles-p1)).
8. Add a reference test to `crates/loom-modules/tests/reference.rs` with the [module test harness](docs/design/module-test-harness.md) (`loom-test`). It checks that block size does not change the output and compares with a short reference render. Create the file with `LOOM_UPDATE_REFERENCES=1 cargo test -p loom-modules --test reference`, listen to it, and commit it. Reviewers can listen to it too. For smaller checks, write unit tests with `loom_core::testing::run`. Once the module is in `loom_modules::ALL`, `tests/non_finite.rs` checks that a NaN or infinite input does not stay in its state: replace a non-finite input with a safe value, or clear the state, rather than keep it. `tests/reset.rs` checks that `reset()` clears the state and snaps parameter smoothing to its targets, and `tests/copy_state.rs` checks that a module with state hands all of it over in `Module::copy_state_from` (stereo spec, rules 25 to 27). For a module whose fields are plain values, derive `Clone` and call `loom_core::copy_state(self, other)`; copy buffers into the ones the module already has, never allocate.
9. Generate its help page with `mise run help-pages` (or `cargo run -p loom-cli -- modules --markdown docs/modules`), and commit the new page in [`docs/modules/`](docs/modules/README.md). Write longer notes and an example patch below the marker line at the end of the page; regenerating keeps them. A test fails while a page is out of date, so run it again whenever you change a descriptor ([help pages](docs/design/help-pages.md)).
10. Run the full test suite, then try the module in the app: `cargo run --release -p loom-app`, press **A** with the pointer over the canvas (or click **Add module** in the toolbar), type part of its name, and press Enter ([module browser](docs/design/module-browser.md)).

## Working on the canvas and app

The canvas (`crates/loom-canvas`) must stay fast on high-resolution displays. Its cost should follow the number of things drawn, not the number of pixels. So:

- Do not rebuild GPU data every frame. It is rebuilt only when the scene's generation changes; pan and zoom change one uniform.
- Do not add shaders that run over the whole window for each shape.
- In `canvas.wgsl`, avoid names that are reserved in Metal or HLSL (`half`, `in`, `out`, `sample`, and others). The `shader_names` test checks the list.

To check the app without a person at the screen, `loom-app` has these options (see `loom-app --help`):

- **Performance.** `cargo run --release -p loom-app -- --generate 300 --bench` measures the frame rate with the camera moving; read the bottom bar. `--measure` does the same with the camera still. Both print the numbers to stdout once a second, with counts of scene and control rebuilds. `--bench-seconds N` exits after N seconds with a final line, so a script can run the check. `--measure-idle` prints the same without forcing a redraw, to check that an idle patch draws nothing. Results and the method are in [docs/benchmarks/p1-canvas.md](docs/benchmarks/p1-canvas.md).
- **No audio device.** `--fake-audio` runs the engine from a timer, so live values, lights, and meters move on a machine without audio. `--record out.wav` saves what the app plays until it quits, and `loom analyze out.wav` measures it: peak, clipping, the largest step between samples, and DC ([audio QA spec](docs/design/audio-qa.md)).
- **Driving the running app.** `--control SOCKET` lets a script drive the app by name, one JSON command per line: select, set a parameter, play a note, read the UI snapshot, find a control in the window, or save a window screenshot. `--control-script FILE` runs the same commands from a file. See [docs/design/control-channel.md](docs/design/control-channel.md).
- **macOS automation.** `scripts/bundle-macos.sh` wraps the release build in `LoomDSP.app`, which desktop automation tools can attach to (development only, not signed).
- **Screenshots.** For UI changes, add a screenshot to the pull request. `loom-app <patch> --screenshot out.png` saves one without a window. `--zoom`, `--center`, `--scale`, and `--focus` set what it shows. Other options show a state that would need a person at the screen: `--pointer` puts the pointer on a point or a named control or port, `--hover-text` shows its hover text at once, `--pin` pins an output's modulation badge, `--wire` shows a cable being dragged to the pointer, and `--depth` shows a knob's range arc being dragged. `--screenshot-window out.png --select 4` saves the whole window instead: toolbar, canvas, inspector rows for the selected module, and stats bar, at 1440x900 unless `--size` says otherwise. The canvas options apply to the canvas inside it.
- **GPU smoke test.** `crates/loom-app/tests/screenshot.rs` draws the canvas through a headless GPU device, so `canvas.wgsl` compiles for the platform backend. It skips where no GPU adapter is found. Set `LOOM_REQUIRE_GPU=1` to make that a failure instead; the macOS and Windows CI test jobs set it.
- **What the UI shows.** `loom-app <patch> --dump-ui --select 4` prints it as JSON: tiles with their control values, the inspector rows for the selected module, the status bar, the undo steps, and the window title. `--frames 4800` first runs the engine for that many frames on silent input, so knobs show their live values, and lights and meters show where the engine left them; it works with `--screenshot-window` too. In app tests, `App::ui_snapshot()` returns the same data, and `App::process(frames)` runs the engine by hand, so live values, lights, and meters can be tested without an audio device.

## Dependencies

Only permissively licensed dependencies are allowed (MIT, Apache-2.0, BSD, ISC, Zlib, and similar). No GPL, LGPL, AGPL, or other copyleft licences. See [ADR-0006](docs/adr/0006-dependency-and-licence-policy.md). Add a short dependency review to the pull request for any new dependency.

## Pull requests

- Keep them focused: one change per pull request.
- Include tests.
- For a feature, link its specification and say which acceptance criteria the change meets. If the change differs from the spec, update the spec in the same pull request.
- Explain what changed and why, in plain language.
- If you changed a module descriptor's text, ports, or parameters, run `mise run help-pages` and commit the pages in `docs/modules/`. A test fails while they are out of date.
- CI must pass.

## Writing style for docs

Short, direct sentences. Plain words. Explain the reason behind a rule, not only the rule. Many contributors read English as a second language.

## Licence

By contributing, you agree that your contribution is licensed under Apache-2.0.
