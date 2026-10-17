# Open Questions and Risks

Each open item lists the options and a current leaning. Close an item by writing an ADR or updating the requirement it affects. Resolved items stay in the list so the reasoning is not lost.

## Resolved

| ID | Question | Decision | Where recorded |
|---|---|---|---|
| OQ-01 | Desktop first or browser first? | Desktop first. The browser is an optional target, deferred. | Vision, roadmap |
| OQ-02 | UI framework and node editor | Iced for the application UI. Our own patch canvas as an Iced widget, after spikes with iced_nodegraph (GPU fill-rate bound on Retina) and egui-snarl (poor look). | ADR-0009, ADR-0010 |
| OQ-03 | Stereo on every cable? | No. Channel count is inferred per cable. Stereo flows where audio needs it. | FR-SIG-07, ADR-0007 |
| OQ-04 | Pitch and value conventions | Our own documented conventions, following the Eurorack and Bitwig Grid idea of "patch anything to anything". | FR-SIG-03, 05, ADR-0007 |
| OQ-05 | Patch file format | RON, because it keeps Serde's full data model (enums, structs, tuples). | FR-FILE-01, ADR-0008 |
| OQ-06 | VST3 licensing | Steinberg released VST 3 SDK 3.8.0 under the MIT licence (October 2025). The `vst3` crate is MIT/Apache-2.0 and now ships generated bindings. The older `vst3-sys` crate is GPLv3 and is excluded. | FR-NATIVE-02, ADR-0006 |
| OQ-07 | Plugin wrapper framework | No nih-plug (or its fork nice-plug), because of their contribution policies. Use `clack` (MIT/Apache-2.0) for CLAP. Build our own VST3 adapter on the `vst3` crate and publish it as a crate. | FR-NATIVE-01, 02, FR-SDK-06 |
| OQ-08 | Exported plugin licence | The runtime is Apache-2.0, with its licence and NOTICE included in every export. The patch author chooses the licence for their patch. | FR-NATIVE-09 |
| OQ-09 | Block vs. per-sample processing | Hybrid: blocks for acyclic parts, per sample inside feedback loops. Validate with benchmarks in P0. | ADR-0002, NFR-PERF-08 |
| OQ-10 | Project name | "LoomDSP" appears to be free (informal search). Do a formal trademark check before a 1.0 release. | |
| OQ-12 | Maturity of `iced_nodegraph` | Not used for the canvas. Findings reported upstream as a courtesy. | ADR-0010 |
| OQ-19 | What is a trigger? | An output is high for exactly one sample. Inputs count rising edges above 0.5, with no hysteresis, and the first frame counts as following a low one. A fixed 1 ms pulse was rejected, because close triggers would merge. | FR-SIG-06, ADR-0007 |

## Open

### OQ-11. Desktop audio and MIDI I/O

- `cpal` is the standard Rust crate for audio I/O. It is Apache-2.0 licensed, so it passes our licence rules. But it lives in the RustAudio organisation, whose community AI policy restricts AI-generated contributions. We could use it, but probably could not send it AI-assisted fixes.
- **Options:**
  1. Use `cpal` as a dependency behind our own host-adapter trait. Accept that we may not contribute upstream.
  2. Write our own audio backends (CoreAudio, WASAPI, ALSA/PipeWire/JACK).
  3. Use `cpal` now, and replace it per platform later if we hit limits.
- **Leaning:** option 3. Writing audio backends for three platforms is a large project, and not where LoomDSP adds value early on. The adapter trait (NFR-PORT-02) keeps it replaceable.
- Same review needed for MIDI I/O (`midir`, which is outside the RustAudio organisation; check its licence and contribution policy).

### OQ-13. Embedding Iced in plugin windows

- Plugins draw inside a window owned by the DAW. Iced normally creates its own window through `winit`.
- `nice-plug-iced` shows this is possible, but it sits inside RustAudio and its AI policy.
- **Action:** research when plugin export is scheduled (deferred). A small "embed Iced in a parent window" crate could be another published primitive (FR-SDK-06).

