# ADR-0010: Our own patch canvas, built as an Iced widget

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-13
- **Related requirements:** FR-CANVAS-*, FR-VIS-*, FR-LIB-18, NFR-PERF-04, NFR-UX-03
- **Related decisions:** ADR-0005 (Rust end to end), ADR-0009 (Iced for the UI), ADR-0011 (everything is a module), ADR-0013 (modules carry their own panel)

## Context

The patch canvas is the centre of LoomDSP. Users spend most of their time there, on stage as well as in the studio. Two P0 spikes tested ready-made node editors with the same patch, generator, and measurements:

| | iced_nodegraph (Iced) | egui-snarl (egui) |
|---|---|---|
| Fit with our data model | Very good: stateless widget, renders straight from `Controller::graph()` | Fair: keeps its own copy of the graph, so every edit is mirrored twice |
| Look | Good, but styling only through its hooks | Poor: egui's stock widget look and spacing |
| Speed on the reference laptop (M1-family, built-in Retina) | 22 to 24 fps with 300 modules, 40 fps with 8, at full window size | Not measured on real hardware |
| Why | Signed-distance-field shading costs GPU time per physical pixel: 24 fps on Retina, about 70 fps on a 1x external display, same window | Lays out every node every frame (immediate mode); about 11 ms of CPU for 300 modules on a slow VM |

Details: [p0-ui-spikes.md](../benchmarks/p0-ui-spikes.md).

At the same time, the product scope grew (ADR-0011): everything is a module, including modulators, note effects, sequencers, and the timeline. Many of these need rich views inside the canvas: oscilloscopes, spectra, step and Euclidean sequencer grids, envelope shapes, clip timelines. The look and feel should match the polish of Bitwig Studio's Grid.

A generic node editor gives us none of this control. Its renderer sets our performance limits, and its styling hooks set our look.

## Options considered

1. **Keep iced_nodegraph, fix upstream.** Report the per-pixel cost and the per-pin style cost, contribute fixes. Lowest effort if upstream moves fast. We stay bound to its renderer, interaction model, and release pace.
2. **egui-snarl.** Rejected on look, and its own-copy graph model fits poorly.
3. **Our own canvas as an Iced widget.** Iced keeps windowing, text, theming, and ordinary widgets (inspector, browser, panels). We write the canvas: layout, hit-testing, interaction, and rendering.
4. **Our own canvas on raw wgpu, outside Iced.** Full control, but we would rebuild text, input, and accessibility, and run two UI systems side by side.

## Decision

Option 3: write the patch canvas ourselves, as an Iced widget in a new crate (`loom-canvas`).

## Design brief

The Bitwig Grid is the reference for feel and polish. Goals:

- **Compact modules on a snap grid.** Signal flows left to right. Modules are small tiles, not tall forms.
- **Ports on module edges,** coloured by signal type, with shape as a second cue (NFR-UX-03).
- **Controls and displays on the tile.** Each module carries its own panel, edited in place, as in Bitwig's Grid: knobs, toggles, radio buttons, lights, scopes, step grids (ADR-0013, [panels specification](../design/panels.md)). An **inspector panel** stays for exact values and settings that do not fit on the tile. *(Changed 2026-10-15: the first version kept tiles to one or two controls and put everything else in the inspector.)*
- **Cables that carry meaning:** colour by type, thicker for stereo and polyphonic signals, and a visible mark where the engine inserted a 1-sample delay. Cables do not animate with the signal; activity shows on ports.
- **Fast insertion:** type to add a module at the cursor, drop a module onto a cable to insert it (FR-CANVAS-03, FR-CANVAS-08).
- **Modulation on the parameter:** modulation depth shown as a ring or arc on the control itself (FR-MOD-04).
- **Level of detail:** when zoomed out, modules draw as simple tiles without text or live views.

## Rendering approach

- **One custom wgpu primitive** through Iced's shader widget for tiles, ports, and cables. Instanced quads and triangle meshes, built once and updated only when the graph or camera changes. Cost should follow the number of shapes, not the number of pixels.
- **Text through Iced's text system,** cached per module; skipped when zoomed out.
- **Live views as GPU textures or vertex buffers,** fed from the engine's telemetry queues (ADR-0001), uploaded once per frame.
- **Redraw only on visible change:** graph edits, camera moves, hover, and displays whose data changed while they are on screen. A patch with nothing changing on screen does not redraw.

## Performance gates

Measured on the reference laptop (NFR-PERF-04), at full window size on the built-in Retina display:

- A prototype in P1 with 100 modules visible and 300 in the patch holds 60 fps while panning and zooming.
- With 4 live scopes on screen, it still holds 60 fps.
- A patch with nothing changing on screen uses close to zero GPU and CPU.

If the first prototype misses these by a wide margin, revisit this ADR before building on it.

**Prototype status:** the first prototype is in `crates/loom-canvas` and `crates/loom-app`. How to measure it, and the results so far, are in [p1-canvas.md](../benchmarks/p1-canvas.md). Accept this ADR once the reference laptop passes the first and third gates.

## Consequences

- We own a substantial component: hit-testing, selection, pan and zoom, keyboard navigation, undo integration, and rendering. Estimate two to four weeks for a solid core.
- We control performance, look, and interaction, and can add module panels and displays freely. That is central to "everything is a module".
- The same primitives can draw front panels and plugin GUIs later, giving one visual language.
- `loom-canvas` could be published as a crate once stable (FR-SDK-06).
- The spike code was removed after the ADR-0009 decision. Its write-ups and screenshots are kept in [p0-ui-spikes.md](../benchmarks/p0-ui-spikes.md), and the code is in git history.
- Upstream: report the iced_nodegraph findings (per-pin theme style cost, per-pixel cost on Retina) as a courtesy, with the profiles.
