# Roadmap

The roadmap is split into phases. Each phase ends with something usable and has clear exit criteria. No dates yet. We should estimate after Phase 0 shows how fast the core comes together.

## Overview

Phases are numbered M0 to M4, to match the GitHub milestones. P0 to P2 are issue priorities, so they are kept for that. Accepted ADRs, research snapshots, and benchmark reports are frozen records, so they still say P0 to P4 for the same phases.

| Phase | Name | Main outcome |
|---|---|---|
| M0 | Engine foundations | Headless engine that plays a patch file with live MIDI |
| M1 | Desktop patcher and live basics | Build and play a synth from primitives, with MIDI controllers |
| M2 | Encapsulation, panels, performance | Sub-patches, front panels, polyphony, performance mode, set lists |
| M3 | Living systems | Loopers, groups and next actions, counters and logic, scenes, sequencers, note effects |
| M4 | Sharing and ecosystem | Self-contained shareable patches, patch library, module SDK, community modules |
| Later | Not scheduled | Plugin and standalone export (CLAP, VST3), web target. Revisit after M3. |

Why this order:

1. The engine and its real-time rules are the hardest part to change later, so they come first.
2. Live performance is the main purpose of the app. MIDI controller support starts in M1, not after export.
3. Living systems (M3) are what LoomDSP is for: patches that play, evolve, and arrange themselves on stage.
4. Sharing comes next (M4), following VCV Rack: people swap patches and modules, and the ecosystem grows around the core.
5. **Scope is kept tight on purpose.** Generative live performance and plugin export pull in different directions: plugins bring host integration, parameter automation, many formats, and per-platform builds and signing. Doing both at once risks doing neither well. So plugin and standalone export, and the web target, are deferred ("Later"). We decide after M3 whether to take them on.
6. The engine core stays portable and WASM-compatible (NFR-PORT-01, NFR-PORT-02), and ADR-0004 records how export would work, so the option stays open at low cost.

**Module types per phase.** The [module catalogue](module-catalogue.md) gives every planned module type a stable ID (such as `MT-FILTER-02`) and a phase. Each phase below builds the catalogue entries marked with that phase. A type's phase follows the use case it unblocks: the subtractive synth in M1, panels and live play in M2, self-arranging sets in M3, and the rest in M4.

---

## M0. Engine foundations

**Goal:** prove the core architecture with no UI.

Scope:
- Cargo workspace layout, for example `loom-core` (engine), `loom-modules`, `loom-patch` (RON format), `loom-rt` (real-time queues, a candidate for its own crate), `loom-host-desktop` (audio and MIDI I/O), `loom-cli`.
- Module trait with block and per-sample entry points (FR-SDK-01).
- Runtime graph with atomic schedule swap (FR-GRAPH-01, 02, NFR-RT-05).
- Feedback regions run per sample (ADR-0002), with benchmarks (NFR-PERF-08).
- SPSC queues for control and telemetry (NFR-RT-04).
- Patch format v0 in RON, with version field (FR-FILE-01, 02, 04).
- About 10 modules: phase generator, sine, saw, SVF, ADSR, add, multiply, one-sample delay, MIDI note in, audio out.
- Offline renderer (FR-GRAPH-06).
- CLI: `loom play patch.ron`, `loom render patch.ron out.wav`.
- CI: tests, `assert_no_alloc`, WASM build check, `cargo-deny` licence check.
- UI spike (OQ-02): Iced with `iced_nodegraph`, 100 nodes, cables, pan and zoom, a live scope. (Done without the scope, which needs the M1 telemetry path.)
- Repository basics: `CONTRIBUTING.md`, AI contribution policy, agent context file, issue templates.

