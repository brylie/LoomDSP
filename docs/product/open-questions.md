# Open Questions and Risks

Each open item lists the options and a current leaning. Close an item by writing an ADR or updating the requirement it affects. Resolved items stay in the list so the reasoning is not lost.

## Resolved

| ID | Question | Decision | Where recorded |
|---|---|---|---|
| OQ-01 | Desktop first or browser first? | Desktop first. The browser is an optional target, deferred. | Vision, roadmap |
| OQ-02 | UI framework and node editor | Iced for the application UI. Our own patch canvas as an Iced widget, after spikes with iced_nodegraph (GPU fill-rate bound on Retina) and egui-snarl (poor look). Superseded on 2026-11-09: the app moves in phases to our own UI layer on `winit`, `wgpu`, and AccessKit; the canvas stays ours. | ADR-0009, ADR-0010, ADR-0040 |
| OQ-03 | Stereo on every cable? | No. Channel count is inferred per cable. Stereo flows where audio needs it. | FR-SIG-07, ADR-0007, ADR-0020 |
| OQ-04 | Pitch and value conventions | Our own documented conventions, following the Eurorack and Bitwig Grid idea of "patch anything to anything". | FR-SIG-03, 05, ADR-0007 |
| OQ-05 | Patch file format | RON, because it keeps Serde's full data model (enums, structs, tuples). | FR-FILE-01, ADR-0008 |
| OQ-06 | VST3 licensing | Steinberg released VST 3 SDK 3.8.0 under the MIT licence (October 2025). The `vst3` crate is MIT/Apache-2.0 and now ships generated bindings. The older `vst3-sys` crate is GPLv3 and is excluded. | FR-NATIVE-02, ADR-0006 |
| OQ-07 | Plugin wrapper framework | No nih-plug (or its fork nice-plug), because of their contribution policies. Use `clack` (MIT/Apache-2.0) for CLAP. Build our own VST3 adapter on the `vst3` crate and publish it as a crate. | FR-NATIVE-01, 02, FR-SDK-06 |
| OQ-08 | Exported plugin licence | The runtime is Apache-2.0, with its licence and NOTICE included in every export. The patch author chooses the licence for their patch. | FR-NATIVE-09 |
| OQ-09 | Block vs. per-sample processing | Hybrid: blocks for acyclic parts, per sample inside feedback loops. Validate with benchmarks in M0. | ADR-0002, NFR-PERF-08 |
| OQ-10 | Project name | "LoomDSP" appears to be free (informal search). Do a formal trademark check before a 1.0 release. | |
| OQ-12 | Maturity of `iced_nodegraph` | Not used for the canvas. Findings reported upstream as a courtesy. | ADR-0010 |
| OQ-14 | Iced version churn | Settled (2026-11-09): churn is Iced's stated policy, so the app leaves Iced for our own UI layer, in phases. Until the last phase, Iced stays pinned and is upgraded once per release, in a dedicated issue. | ADR-0040, [plan](../dev/leaving-iced-plan.md) |
| OQ-19 | What is a trigger? | An output is high for exactly one sample. Inputs count rising edges above 0.5, with no hysteresis, and the first frame counts as following a low one. A fixed 1 ms pulse was rejected, because close triggers would merge. | FR-SIG-06, ADR-0007 |
| OQ-17 | Module data beyond parameters | Typed data slots (curve, pattern, buffer). Curves and patterns inline in the patch; audio as WAV files in a sidecar folder. Swapped in from the control thread. Undo covers player edits, not live recording. | FR-LIB-19, ADR-0032, [module data](../design/module-data.md) |
| OQ-20 | How roles are named and versioned | Settled (2026-11-08): namespaced IDs with a version (`loom.role.filter@1`), each port and parameter required or optional. A new version only adds optional parameters, and a part fits a slot by role id. A part may fill several roles, and may map role names to its own. | FR-SDK-08, ADR-0017, [sub-patches](../design/sub-patches.md#roles) |
| OQ-21 | What carries across a slot swap | Settled (2026-11-08): parameters by role name, clamped to the new part's range. Each part keeps its own values and data in the patch, keyed by part, restored on swap back, never converted. A swap is one undo step and fades over 10 ms, undo included. | FR-ENCAP-08, [sub-patches](../design/sub-patches.md#slots) |
| OQ-22 | Is the scale a module or a patch setting? | Like the transport: the engine holds one tuning and one active scale per patch, read without cables. A Scale and tuning module stores up to 8 scales and switches them from a trigger. | FR-LIB-29, ADR-0033, [scale and tuning](../design/scale-and-tuning.md) |
| OQ-16 | How notes travel on cables | Per-voice signals only: pitch, gate, velocity, and expression in a polyphonic bundle, which is an inferred cable width as stereo is, with voices run as module copies. No event cable kind. Every module that makes or changes notes allocates voices with one shared algorithm. Decided without the planned prototype, because the engine already runs copies and lands events on their sample. | FR-SIG-08, FR-LIB-13, ADR-0038, [polyphony](../design/polyphony.md) |

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
- **Action:** research when plugin export is scheduled (deferred). Since ADR-0040, embedding depends on our own `winit` shell rather than on Iced: the shell takes a parent window it is given as well as one it creates ([UI layer spec](../design/ui-layer.md#out-of-scope)). That shell could be another published primitive (FR-SDK-06).
- Trade-offs of leaving Iced for our own UI stack, and when to revisit: [research snapshot, 2026-11](../research/2026-11-own-ui-stack-tradeoffs.md).

### OQ-15. Looper recording

- Loopers record audio and MIDI (FR-LIB-20). Recording on the audio thread must not allocate or touch the disk.
- **Leaning:** record into memory allocated when the looper is created or resized (on the control thread), with a maximum loop length per looper. Saving loops to disk, and loading them, happens on a control thread. Long-form recording to disk is out of scope for now.
- Decide the maximum lengths and memory budget in M3.

### OQ-18. How third-party modules are distributed

VCV Rack's ecosystem works because people can install modules without building the program (FR-SHARE-05). Our current rule is "compile modules in, no runtime loading of native code" (FR-SDK-04), for safety and portability.

| Option | For | Against |
|---|---|---|
| Compile in from source (curated list) | Safe, portable, simple. | Users cannot add modules without a new LoomDSP build. Slow for the ecosystem. |
| Native dynamic libraries (like VCV Rack) | Full speed. Proven model. | Rust has no stable ABI, so modules must match the exact compiler and LoomDSP version. Untrusted native code runs with full access. Builds needed for every platform. |
| WebAssembly modules, run in a sandbox | Safe (sandboxed), one build for all platforms, any language that compiles to WASM. | Slower than native. Real-time safety of the WASM runtime (no allocation or JIT pauses on the audio thread) must be proven. |

- **Leaning:** compile in for now. Spike WebAssembly modules in M4: measure the cost of a filter and an oscillator as WASM against native, and check the runtime's real-time behaviour. Decide before announcing a module SDK.

## Risks

| ID | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-01 | UI work is much larger than DSP work, and the project stalls on the canvas and panel editor. | High | High | Own canvas with a small first version and performance gates (ADR-0010). Ship the headless engine and CLI early so progress stays visible. |
| R-02 | Per-sample feedback regions are too slow for large patches. | Medium | High | Benchmark in M0 (NFR-PERF-08). Limit per-sample mode to cycle regions only. |
| R-04 | A dependency's licence blocks permissive plugin export. | Medium | High | `cargo-deny` in CI from M0 with an allow list (ADR-0006). |
| R-05 | Scope creep: features that do not fit the grid (separate DAW views, mixers, track lists). | Medium | Medium | Everything new must be a module (ADR-0011). Non-goals in the vision document. |
| R-06 | Single maintainer, limited time. | High | High | Small phases with usable results. Public backlog with contributor labels. AI-assisted development. |
| R-07 | macOS signing and notarisation makes export hard for non-developers. | High | Medium | Document the process. Consider unsigned builds with clear instructions. |
| R-08 | Building our own primitives (VST3 adapter, GUI embedding) adds maintenance load. | High | Medium | Only build what has no acceptable alternative. Keep each crate small, documented, and tested so others can help maintain it. |
| R-09 | Building our own canvas (ADR-0010) takes much longer than estimated. | Medium | High | Iced keeps text, windowing, and ordinary widgets. Build a measured prototype first, with performance gates on the reference laptop. Keep the scope of the first version small. |
| R-10 | Strict dependency rules leave a gap with no acceptable crate. | Medium | Medium | Licence rules are strict (Must). AI-policy preference is soft (Should). We can use a permissive crate even if we cannot contribute to it. |
| R-11 | "Everything is a module" makes the library very large, and the project spreads thin. | High | High | Grow by phase, following the [module catalogue](module-catalogue.md). Prioritise modules for live performance. Build primitives, shared traits, and open abstractions instead of one fixed module per device (ADR-0017). Make the module SDK good enough that the community can add modules. |
| R-12 | Note handling (OQ-16) is decided late and forces rework in modules already built. | Low | High | Decided in M2 (ADR-0038): per-voice signals, which the engine already runs as copies. No prototype was built, so the first M3 note effect is the test of the model. If it needs events, a superseding ADR adds an event cable kind beside per-voice signals, and nothing built on bundles changes. |
| R-13 | No way to install third-party modules without rebuilding slows the community (OQ-18). | Medium | Medium | Ship a strong built-in library first. Decide OQ-18 early in M4. |

## Notes on the background research

The research document was a good starting point. Points that changed during review:

1. **"Acyclic graph" vs. feedback.** The research describes the graph as a DAG and also promises single-sample feedback. Feedback needs cycles. FR-GRAPH-03 and ADR-0002 resolve this.
2. **Web Audio block sizes.** The 128-sample AudioWorklet limit only matters for the optional web target. The desktop engine works with any host buffer size.
3. **Iced vs. Vizia.** We agree with the research's lean toward Iced, for a different reason: `iced_nodegraph` now gives a GPU-rendered node editor for Iced. `iced_audio` (cited in the research) is not needed. We will build our own knobs and sliders.
4. **Performance figures** in the research come from varied sources. We will replace them with our own benchmarks in M0.
