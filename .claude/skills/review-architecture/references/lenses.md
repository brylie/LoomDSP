# Lenses for an architecture review of LoomDSP

Each lens says what to look for, where it has bitten this workspace before, and what to check before calling something a finding. The examples are from the review of November 2026 on `main` at a671e30. Line numbers there have moved since; use them to find the pattern, not the line.

## Contents

1. Correctness in repeated plumbing
2. Real-time safety
3. Rules the tests cannot see
4. Boundaries and layering
5. Shape: oversized and god objects
6. Stringly-typed seams
7. Duplication
8. Testability
9. Performance
10. Choices that look like debt and are not
11. Traps that gave false findings

---

## 1. Correctness in repeated plumbing

When a feature needs the same five or six manual edits in every module, some module has missed one. Look for a convention that is enforced by habit and ask which implementations break it.

- **Smoothed parameters and `param_glide`.** A module that smooths a parameter itself must implement `Module::param_glide` so the engine can take over the glide when the first modulation assignment arrives (`crates/loom-core/src/compile/schedule.rs`, `take_smooth_state_from`). Three built-in modules had the parameters and not the hook. The script lists candidates; read each, because one named case (Mixer mutes) turned out harmless when traced.
- **Linear and logarithmic glides.** A `.logarithmic()` parameter should glide in log space, as the engine's fill does. Check that each module's own smoother for such a parameter runs in pitch or normalised space, not in Hz or Q.
- **Named indices.** Every parameter and port index is a named constant looked up from the descriptor (#468). A bare number in `set_param` or `io.split` is a finding.
- **Reset and state copy.** A module with state needs `reset()` and `copy_state_from`. Check new modules for both.

For each pattern: find one correct implementation, grep for the others, and read the ones that differ. Then propose the test that makes the next omission fail the build, usually a loop over `loom_modules::registry()`.

## 2. Real-time safety

Nothing reachable from `Engine::process` or `Module::process` may allocate, free, lock, wait, print, or do I/O. `tests/no_alloc.rs` catches allocation; it does not catch a lock, a syscall, or a `println!` behind a flag.

- Grep the engine and modules for `Mutex`, `RwLock`, `println!`, `eprintln!`, `log::`, `std::fs`, `Instant::now`, and `format!` reachable from `process`.
- Look at what happens when a bounded queue is full: telemetry must drop, never wait.
- Check that every `unsafe` block has a `// SAFETY:` comment that states the invariant and where it is guaranteed, and whether a `debug_assert!` could check it in tests. Raw-pointer aliasing in `PortIo` is guaranteed by the compiler's buffer layout; a `debug_assert_ne!` on the pointers is cheap insurance.
- Ask whether raw pointers could be private to the module that creates them. In `loom-core`, `Schedule`'s pointers were read by `Engine::run_block`; moving the loop onto `Schedule` would keep them in one file.

## 3. Rules the tests cannot see

AGENTS.md states rules that CI only partly enforces. For each, ask what would break it without a test failing.

- **Portability (rule 6).** The wasm task only builds the five portable crates. Rust's `std` compiles `std::fs` and `std::process` as stubs on `wasm32-unknown-unknown`, so a filesystem call in `loom-patch` builds and then panics or errors at runtime. The script lists such calls; check each is gated or belongs elsewhere.
- **Determinism.** Random sources are seeded. Look for `HashMap` iteration that reaches output order, `Instant`, or thread timing on the audio path.
- **Licences.** `cargo deny` covers crates. It does not cover code pasted from elsewhere; the clean-room rule is a review question, not a tool.
- **Block-size invariance.** Reference tests run block sizes 1, 77, and 512. A module that keeps a per-block value across `process` calls can pass them and still differ at another size. Read for state updated once per call rather than once per sample.
- **Stable IDs.** Nothing is released, so renames are allowed. Check that a rename updated every patch, test, and reference render in the same change.

## 4. Boundaries and layering

- **Crate coupling.** For each dependency between workspace crates, ask what it is used for. One constant, one type, or one function is a smell: `loom-analysis` depended on all of `loom-core` for `safety::CEILING`. The script prints the graph; `grep -rn 'loom_core::' crates/X/src | sort | uniq -c` shows how deep a dependency goes.
- **Engine crates stay UI-free.** `loom-core`, `loom-dsp`, `loom-modules`, `loom-patch`, and `loom-rt` never depend on Iced, cpal, or the app. Check `Cargo.toml` and `use` lines.
- **Model and view (ADR-0021).** A view draws from a plain-data model that `ui_snapshot` reports. A view that queries `&Graph` or `&Controller` while building widgets is a finding, because a headless test cannot see what it shows.
- **Persistence and layout.** Whether two parameters form a pair is a parameter fact; which widget shows them is a panel fact. Look for model rules inferred from the UI description, and judge whether the inference is a shortcut or the design (see section 10).
- **Desktop concerns in the app.** Device enumeration, hot-plug, and recovery are host concerns. If `loom-cli` and `loom-app` both need them, they belong in `loom-host-desktop`.

## 5. Shape: oversized and god objects

Size is a map. The finding is what the size makes hard.

- **A struct with many fields across several concerns.** Ask what happens when one concern is replaced. `App::replace` built a second `App` and copied eleven runtime fields by hand because patch state and app-lifetime state shared one struct; a forgotten field was reset silently on Open. The remedy is ADR-0034's pattern: a flat API over typed parts (`Controller` with `Slots`, `MidiSync`, `Probes`, `LiveValues`), or `InputSession` beside `Rig`.
- **`impl` blocks spread over many files.** It means the type's fields are read everywhere, so no invariant can be local. The script counts files per type.
- **Flag proliferation.** Several `Option` and `bool` fields that must agree, repaired by `close_stale_*` helpers on every event, want an enum whose variants cannot disagree. Seven booleans on a widget builder want one policy struct.
- **A large `Message` enum with a split dispatch.** Group variants by the view that owns them, so `update` arms sit beside their views.
- **A file holding several concerns.** `descriptor.rs` held port specs, parameter curves, text parsing, note formatting, panel layout, and a button state machine. Each is fine; together they make every change touch the same 2,000-line file. Propose a directory with one file per concern, and no behaviour change.

For functions, the script lists the longest. Read them for several levels of loop and branch, repeated blocks, and a name that says "and".

## 6. Stringly-typed seams

Text that one side formats and the other side parses is a seam that breaks on a rewording and that tests rarely cover.

- Error prose stripped back to a cause (`unwrap_reopen_error`).
- Warnings returned as `Vec<String>` and classified with `contains(...)`.
- JSON appended as a comment to a RON clipboard payload.
- Ids compared by string in several places where a `ModuleRole` already exists for similar cases (`AudioOutput`, `UnitDelay`).
- A modal struct opened only to borrow its validated list, then closed.
- A graph rebuilt by diffing an undo snapshot against an empty controller, because no direct clone exists.

The direction is usually the same: a typed value across the seam, prose formatted once at the boundary, and the existing enum extended.

## 7. Duplication

The script finds the mechanical cases: WAV readers and writers, `thread::spawn` loops, temp-directory helpers in tests, offline render loops. Beyond those:

- The same formula in several modules (dB conversions, meter floors). The home is `loom-dsp`, pure and inlined.
- The same lookup in several app files (`MODULE:NAME` targets, resize clamps, snap). Route them through one helper.
- The same state machine in two crates (a typed-value draft in the inspector and in the canvas). One model, two views.
- Mirrored enums across a crate boundary. Before calling it duplication, check why the boundary exists (section 10).

A duplicate is worth filing when the copies have already drifted, or when one has a fix the other lacks. Say which.

## 8. Testability

- **Binary-only crates.** Without a `lib.rs`, every test spawns the binary, and useful code inside (a graph fuzzer, a renderer) cannot be reused in `cargo test`.
- **Missing unit-test files.** Tests live in `foo/tests.rs`. A large core file with none relies on integration tests to reach its branches. The coverage gate holds at 80% overall, so look at the file, not the total.
- **Copied test helpers.** Each copy is a place a fix does not reach.
- **Routing with two paths.** When the headless test harness and the real window route an event differently, a test can pass on behaviour the app does not have. The project fixed this once (`input_owner.rs`); look for the next pair.
- **Structural invariants with no test.** For each "every X must Y" rule you relied on, ask what test enumerates X.

## 9. Performance

Only the audio thread and the render loop matter here, and only with a number.

- **Per-sample work that could be per-block:** a port lookup inside the frame loop, a linear scan per sample, a `powf` round trip per sample on a modulated logarithmic parameter.
- **GPU rebuilds per frame.** ADR-0010: tiles, ports, and cables rebuild only when the scene generation changes. A new overlay that rebuilds every frame is a finding.
- **Thread churn.** A subscription whose hash includes a list that changes on pan or zoom restarts its thread on every pan or zoom.

Report with a benchmark where one exists (`cargo bench -p loom-modules`, `docs/benchmarks/`), or say that the cost is unmeasured and propose the bench. A per-sample scan is a finding on its own; its size is not.

## 10. Choices that look like debt and are not

Each of these was flagged by a review and is a decision. A review may still say the decision's cost has grown, but then it names the ADR or issue and asks for a new decision.

| Looks like | Is |
|---|---|
| `WidgetKind` in `loom-core` mirrored as `WidgetViewKind` in `loom-canvas` | `loom-canvas` has no dependency on `loom-core` by design (ADR-0010), so the canvas stays a plain widget crate. The mirror is the price of that boundary. |
| Nine overlay `Option`s on `App` read in several places | Since #569 one function, `input_owner()`, reads them. The `Option`s as source are still a hazard (#624), but the divergent guards are gone. |
| `test.*` modules compiled into the app | Decided in #469: the feature is on so that `--test-modules` works; registration is gated by the flag, so users never see them. |
| Range pairing found by scanning the panel for a `Range` widget | The widget must declare the pair anyway; declaring it twice would be the duplication. |
| Display and live tables fenced on the module id | The key carries the id, so a recycled slot can only return the same logical module's stale value for one frame (#620). Real, and Later. |
| `Controller` has `connect` and `assign_modulation` by name only | Edit-path cost, not audio-path. Minor. |
| Patch format version stays at 1 through the alpha | AGENTS.md: migrations start with the first release. |

Add to this table when a review finds another, because the next reviewer will trip on the same one.

## 11. Traps that gave false findings

- **Line numbers and counts from a pasted review.** They were right when written. Re-count with the script; re-read the lines.
- **A fix already on `main`.** The two loudest structural claims in the November review described code that commit d13ece9 had replaced. Check `git log -S'<identifier>'` and the recent commits for the area before writing a structural finding.
- **"Missing" that is harmless when traced.** The Mixer's mutes had no `param_glide`; the module's own smoother keeps ticking under modulation, so nothing steps. Trace the path before counting it.
- **"Unclamped" that is clamped.** The screenshot camera clamped zoom to `[MIN_ZOOM, 1.0]`; what it lacked was the readable floor. Say exactly what differs.
- **A smaller twin the review missed.** The LFO's linear glide was named; the EQ's mid Q had the same mismatch two files away. Grep the pattern after confirming one case.
- **A crate boundary read as coupling.** The absence of a dependency can be the design (section 10).
- **Backlog overlap.** About a third of a review's recommendations already had open issues. Search before "not filed", and search closed issues before "nobody decided".
