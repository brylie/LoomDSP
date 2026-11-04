# XY Scope

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Shows one signal against another. Passes both inputs through unchanged.

| | |
|---|---|
| ID | `loom.display.xy` |
| Version | 1 |
| Family | Interface |
| Category | Display |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **X** (`x`) | audio | 0 | The signal that moves the trace left and right. |
| **Y** (`y`) | audio | 0 | The signal that moves the trace up and down. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **X** (`x_out`) | audio | bipolar | The X input, unchanged. |
| **Y** (`y_out`) | audio | bipolar | The Y input, unchanged. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Range** (`range`) | ±0.1, ±1, or ±10 | ±1 | The value at each edge of the frame. A point beyond it stays at the edge. |
| **Time** (`time`) | 0.001 to 0.1 s, logarithmic | 0.02 s | How much history the trace shows. |
| **Rotate** (`rotate`) | 0 to 1, whole numbers | 0 | Turns the trace by 45 degrees, so a mono signal is a vertical line. |

## Panel

6 by 11 grid units.

- XY scope: **X** (`x_out`) across, **Y** (`y_out`) up
- Segmented control: **Range** (`range`)
- Knob: **Time** (`time`)
- Toggle: **Rotate** (`rotate`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use XY Scope to see how two signals relate. X moves the trace left and right, and Y moves it up and down. Older points fade, so the newest part is brightest.

Two sines a quarter cycle apart draw a circle, and two equal signals draw a diagonal line. To see how wide a stereo signal is, connect it to X and leave Y empty: the left side moves the trace across and the right side moves it up. Turn on Rotate for a mid and side view, where a mono signal is a vertical line.

Range sets the value at each edge of the frame, and a point beyond it stays at the edge. Time sets how much history the trace shows. Both inputs pass through unchanged.

## Example patch

Draws a circle from an LFO's two outputs, a quarter cycle apart.

```ron
Patch(
    version: 1,
    name: "XY Circle",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 50.0}),
        PatchModule(id: 2, kind: "loom.display.xy@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "x")),
        PatchCable(from: (1, "out90"), to: (2, "y")),
    ],
)
```

## See also

- [Reading an XY scope](../learn/reading-an-xy-scope.md)
- [Stereo cables](../learn/stereo-cables.md)
- [Scope](loom.display.scope.md)