### OQ-14. Iced version churn

- Iced is pre-1.0 and has breaking changes between releases. `iced_nodegraph` must keep up.
- **Leaning:** pin versions. Upgrade once per Iced release, in a dedicated issue.

### OQ-15. Looper recording

- Loopers record audio and MIDI (FR-LIB-20). Recording on the audio thread must not allocate or touch the disk.
- **Leaning:** record into memory allocated when the looper is created or resized (on the control thread), with a maximum loop length per looper. Saving loops to disk, and loading them, happens on a control thread. Long-form recording to disk is out of scope for now.
- Decide the maximum lengths and memory budget in P3.

### OQ-16. How notes travel on cables

Note effects (arpeggiators, chords, quantise) need notes to move between modules (ADR-0011). Triggers such as "loop finished" are already gate signals and need no new cable kind.

| Option | For | Against |
|---|---|---|
| Per-voice signals (pitch, gate, velocity as polyphonic audio-rate signals), as in Bitwig's Grid | Fits the "patch anything" signal model (ADR-0007). Anything can modulate notes. | Chords and arpeggios need voice allocation in every note module. Note timing is tied to voice slots. |
| Event cables: a new cable kind carrying sample-timestamped note events | Exact timing, any number of notes, natural for MIDI effects and the timeline. | A second kind of cable. Breaks "every cable is f32" and needs its own scheduling. |
| Both, with converter modules (events to voices and back) | Each module uses the natural form. | More concepts for users to learn. |

- **Leaning:** both, with event cables shown as their own colour and automatic conversion where an event output meets a signal input. Needs a design spike in P2, before note effects are built in P3.

### OQ-17. Module data beyond parameters

- Patterns, clips, wavetables, and samples are module state, not parameters (FR-LIB-19).
- **Questions:** the patch format (RON for small data, separate asset files for audio?), undo for large edits, and handing new data to the audio thread without allocation there (same pattern as schedule swaps, ADR-0001).
- **Leaning:** a per-module `data` field in the patch for small structured data, asset references for audio. Decide in P2.

### OQ-18. How third-party modules are distributed

VCV Rack's ecosystem works because people can install modules without building the program (FR-SHARE-05). Our current rule is "compile modules in, no runtime loading of native code" (FR-SDK-04), for safety and portability.

| Option | For | Against |
|---|---|---|
| Compile in from source (curated list) | Safe, portable, simple. | Users cannot add modules without a new LoomDSP build. Slow for the ecosystem. |
| Native dynamic libraries (like VCV Rack) | Full speed. Proven model. | Rust has no stable ABI, so modules must match the exact compiler and LoomDSP version. Untrusted native code runs with full access. Builds needed for every platform. |
| WebAssembly modules, run in a sandbox | Safe (sandboxed), one build for all platforms, any language that compiles to WASM. | Slower than native. Real-time safety of the WASM runtime (no allocation or JIT pauses on the audio thread) must be proven. |

- **Leaning:** compile in for now. Spike WebAssembly modules in P4: measure the cost of a filter and an oscillator as WASM against native, and check the runtime's real-time behaviour. Decide before announcing a module SDK.

### OQ-20. How roles are named and versioned

Roles let a slot accept any part with the right ports and parameters (ADR-0017, FR-SDK-08). A role is a contract that patches depend on, so it needs a stable name and a way to change.

- **Questions:** the naming scheme; where a role's contract lives (in the SDK, as data like a module descriptor); what happens when a role gains a parameter; whether a part can fill several roles.
- **Leaning:** namespaced IDs like module IDs (for example `loom.role.filter`), with a version. A new version may only add optional parameters, so every part that filled the old version still fits. Decide in the roles and slots specification before P2 work starts.

### OQ-21. What carries across a slot swap