Exit criteria:
- [x] A patch file plays through the default audio device with live MIDI input. (`loom play`.)
- [x] Adding and removing modules from a test script while audio plays causes zero dropouts over 10 minutes. (`loom stress --seconds 600`: 11,026 edits, 0 overruns, max load 4.1%. See [p0-engine.md](../benchmarks/p0-engine.md).)
- [x] Feedback benchmark results recorded. ADR-0002 confirmed ([p0-engine.md](../benchmarks/p0-engine.md)).
- [x] Offline renders are bit-identical across runs, and across host buffer sizes.
- [x] UI spike results recorded. ADR-0009 decided: Iced accepted for the app; both node-editor crates rejected for the canvas, which becomes our own widget (ADR-0010).
- [x] NFR *(proposal)* numbers reviewed against real benchmarks. Engine numbers are well inside NFR-PERF-01. NFR-PERF-04 restated for the reference laptop (Retina display, full window).

## M1. Desktop patcher and live basics

**Goal:** a musician can build a synth from primitives and play it with a MIDI controller.

Scope:
- Our own patch canvas as an Iced widget (ADR-0010, FR-CANVAS-09 to 11): compact tiles on a snap grid, inspector panel, level of detail.
- Module panels on tiles (ADR-0013): built-in modules declare controls and displays, edited in place. First widgets: knob, toggle, radio buttons, light, scope (FR-CANVAS-10, FR-LIB-18).
- Scope module and "Add scope" on a cable (FR-LIB-17, FR-VIS-02).
- Desktop app with canvas, module browser, cables, undo/redo (FR-CANVAS-01 to 06).
- Polymorphic signals and colours, inferred channel count (FR-SIG-01 to 07).
- Feedback with automatic single-sample delay (FR-GRAPH-03, 04).
- Built-in modulation on parameters, without cables: assignments, depth arcs, and the modulation matrix (ADR-0014, FR-MOD-01, 03 to 08).
- Transport, default inputs that follow it, and the Clock, Clock Divide, Phase Scale, and Phase Trigger modules (ADR-0015, FR-LIB-25 to 27).
- Phase 1 module library (FR-LIB-01 to 07, 11).
- Signal activity on ports, without animating cables (FR-VIS-01).
- Audio and MIDI device setup, hot-plugging, audio input (FR-APP-01, 03, 04, FR-MIDI-08).
- MIDI learn, panic, CPU meter (FR-MIDI-02, FR-PERF-04, 05).

