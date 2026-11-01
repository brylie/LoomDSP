# Convert

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Converts between decibels and level, and between pitch and frequency in Hz.

| | |
|---|---|
| ID | `loom.math.convert` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The value to convert. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The converted value. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 3, whole numbers, choices dB to level, Level to dB, Pitch to Hz, Hz to pitch | 0 | Which conversion to do: dB to level, level to dB, pitch to Hz, or Hz to pitch. |

## Panel

6 by 5 grid units.

- Radio buttons: **Mode** (`mode`) (dB to level, Level to dB, Pitch to Hz, Hz to pitch)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Convert to translate between musical representations: decibels and linear gain, or pitch (0.1 per octave) and frequency in Hertz. This enables exact acoustic calculations and cross-domain modulation.

## Example patch

Converts a decibel value into linear gain for an amplifier.

```ron
Patch(
    version: 1,
    name: "dB to level",
    modules: [
        PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": -6}),
        PatchModule(id: 2, kind: "loom.math.convert@1", params: {"mode": 0}),
        PatchModule(id: 3, kind: "loom.phase.generator@1"),
        PatchModule(id: 4, kind: "loom.osc.sine@1"),
        PatchModule(id: 5, kind: "loom.level.amp@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (3, "phase"), to: (4, "phase")),
        PatchCable(from: (4, "out"), to: (5, "in")),
        PatchCable(from: (2, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [Amplifier](loom.level.amp.md)