Parameters carry across by role name (FR-ENCAP-08). Stored data is harder: a sequencer pattern, a wavetable, or a recorded buffer belongs to one part.

- **Questions:** whether module data carries across when two parts use the same kind of data; how much stored data a slot keeps for parts that are not active; how undo treats a swap.
- **Leaning:** keep each part's data in the patch, keyed by the part, and restore it when the user swaps back. Never convert data between parts. A swap is one undo step.

### OQ-22. Is the scale a module or a patch setting?

Pitch modules follow a patch-wide scale and tuning (FR-LIB-29). It could be a setting on the patch, like the transport (ADR-0015), or a module.

- **Leaning:** a patch setting, read by modules without cables like the transport, plus a Scale and tuning module (MT-PITCH-03) that changes it from a trigger, so logic can change key between sections.

## Risks

| ID | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-01 | UI work is much larger than DSP work, and the project stalls on the canvas and panel editor. | High | High | Own canvas with a small first version and performance gates (ADR-0010). Ship the headless engine and CLI early so progress stays visible. |
| R-02 | Per-sample feedback regions are too slow for large patches. | Medium | High | Benchmark in P0 (NFR-PERF-08). Limit per-sample mode to cycle regions only. |
| R-04 | A dependency's licence blocks permissive plugin export. | Medium | High | `cargo-deny` in CI from P0 with an allow list (ADR-0006). |
| R-05 | Scope creep: features that do not fit the grid (separate DAW views, mixers, track lists). | Medium | Medium | Everything new must be a module (ADR-0011). Non-goals in the vision document. |
| R-06 | Single maintainer, limited time. | High | High | Small phases with usable results. Public backlog with contributor labels. AI-assisted development. |
| R-07 | macOS signing and notarisation makes export hard for non-developers. | High | Medium | Document the process. Consider unsigned builds with clear instructions. |
| R-08 | Building our own primitives (VST3 adapter, GUI embedding) adds maintenance load. | High | Medium | Only build what has no acceptable alternative. Keep each crate small, documented, and tested so others can help maintain it. |
| R-09 | Building our own canvas (ADR-0010) takes much longer than estimated. | Medium | High | Iced keeps text, windowing, and ordinary widgets. Build a measured prototype first, with performance gates on the reference laptop. Keep the scope of the first version small. |
| R-10 | Strict dependency rules leave a gap with no acceptable crate. | Medium | Medium | Licence rules are strict (Must). AI-policy preference is soft (Should). We can use a permissive crate even if we cannot contribute to it. |
| R-11 | "Everything is a module" makes the library very large, and the project spreads thin. | High | High | Grow by phase, following the [module catalogue](module-catalogue.md). Prioritise modules for live performance. Build primitives, shared traits, and open abstractions instead of one fixed module per device (ADR-0017). Make the module SDK good enough that the community can add modules. |
| R-12 | Note handling (OQ-16) is decided late and forces rework in modules already built. | Medium | High | Design spike before the first note effect. Keep MIDI note input behind one module until then. |
| R-13 | No way to install third-party modules without rebuilding slows the community (OQ-18). | Medium | Medium | Ship a strong built-in library first. Decide OQ-18 early in P4. |

## Notes on the background research

The research document was a good starting point. Points that changed during review:

1. **"Acyclic graph" vs. feedback.** The research describes the graph as a DAG and also promises single-sample feedback. Feedback needs cycles. FR-GRAPH-03 and ADR-0002 resolve this.
2. **Web Audio block sizes.** The 128-sample AudioWorklet limit only matters for the optional web target. The desktop engine works with any host buffer size.
3. **Iced vs. Vizia.** We agree with the research's lean toward Iced, for a different reason: `iced_nodegraph` now gives a GPU-rendered node editor for Iced. `iced_audio` (cited in the research) is not needed. We will build our own knobs and sliders.
4. **Performance figures** in the research come from varied sources. We will replace them with our own benchmarks in P0.
