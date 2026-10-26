# Clamp

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Limits a signal to the range between two limits. NaN becomes 0 first, so Clamp makes a signal safe.

| | |
|---|---|
| ID | `loom.level.clamp` |
| Version | 1 |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to limit. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input, held between the two limits. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Low** (`low`) | -10 to 10 | -1 | One limit. If it is above High, the two swap. |
| **High** (`high`) | -10 to 10 | 1 | The other limit. If it is below Low, the two swap. |

## Panel

6 by 3 grid units.

- Knob: **Low** (`low`)
- Knob: **High** (`high`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Clamp to limit a modulation or audio signal to a strict range between low and high bounds. It prevents extreme modulation values from destabilizing filters or wrapping phases. Any NaN input is converted to 0 first.

## Example patch

Restricts an LFO swing between 0 and 0.8.

```ron
Patch(
    version: 1,
    name: "Clamped LFO",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 2}),
        PatchModule(id: 2, kind: "loom.level.clamp@1", params: {"low": 0, "high": 0.8}),
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

- [Scale](loom.level.scale.md)
- [Output safety](../learn/output-safety.md)
