# Scale

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Multiplies a signal by a factor and adds an offset. A factor of -1 inverts. Bipolar to unipolar is factor 0.5 with offset 0.5.

| | |
|---|---|
| ID | `loom.level.scale` |
| Version | 1 |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to scale. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input times the factor, plus the offset. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Factor** (`factor`) | -1 to 1 | 1 | What the input is multiplied by. -1 flips it upside down. |
| **Offset** (`offset`) | -10 to 10 | 0 | A fixed amount added after the factor. |

## Panel

6 by 3 grid units.

- Knob: **Factor** (`factor`)
- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Scale to remap, invert, or offset control voltages. Multiplying by -1 inverts a signal, while setting factor to 0.5 and offset to 0.5 transforms a bipolar signal (-1 to 1) into a unipolar signal (0 to 1).

## Example patch

Converts a bipolar LFO into a unipolar envelope to modulate filter cutoff.

```ron
Patch(
    version: 1,
    name: "Bipolar to Unipolar",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 1}),
        PatchModule(id: 2, kind: "loom.level.scale@1", params: {"factor": 0.5, "offset": 0.5}),
        PatchModule(id: 3, kind: "loom.phase.generator@1"),
        PatchModule(id: 4, kind: "loom.osc.saw@1"),
        PatchModule(id: 5, kind: "loom.filter.svf@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (5, "cutoff")),
        PatchCable(from: (3, "phase"), to: (4, "phase")),
        PatchCable(from: (4, "out"), to: (5, "in")),
        PatchCable(from: (5, "lowpass"), to: (6, "left")),
    ],
)
```

## See also

- [Modulation without cables](../learn/modulation-without-cables.md)
- [Clamp](loom.level.clamp.md)
