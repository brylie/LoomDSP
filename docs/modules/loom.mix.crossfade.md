# Crossfade

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Blends two signals. Equal power keeps the loudness of audio steady; linear is exact for control signals.

| | |
|---|---|
| ID | `loom.mix.crossfade` |
| Version | 1 |
| Category | Mix |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | audio | 0 | The signal heard when the mix is 0. |
| **B** (`b`) | audio | 0 | The signal heard when the mix is 1. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | A blend of A and B. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mix** (`mix`) | 0 to 1 | 0.5 | 0 is all A and 1 is all B. |
| **Law** (`law`) | 0 to 1, whole numbers, choices Equal power, Linear | 0 | How the two signals share the mix: equal power, or linear. |

## Panel

3 by 6 grid units.

- Knob: **Mix** (`mix`)
- Radio buttons: **Law** (`law`) (Equal power, Linear)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Crossfade to smoothly blend two signals using a mix control from 0 (pure A) to 1 (pure B). Equal power law preserves perceived volume when mixing audio signals, while linear law ensures exact proportional summation for control voltages.

## Example patch

Fades smoothly between a saw oscillator and a pulse oscillator.

```ron
Patch(
    version: 1,
    name: "Oscillator Crossfade",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.osc.pulse@1", params: {"width": 0.2}),
        PatchModule(id: 4, kind: "loom.mix.crossfade@1", params: {"mix": 0.5, "law": 0}),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (1, "phase"), to: (3, "phase")),
        PatchCable(from: (2, "out"), to: (4, "a")),
        PatchCable(from: (3, "out"), to: (4, "b")),
        PatchCable(from: (4, "out"), to: (5, "left")),
    ],
)
```

## See also

- [Mixer](loom.mix.mixer.md)
- [Stereo cables](../learn/stereo-cables.md)