Progress:
Issues for this phase are in the [M1 milestone](https://github.com/brylie/LoomDSP/milestone/1).

- [x] Canvas prototype: `loom-canvas` and `loom-app`, with tiles, typed ports, cables, level of detail, inspector, live audio, every MIDI input connected, and a stats bar. 120 fps (the display maximum) with 300 modules and the camera moving on the reference laptop ([p1-canvas.md](../benchmarks/p1-canvas.md)). The zoomed-in and idle measurements are still open ([#4](https://github.com/brylie/LoomDSP/issues/4)).
- [x] Engine-to-UI telemetry ([#6](https://github.com/brylie/LoomDSP/issues/6)).
- [ ] Module panels on tiles and the scope module (ADR-0013) ([#6](https://github.com/brylie/LoomDSP/issues/6) to [#10](https://github.com/brylie/LoomDSP/issues/10)).
- [ ] Module browser, undo/redo, copy and paste, open and save ([#11](https://github.com/brylie/LoomDSP/issues/11) to [#15](https://github.com/brylie/LoomDSP/issues/15)).
- [ ] Modulation on parameters, MIDI learn, panic, device selection and hot-plugging ([#19](https://github.com/brylie/LoomDSP/issues/19) to [#29](https://github.com/brylie/LoomDSP/issues/29)).
- [ ] Phase 1 module library, test harness, and help pages ([#30](https://github.com/brylie/LoomDSP/issues/30) to [#44](https://github.com/brylie/LoomDSP/issues/44)). The tutorial moved to M2 ([#45](https://github.com/brylie/LoomDSP/issues/45)).

Exit criteria:
- [ ] A subtractive synth can be built from primitives in the app, with the module browser, inspector, and cables, and played from MIDI. (The written tutorial, tested with newcomers, is a M2 exit criterion: it is written once the foundation has settled, so it is written once.)
- [ ] Play that synth with a MIDI keyboard and map 4 knobs with MIDI learn.
- [x] An FM feedback patch sounds the same at buffer sizes 32 and 1024. (Already true in the M0 engine; covered by tests.)
- [ ] Unplugging and replugging the MIDI controller during playback works without a restart.
- [ ] Canvas holds 60 fps at full window size on the reference laptop, with 100 modules visible and 300 in the patch, including 4 live scopes (NFR-PERF-04).

## M2. Encapsulation, panels, performance

**Goal:** complex instruments stay manageable and are ready for the stage.

Scope:
- Sub-patches, nesting, exposed parameters, user library (FR-ENCAP-*).
- Polyphony and voice allocation (FR-SIG-08, FR-VOICE-*).
- Panel editor, the same for patches and sub-patches, and the full widget set (FR-PANEL-*). A sub-patch's front panel is its tile.
- Performance mode, presets, set lists, layers (FR-PERF-01, 02, 06, 07).
- Per-device controller mappings, MIDI clock, MIDI out, MPE, pickup modes (FR-MIDI-03, 04, 06, 07, 09).
- Effects and sequencing modules (FR-LIB-08, 09).
- Modulators as modules (FR-LIB-12), including drawn modulation curves.
- Module data beyond parameters, for patterns and samples (FR-LIB-19, OQ-17).
- Cable styles for stereo, polyphony, and inserted delays (FR-CANVAS-12).
- Design spike: how notes travel on cables (OQ-16), so M3 can build note effects on a settled model.
- Roles, slots, and traits (ADR-0017): a specification first, then slots in abstractions, sub-patch modes, role-bound panel widgets, and traits in the SDK (FR-ENCAP-08 to 10, FR-PANEL-11, FR-SDK-07, 08). Patch-wide scale and tuning (FR-LIB-29).
- The catalogue's M2 module types, including the first abstractions: a synth voice and an effect chain with swappable slots.
- Patch authors, tags, and licence, and a browser for the user's own patches that filters by them (FR-FILE-08 to 10, FR-SHARE-06, ADR-0023, [specification](../design/patch-metadata.md)).

Exit criteria:
- [ ] An 8-voice polysynth built from primitives, with a 6-knob panel, runs within the CPU budget.
- [ ] A 30-minute test set with 4 patches, preset changes, and external MIDI clock runs with zero dropouts.
- [ ] One-hour live soak test with edits during playback: zero dropouts, no memory growth.
- [ ] Tutorial: build a subtractive synth from primitives in under 20 minutes (tested with 3 people who did not write the tutorial) ([#45](https://github.com/brylie/LoomDSP/issues/45)). Written last in this phase, when sub-patches, polyphony, and panels have settled.
- [ ] First real live performance with LoomDSP.

## M3. Living systems

**Goal:** patches that play, evolve, and arrange themselves. Arrangement happens inside the grid, through loopers, groups, logic, and scenes, not on a timeline (ADR-0011).

Scope:
- Arrangement from phase-driven primitives (ADR-0012, [specification](../design/arrangement.md)):
  - Note and audio cycles that read the clock's bar phase and output an end trigger (FR-LIB-20).
  - Counter (with phase output) and Step Select (FR-LIB-22), with Clock Divide from M1 (FR-LIB-27). These small logic modules can be built as early as M1.
  - Built-in abstractions made from them: Looper Group, Part, Song Sequencer (FR-LIB-21, FR-LIB-24).
- Scenes: snapshots of module states, switched by triggers, with morph time (FR-LIB-23).
- Sequencers, including polymetric multi-output step sequencers (FR-LIB-14).
- Note effects, on the model chosen in OQ-16 (FR-LIB-13).
- Modulation curves over bars and sections (FR-LIB-16), on the M1 transport and clock modules.
- Record the live output to an audio file while performing (FR-SHARE-03).
- Seeded melody and rhythm generators (FR-LIB-28), and the rest of the catalogue's M3 module types.

Exit criteria:
- [ ] A patch plays group A round robin, each loop twice; after two rounds it plays group B the same way, then returns to A, with no manual action (the [worked example](../design/arrangement.md#worked-example)).
- [ ] A polymetric step sequencer with lanes of different lengths drives notes and modulation in the same patch.
- [ ] A 30-minute self-arranging set runs live with zero dropouts.

## M4. Sharing and ecosystem

**Goal:** people share living systems and extend LoomDSP, as with VCV Rack.

Scope:
- Self-contained patch files with embedded assets (FR-SHARE-01, FR-FILE-06).
- Missing-module handling for shared patches (FR-SHARE-02).
- Patch library browser backed by a public git repository, filtered by author, tag, and licence (FR-SHARE-04, FR-SHARE-07).
- Decide how third-party modules are distributed: compile-in or sandboxed WebAssembly (OQ-18, FR-SHARE-05).
- Module SDK release, templates, contributor guide (FR-SDK-02 to 05).
- Publish stable crates to crates.io (FR-SDK-06).
- Patch format documentation (FR-FILE-03).
- Spectral modules, preset morphing, OSC (FR-LIB-10, FR-PERF-03, FR-MIDI-05).
- Library breadth: the rest of the [module catalogue](module-catalogue.md), including the remaining modulators, note effects, sequencers, and device-style processors (FR-LIB-12 to 15).

---

## Later (not scheduled)

These are part of the long-term vision but not on the roadmap. Revisit after M3, when living systems are proven.

### Plugin and standalone export

**Goal:** patches leave LoomDSP as plugins and standalone apps. Not scheduled. See ADR-0004 for the planned approach.

Scope:
- Prebuilt runtime that loads embedded patch data (ADR-0004).
- CLAP export on top of `clack` (FR-NATIVE-01).
- Our own VST3 adapter on top of the MIT-licensed SDK bindings, as a separate crate (FR-NATIVE-02, FR-SDK-06).
- Plugin GUI embedding for Iced in a host window (OQ-13).
- Standalone app export (FR-NATIVE-08).
- LoomDSP host plugin (FR-NATIVE-06).
- Latency reporting, licence notices (FR-GRAPH-08, FR-NATIVE-09).
- macOS code signing and notarisation guidance.

Exit criteria:
- [ ] Exported CLAP passes `clap-validator`. Loads, automates, and restores state in Bitwig and REAPER.
- [ ] Exported VST3 passes the Steinberg validator. Loads in at least two DAWs.
- [ ] Sound parity test between the app and both plugin formats passes.

### Web target

**Goal:** run in the browser, only if it can match the desktop experience. The main driver is education: students open a link and start patching with no install (Sanna, the educator).

Go/no-go check before starting:
- Does the chosen UI stack (Iced on wgpu/WebGPU) run the canvas at 60 fps in current browsers?
- Is the WASM engine fast enough for the reference patch inside an AudioWorklet?
- Does someone in the community want to own this work?

Scope if yes: browser app and open-from-URL (FR-APP-02, 05), WAM2 export (FR-WAM-*), web runtime rules (NFR-WEB-*), ADR-0003.

---

## Project operations

How we keep the work visible and open to contributors:

- **GitHub Project board** with one milestone per phase (M0 to M4), plus a "Later" milestone for deferred work.
- **Issues reference requirement IDs** (for example "Implements FR-MIDI-02"). This links code to the reason it exists.
- **Labels:**
  - Type: `bug`, `feature`, `docs`, `research`, `adr`.
  - Area: `area:engine`, `area:ui`, `area:modules`, `area:midi`, `area:export`, `area:crates`.
  - Contributor: `good first issue`, `help wanted`, `hacktoberfest`.
  - Priority: `must`, `should`, `could`, matching the requirement priority.
- **Good first issues** come mostly from the module library (FR-LIB). Each module is small, self-contained, and testable with the SDK harness.
- **Hacktoberfest:** opt the repository in with the `hacktoberfest` topic and label suitable issues each October.
