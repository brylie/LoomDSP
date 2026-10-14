# P1 Canvas Prototype

The first version of our own patch canvas ([ADR-0010](../adr/0010-own-patch-canvas.md)): the `loom-canvas` crate (an Iced widget) and the `loom-app` desktop app. This page records how to measure it against the performance gates, and the results.

![FM feedback patch in the P1 canvas](images/p1-canvas-fm-feedback.png)

## What is in the prototype

- Compact module tiles on a 12-unit snap grid. Signal flows left to right. Inputs on the left edge, outputs on the right.
- Ports coloured by signal type, with shape as a second cue: pitch and gate are square, the others round. Connected ports are filled, free ports are rings.
- Cables as curves, coloured from source to destination type. Cables where the engine inserted a 1-sample delay are dashed. Feedback cables (running right to left) loop below the modules.
- Level of detail: titles only when at least 6.5 px tall, port labels only when at least 7.5 px tall.
- Inspector panel for the selected module, with logarithmic sliders for frequencies and times.
- Live audio and MIDI while editing. Every edit goes through the engine's lock-free schedule swap.
- A stats bar: frame rate, average and worst frame time, module and cable count, audio load and overruns.

Gestures: drag from a port to patch a cable; drag a connected input to move or remove its cable; drag tiles to move them; shift-click or drag a box to select; right-drag or two-finger scroll to pan; mouse wheel or Cmd/Ctrl-scroll to zoom; F or Home frames the patch; Delete removes the selection.

## How it draws

One custom wgpu primitive draws the whole canvas, in this order:

1. A dot grid: one quad with a few operations per pixel, faded out when zoomed far out.
2. Tiles: instanced rounded rectangles. Each one shades only its own pixels.
3. Cables: one instanced triangle strip per cable. The vertex shader builds the curve from the two end points, so a cable is 64 bytes of GPU data.
4. Ports, then the overlay (selection, hover, the cable being dragged, the selection box).
5. Text, through Iced, on top.

Tile, port, and cable data is built on the CPU and uploaded only when the scene changes. Panning and zooming change one 32-byte uniform buffer. Without the **Measure** toggle, the window redraws only on input and a twice-a-second stats refresh.

## How to measure (reference laptop)

```sh
cargo run --release -p loom-app -- --generate 300 --bench
```

`--bench` turns on **Measure** (redraw every frame and count) and **Move camera** (a slow loop of pan and zoom). Run it at full window size on the built-in Retina display. To test the "100 visible" case, switch off **Move camera**, zoom in until about 100 tiles are on screen (titles visible), then switch it back on. The camera loops around the current view.

Record from the stats bar after about 10 seconds:

| Run | fps | frame avg | worst | Notes |
|---|---|---|---|---|
| `--generate 300 --bench`, whole patch in view (titles hidden) | | | | |
| Same, zoomed to about 100 tiles visible (titles shown) | | | | |
| Same, zoomed in to about 20 tiles (port labels shown) | | | | |
| `examples/patches/fm-feedback.ron`, **Measure** on, camera still | | | | |
| Idle: **Measure** off, nothing moving. Activity Monitor CPU % and GPU % | | | | |

Pass criteria (NFR-PERF-04): 60 fps with 300 modules in the patch and 100 visible, while panning and zooming. An idle patch uses close to no CPU or GPU. The 4 live scopes come with module views (next P1 step) and are measured then.

## First results

### Cloud VM, software rendering (2026-10-13)

A 2-vCPU cloud VM with no GPU, rendering through Mesa's software Vulkan driver (lavapipe) at 1440 x 900. This says nothing about real GPU performance, but it is the same machine and driver the P0 spikes were measured on, so the two can be compared.

| Run | P0 spike (iced_nodegraph) | P1 canvas |
|---|---|---|
| 300 modules, whole patch in view, camera moving | 2 to 7 fps | 42 fps |
| 300 modules, zoomed in with titles, camera moving | | 49 fps (20 ms average) |
| 300 modules, zoomed in with titles, camera still | | 53 fps |

On software rendering every pixel costs CPU time, so this is a rough sign that the per-pixel cost has dropped by about an order of magnitude.

![300 generated modules, zoomed in, camera moving](images/p1-canvas-generated-300.png)

### Reference laptop (M1 family, built-in Retina)

**First result (2026-10-14):** a steady 120 fps with `--generate 300 --bench` at full window size, with the camera moving. 120 fps is the display's refresh rate (ProMotion), so each frame finishes early and waits for the screen. The P0 spike reached 22 to 24 fps in the same setup.

Still to record: the zoomed-in runs and idle CPU and GPU (table above).
