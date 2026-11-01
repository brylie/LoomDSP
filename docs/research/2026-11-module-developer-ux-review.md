# UX Review: Writing a Module

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at fd6f938 (after #485).
- **Follows:** [the learner review](2026-11-learner-ux-review.md) and the other persona reviews from the same day.
- **Used by:** pull request #516 (findings 1 and 2) and issues #517 and #518. Decisions go in specs, ADRs, or the contributor docs.

This review reads LoomDSP as Leo, the module developer in the [personas](../product/personas-and-use-cases.md): a Rust programmer interested in DSP who "wants to write a new filter or oscillator and have it appear in the module browser", and needs "a documented module SDK, a test harness, examples, and a clear contribution process". Here the interface is not the app's window but the contributor docs, the Rust API, the tests, and the feedback they give. For an open-source project that tags issues for newcomers, this path decides whether a first contribution becomes a second. Do not update this snapshot later; write a new one after the fixes.

## Method

- Followed the path of a first module: README, CONTRIBUTING (setup, the rules for audio code, and the 11 steps of Adding a module), the module catalogue, ADR-0018 (clean room), and the issue template for modules.
- Read the `Module` trait (`crates/loom-core/src/module.rs`), `ModuleDescriptor`, `PortSpec`, and `ParamSpec` (`descriptor.rs`), the smallest module (Subtract, `crates/loom-modules/src/math.rs`), a reference test, and the checks that run over every module (`tests/no_alloc.rs`, `non_finite.rs`, `reset.rs`, `copy_state.rs`).
- Compared the code example in CONTRIBUTING with the current structs.
- Timed `cargo test -p loom-modules` on an already built target, and read the `mise` tasks.
- Looked at the open module issues that are ready to build, for labels and design references.

**Limits.** No new module was written from scratch, and no newcomer was watched. One timing on one machine.

Priority means: **High** blocks the task or misleads the developer; **Medium** slows them or looks broken; **Low** is polish.

## Summary

The foundations are better than in most audio projects. The `Module` trait has one required method; every other method has a default and a comment that states its real-time rules. Port and parameter indices are looked up by name at compile time, so a typo is a build error. A reference test is five lines with the test harness, and it checks block-size invariance by itself. Once a module is in `ALL`, the NaN, reset, and state-copy tests cover it with no extra work. The `loom-modules` tests run in about 40 seconds. There are explainers for real-time safety and block-size invariance.

Two things undermine that trust. The first code a contributor copies from CONTRIBUTING no longer compiles. And the no-allocation test, which CONTRIBUTING names as the guard for the audio thread's most important rule, runs the `process` of only the modules that appear in five example patches: most modules, and every new one, are never checked. Around them, the path is long and unguided: eleven steps in several files and no template, ready module issues that are neither labelled for newcomers nor give the design references the clean-room rule depends on, and a setup section that opens with worktree and external-drive machinery.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Docs | CONTRIBUTING's `PortSpec` example does not compile: `polarity` was removed, `range` is missing | High |
| 2 | Safety net | The no-allocation test runs `process` only for modules in five example patches | High |
| 3 | Path | Eleven steps, several files, no template; a 4-line module needs about 40 lines of descriptor | Medium |
| 4 | Issues | Ready module issues have no newcomer label and cite no design references | Medium |
| 5 | Setup | The setup section opens with worktree and external-drive scripts | Medium |
| 6 | API | `io.split` binds ports by position, and nothing checks it | Low |
| 7 | Reach | A module of your own means a fork: no out-of-tree modules until M4 | Low |

## Findings

### 1. The first code example does not compile (High)

Step 4 of Adding a module shows a `PortSpec` with `polarity: Polarity::Unipolar`, and the text says "Every port names its `polarity`". The [port ranges spec](../design/port-ranges.md) removed that field: "`PortSpec::polarity` is removed: the range replaces it". Today a `PortSpec` has `name`, `label`, `signal`, `description`, `default`, `range`, and `channels` (`crates/loom-core/src/descriptor.rs`). A newcomer who copies the example gets two errors: an unknown field and a missing one.

The example is the first code Leo meets, so it also teaches that the docs may be wrong. Nothing checks code in CONTRIBUTING.

Direction: fix the example, and keep it true from then on: point to a real module file instead of a copy, or make the example a compiled doctest or a compiled template (finding 3).

### 2. The no-allocation test misses most modules (High)

CONTRIBUTING says "The test `crates/loom-modules/tests/no_alloc.rs` aborts if the audio path allocates." Its main test, `audio_thread_never_allocates`, loads five example patches (`sine`, `subtractive`, `fm-feedback`, `fm-feedback-delay1`, `plucked-string`) and runs the engine under the aborting allocator. Those patches use 14 module types. `ALL` has 47. A few more are covered by their own tests (Delay Line, All-Pass, Audio In), and `copying_state_never_allocates` loops over `ALL`, but only for `copy_state_from`, not `process`.

So a new module that allocates in `process` passes every check, unless someone also adds it to an example patch. This is hard rule 1, and the test a contributor is told to trust does not cover their code.

Direction: run `process` for every DSP module in `ALL` under the allocator, as `tests/non_finite.rs` already loops over `ALL` with the harness. Drive each input with a moving signal and turn each parameter across its range, so smoothing and lazy paths run too.

### 3. A long path with no template (Medium)

Adding a module means: a descriptor and a struct in `crates/loom-modules/src/`, an entry in `ALL`, a reference test in `tests/reference.rs`, a reference WAV, a help page from `mise run help-pages` with notes below the marker, a catalogue entry (before starting), and a panel that fits. CONTRIBUTING lists all of it, clearly, in 11 steps, but there is no file to copy from and no command that checks a new module is complete. Subtract has 4 lines of DSP and about 40 lines of descriptor, because every port and parameter is a full struct literal (ADR-0016, a deliberate choice for readable, checked text).

Direction: a template module, commented step by step, that is compiled in CI (so it can never drift, unlike finding 1) and that CONTRIBUTING points to as "start here". Optionally a `mise run new-module` task that copies it and adds the `ALL`, reference test, and help page entries.

### 4. Ready module issues are not set up for newcomers (Medium)

CONTRIBUTING says "New modules are often a good first contribution". About ten module issues are ready to build (no `needs spec agreed`): Random #304, Chance #305, Euclidean rhythm #306, the sequencer #307, the pitch quantiser #308, scale and tuning #309, Selector #310, and Phase reset #174. Only one open `area:modules` issue has `good first issue`, and none of these does.

The clean-room rule (ADR-0018) says contributors must work from textbooks, papers, and standards, never from other projects' code, and that "good references in each issue make this easier". The module issue form asks for design references. None of the four ready issues checked (#304, #306, #308, #174) cites one. A newcomer is told what not to read, but not what to read.

Direction: label the ready, self-contained module issues `good first issue` or `help wanted`, and add one or two references to each. A short shared reading list (for example in `docs/learn/README.md`) would serve every issue at once.

### 5. Setup opens with advanced machinery (Medium)

After installing Rust and mise, the Build and test section explains worktree trust, a script that points each worktree at a target folder on another drive, and a script that prunes old targets, before it reaches `cargo build`. These are useful for the maintainers' many parallel worktrees. For a first contributor with one clone they are noise, and they make the setup look harder than it is.

Direction: keep the short path first (install, `mise trust`, `mise install`, `cargo build`, `mise run test`), and move worktrees and target folders to a later "Working with many worktrees" section.

### 6. `io.split` binds by position (Low)

`io.split::<N, M>()` returns the ports in descriptor order; the names in the pattern are only names. CONTRIBUTING says so, and gives the example: swapping Subtract's inputs without its pattern would compute `b - a`, and nothing checks it. Most reference tests drive only one input, so they would not catch a swap.

Direction: a harness option, or a check over `ALL`, that drives each input with a distinct signal and fails when an output does not change as the descriptor says. Not filed; worth noting in the template (finding 3).

### 7. A module of your own means a fork (Low)

Third-party modules are compiled into the main build from source (FR-SDK-04) and distributed without a rebuild only later (FR-SHARE-05, M4). Until then, a module that is not accepted upstream lives in a fork. This is a known, planned limit; it matters here only because Leo's need ("have it appear in the module browser") is met only through a pull request.

## What works well

- **One required method.** `Module` asks only for `process`. `set_param`, `reset`, `copy_state_from`, `displays`, and `param_glide` have defaults, and each comment says which real-time rules apply.
- **Names, not numbers.** `const CUTOFF: usize = SVF.param_index("cutoff").unwrap();` fails the build on a typo, and reordering the descriptor moves every index with it.
- **Text is required.** An empty label or description is a build error, so every port explains itself in the app.
- **A five-line reference test.** `Harness::new(&osc::SAW).seconds(0.05).input("phase", Signal::phase(220.0)).check(reference!("saw"))` checks block sizes 1, 77, and 512 and compares with a WAV a reviewer can listen to.
- **Free checks.** Being in `ALL` is enough for the NaN, reset, and state-copy tests to cover a module.
- **Clear panel errors.** A widget outside the panel or over another one fails with the widget's name.
- **Help pages are generated** from the descriptor, keep hand-written notes, and a test fails when one is out of date.
- **A fast loop.** `cargo test -p loom-modules` ran in about 40 seconds on a built target.
- **Theory for the hard rules.** Explainers cover [real-time safety](../learn/real-time-safety.md) and [block-size invariance](../learn/block-size-invariance.md).
- **A module issue form** asks for the catalogue ID and design references.

## Issues

Grouped to keep the backlog small. Findings 1 and 2 were fixed at once rather than filed. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#516](https://github.com/brylie/LoomDSP/pull/516) (pull request): fix the CONTRIBUTING and architecture examples, and check every module's `process` for allocation | 1, 2 | | | |
| [#517](https://github.com/brylie/LoomDSP/issues/517) A module template compiled in CI, and a shorter setup path | 3, 5, 6 | M2 | P1 | S |
| [#518](https://github.com/brylie/LoomDSP/issues/518) Module issues ready for newcomers: labels, design references, and a reading list | 4 | M2 | P2 | S |
| Not filed here: out-of-tree modules (FR-SDK-04, FR-SHARE-05) | 7 | | | |

## Suggested order

1. The no-allocation test over every module (2). Done in #516.
2. Fix the CONTRIBUTING example (1), done in #516; then the template, so it cannot drift again (3, #517).
3. Label and reference the ready module issues (4, #518), so the next newcomer has a place to start.
4. Shorten the setup path (5, #517).
