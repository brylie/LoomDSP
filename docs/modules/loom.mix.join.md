# Join

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Combines two signals into a stereo cable.

| | |
|---|---|
| ID | `loom.mix.join` |
| Version | 1 |
| Family | Level and routing |
| Category | Mix |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Left** (`left`) | audio | 0 | The signal for the left side. |
| **Right** (`right`) | audio | 0 | The signal for the right side. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio, stereo | bipolar | The combined stereo signal. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Join to combine separate left and right signals into one stereo cable.

## Example patch

This patch joins two constants and splits them again.

```ron
Patch(
    version: 1,
    name: "Stereo channels",
    modules: [
        PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 0.25}),
        PatchModule(id: 2, kind: "loom.math.constant@1", params: {"value": -0.5}),
        PatchModule(id: 3, kind: "loom.mix.join@1"),
        PatchModule(id: 4, kind: "loom.mix.split@1"),
        PatchModule(id: 6, kind: "loom.mix.join@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (6, "out"), to: (5, "in")),
        PatchCable(from: (1, "out"), to: (3, "left")),
        PatchCable(from: (2, "out"), to: (3, "right")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "left"), to: (6, "left")),
        PatchCable(from: (4, "right"), to: (6, "right")),
    ],
)
```

## See also

- [Stereo cables](../learn/stereo-cables.md)
