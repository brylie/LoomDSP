# Amplifier

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Scales a signal by a control signal, with a linear or exponential response.

| | |
|---|---|
| ID | `loom.level.amp` |
| Version | 1 |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to scale. |
| **Level** (`level`) | modulation | 0 | How loud the output is. Values below 0 count as 0. Reads 1 while no cable is connected. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The input times the level, after the response curve, times the gain. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Gain** (`gain`) | 0 to 2 | 1 | A fixed amount the output is multiplied by. |
| **Response** (`response`) | 0 to 1, whole numbers, choices Linear, Exponential | 0 | How the level maps to loudness: linear, or exponential over 60 dB. |

## Panel

6 by 6 grid units.

- Knob: **Gain** (`gain`)
- Radio buttons: **Response** (`response`) (Linear, Exponential)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Amplifier to scale an audio signal with a control voltage or envelope. Choose linear response for modulation signals and exponential response for natural audio volume control.

## Example patch

Modulates oscillator amplitude with an LFO.

```ron
Patch(
    version: 1,
    name: "Tremolo",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.mod.lfo@1", params: {"rate": 5}),
        PatchModule(id: 4, kind: "loom.level.scale@1", params: {"factor": 0.5, "offset": 0.5}),
        PatchModule(id: 5, kind: "loom.level.amp@1", params: {"response": 0}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (5, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "left")),
    ],
)
```

## See also

- [Multiply](loom.math.mul.md)
- [ADSR](loom.env.adsr.md)
