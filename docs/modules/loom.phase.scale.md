# Phase Scale

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Runs a phase at multiply / divide times its speed, locked to the input. Divide counts the input's wraps from the first cycle or the last reset.

| | |
|---|---|
| ID | `loom.phase.scale` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | The phase to scale. |
| **Reset** (`reset`) | gate | 0 | A rising edge starts counting the input's wraps again from zero. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | The scaled phase, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Multiply** (`multiply`) | 1 to 16, whole numbers | 1 | How many times faster the output runs. |
| **Divide** (`divide`) | 1 to 16, whole numbers | 1 | How many times slower the output runs: it wraps once every this many input wraps. |

## Panel

6 by 3 grid units.

- Knob: **Multiply** (`multiply`)
- Knob: **Divide** (`divide`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Phase Scale to create harmonic frequency ratios and phase-locked sub-oscillators. It multiplies or divides an incoming phase ramp by integer factors while maintaining phase alignment. Reset restarts the divider on demand.

## Example patch

Generates a sub-octave phase ramp by dividing the master phase by 2.

```ron
Patch(
    version: 1,
    name: "Sub Octave Phase",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.phase.scale@1", params: {"multiply": 1, "divide": 2}),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.osc.pulse@1", params: {"width": 0.5}),
        PatchModule(id: 5, kind: "loom.mix.crossfade@1", params: {"mix": 0.3}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (1, "phase"), to: (3, "phase")),
        PatchCable(from: (2, "phase"), to: (4, "phase")),
        PatchCable(from: (3, "out"), to: (5, "a")),
        PatchCable(from: (4, "out"), to: (5, "b")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Phase Generator](loom.phase.generator.md)
