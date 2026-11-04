# Reading an XY Scope

What drawing one signal against another shows, why a circle means a quarter cycle, and why the two signals must stay paired. For musicians and developers who have used a scope but not this view of one.

- **Kind:** explainer.
- **Used by:** [panel widgets specification](../design/panel-widgets.md#xy-scope), [telemetry specification](../design/telemetry.md#paired-probes).
- **Code:** `loom.display.xy` in `crates/loom-modules/src/display.rs`, `XyTrace` in `crates/loom-app/src/xy.rs`, `PairChunk` in `crates/loom-core/src/telemetry.rs`.

## No time axis

A [scope](../design/panels.md#scope-module) draws a signal against time. An XY scope has no time axis. One signal moves the point left and right, and the other moves it up and down. What you see is how the two relate.

| Frame | X | Y | Point |
|---|---|---|---|
| 0 | 0.0 | 1.0 | Top middle |
| 1 | 0.7 | 0.7 | Upper right |
| 2 | 1.0 | 0.0 | Right middle |
| 3 | 0.7 | -0.7 | Lower right |

Those are two sines, and they trace a circle.

## Why a circle means a quarter cycle

A point on a circle is `(sin t, cos t)`, and a cosine is a sine a quarter cycle ahead. So two equal sines a quarter cycle apart draw a circle. Move them closer in phase and the circle narrows to an ellipse. At no difference it closes to a diagonal line, because X and Y are always equal. At half a cycle it is the other diagonal, because Y is always minus X.

| Difference between the sines | Shape |
|---|---|
| None | Diagonal, rising to the right |
| A quarter cycle | Circle |
| Half a cycle | Diagonal, falling to the right |

Different frequencies make other figures. When one frequency is a simple ratio of the other, such as 2 to 3, the figure closes and holds still. Slightly off that ratio, it slowly turns.

## What stereo width looks like

Connect a stereo cable to X and leave Y empty. The left side then moves the point across, and the right side moves it up.

- **Mono:** left equals right, so the trace is a diagonal line.
- **Wide:** left and right differ, so the line opens into a cloud. The wider the cloud, the wider the sound.
- **Out of phase:** left is minus right, so the line lies on the other diagonal. This one is a warning: the two sides cancel when played as mono.

A diagonal is hard to read, so **Rotate** turns the view by 45 degrees. Across is then `(y - x) / √2`, the difference of the sides, and up is `(x + y) / √2`, their sum. These are the side and mid signals. A mono signal is now a vertical line, and width spreads sideways. A signal only on the left leans up and to the left, as you would expect.

Turning a full-scale mono sine makes it reach 1.41, because the sides add up. The trace stops at the top and bottom edges, and never wraps. A wrapped point would show a shape that is not in the signal.

## Why the samples must be paired

Each point needs an X and a Y from the same moment. Two ordinary scope probes cannot promise that. Each has its own stream of chunks, and when the app falls behind, each stream loses chunks on its own. A sample of X could then be drawn with a sample of Y from a later block, and the trace would show a shape that is not in the signal. The mistake looks like a real one, so nobody would suspect the display.

So a paired probe is one probe. The engine writes X and Y side by side into one chunk, from one block, in one push. A chunk is kept or dropped whole, so a pair can never be split. A gap in the frame numbers means chunks were lost, and the app clears its history instead of drawing a line across the gap.

## Why it keeps every Nth point

**Time** sets how much history the trace holds, up to 0.1 s, which is 4800 pairs at 48 kHz. The trace draws at most 2048 points, so it keeps every third pair there. It picks them by frame number, so the same pairs stay in the trace as new ones arrive, and the shape does not shimmer.

## How other systems do it

An analogue oscilloscope has an XY switch that feeds one channel to the horizontal plates. Eurorack and software patching tools offer the same view, often called a vectorscope or a goniometer when it shows stereo. In LoomDSP it is a module on a tile, so it saves with the patch and can sit in the signal path without changing the sound.
