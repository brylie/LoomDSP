# Level Constant

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Outputs a fixed level, set in decibels. 0 dB is a factor of 1.

| | |
|---|---|
| ID | `loom.level.constant` |
| Version | 1 |
| Family | Level and routing |
| Category | Level |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | unipolar | The level as a factor: 10 to the power of dB over 20. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Level** (`level`) | -60 to 12 dB | 0 dB | The level in decibels. 0 dB is 1.0 and -6 dB is about 0.5. |

## Panel

3 by 3 grid units.

- Knob: **Level** (`level`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Level Constant when a patch needs a fixed gain, set in decibels: a steady level for an Amplifier or a Multiply, or a reference level to compare against.

0 dB outputs 1.0, and -6 dB outputs about 0.5.

## Example patch

Attenuates an oscillator signal by 6 dB before sending it to audio out.

```ron
Patch(
    version: 1,
    name: "Fixed Level Attenuation",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.level.constant@1", params: {"level": -6}),
        PatchModule(id: 4, kind: "loom.level.amp@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (4, "in")),
        PatchCable(from: (3, "out"), to: (4, "level")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
)
```

## See also

- [Amplifier](loom.level.amp.md)
- [Convert](loom.math.convert.md)
