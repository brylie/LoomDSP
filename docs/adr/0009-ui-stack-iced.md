# ADR-0009: UI stack: Iced (canvas decided in ADR-0010)

- **Status:** Accepted
- **Date:** 2026-10-13
- **Related requirements:** FR-CANVAS-*, FR-PANEL-*, FR-VIS-*, NFR-PERF-04, OQ-02, OQ-12, OQ-13, OQ-14

## Context

The UI is a node-based editor for a DSP graph, plus a panel editor and a performance view. It needs:

- A zoomable canvas with hundreds of nodes and coloured cables.
- Custom widgets: knobs, sliders, XY pads, scopes, meters.
- 60 fps with live telemetry.
- High-DPI vector rendering.
- Later, embedding inside plugin windows owned by a DAW.

It must be Rust (ADR-0005) and permissively licensed (ADR-0006).

## Options considered

| Option | Node editor available | Notes |
|---|---|---|
| Iced + `iced_nodegraph` | Yes: GPU-rendered edges and pins (signed distance fields in a single wgpu pipeline), typed pins, nested graphs, minimap, 500-node stress test | MIT. Elm-style architecture. Pre-1.0 with breaking changes. Node editor is young. |
| egui + `egui-snarl` | Yes: generic node-graph container, serde support | MIT/Apache-2.0. Immediate mode is quick to build with but harder to make look polished for shipped panels. |
| egui_node_graph | Yes | Unmaintained. |
| Vizia | No dedicated node editor | Built for audio plugins. Smaller ecosystem. |
| Custom wgpu/Vello renderer | Build it ourselves | Full control. Most work. We build text input, focus, and accessibility ourselves. |

## Decision (proposed)

Use Iced for all UI, with `iced_nodegraph` for the patching canvas.

- Wrap `iced_nodegraph` behind our own canvas module so we can extend, fork, or replace it.
- Build our own audio widgets (knobs, sliders, XY pad) as an Iced widget crate. A candidate for publishing (FR-SDK-06).
- Draw scopes and spectra with custom wgpu shader widgets, fed directly from the telemetry queues (ADR-0001).

## P0 spike: what to measure

1. Frame time with 100 and 300 nodes, 500 cables, and 4 live scopes.
2. Custom port shapes and cable colours per signal type.
3. Nested graph navigation (step into a sub-patch).
4. Effort to build one knob widget with modulation range display (FR-MOD-04).
5. Rough idea of how to embed Iced in a parent window (for OQ-13).

If the spike fails on 1 or 2, try `egui-snarl` next, then a custom renderer.

## Spike results (2026-10-13)

See [p0-ui-spikes.md](../benchmarks/p0-ui-spikes.md).

1. **Frame time:** canvas CPU work is about 5.5 ms per frame for 298 modules and 371 cables on a 2-vCPU VM. GPU frame rate is not measured yet (no GPU on the build machine). To do on the reference machine.
2. **Port shapes and cable colours:** yes. Per-signal colours, gradient cables, square pins for pitch and gate.
3. **Nested graph navigation:** not tried yet. `iced_nodegraph` supports graphs inside node bodies.
4. **Knob effort:** not built. Sliders inside nodes work. A knob with modulation range display is a custom widget, planned for P1.
5. **Embedding:** not tried (OQ-13).

Extra finding: the widget is stateless and renders straight from `Controller::graph()`. Every gesture becomes a controller edit and a schedule swap while audio plays. This fits ADR-0005 (one data model) very well.

Decision: proceed with Iced and `iced_nodegraph` for P1, unless the real-hardware frame-rate test fails.

## Consequences

- One GPU-backed UI stack for the editor, panels, and plugin GUIs.
- Iced also targets WebGPU in browsers, which keeps the optional, deferred web target possible without a second UI.
- We depend on a young crate and a pre-1.0 framework. Version pinning and a wrapper layer reduce the risk (OQ-12, OQ-14).

## Final decision (2026-10-13)

Two spikes were measured on the reference laptop (Apple M1-family GPU, built-in Retina display):

| | iced_nodegraph | egui-snarl |
|---|---|---|
| 300 modules, full window, Retina | 22 to 24 fps | Not measured |
| 8 modules, full window, Retina | 40 fps | Not measured |
| 300 modules, same window, 1x external display | about 70 fps | Not measured |
| Look | Good | Poor (egui's stock look) |

Findings:

1. **Iced itself performed well.** Building the view took under 1 ms. Iced's layout and text work were about 13% of the main thread. A CPU cost in the spike's own pin styling (theme colours recomputed per pin per frame) was found with samply and fixed with a small cache.
2. **iced_nodegraph is limited by GPU fill rate.** Its signed-distance-field renderer costs GPU time per physical pixel. On Retina (4x the pixels of a 1x display at the same window size) it reaches 24 fps with 300 modules and 40 fps with 8. A quarter-size window, or the same window on a 1x display, reaches 60 to 70 fps.
3. **egui-snarl's look is far from the polish we want** (the Bitwig Grid is the reference), and its own-copy graph model fits our `Controller` poorly.

Decision:

- **Accepted:** Iced for the application UI (windows, inspector, browser, panels, theming, text).
- **Rejected for the patch canvas:** iced_nodegraph and egui-snarl.
- **The canvas** will be our own Iced widget (ADR-0010).

Full measurements and profiles: [p0-ui-spikes.md](../benchmarks/p0-ui-spikes.md).
