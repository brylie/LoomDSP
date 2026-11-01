# Crossfade

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Blends two signals. Equal power keeps the loudness of audio steady; linear is exact for control signals.

| | |
|---|---|
| ID | `loom.mix.crossfade` |
| Version | 1 |
| Family | Level and routing |
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
| **Law** (`law`) | Equal power or Linear | Equal power | How the two signals share the mix: equal power, or linear. |

## Panel

11 by 5 grid units.

- Segmented control: **Law** (`law`)
- Knob: **Mix** (`mix`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Crossfade to blend two signals with one control: Mix at 0 is all A and at 1 is all B.

The equal power law keeps audio at the same loudness through the middle of the blend. The linear law gives exact in-between values, which suits control signals.

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
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
)
```

## See also

- [Mixer](loom.mix.mixer.md)
- [Stereo cables](../learn/stereo-cables.md)
