# Mixer

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Adds four signals, each with its own level and mute. Two mixers in a row make a bigger mix.

| | |
|---|---|
| ID | `loom.mix.mixer` |
| Version | 1 |
| Family | Level and routing |
| Category | Mix |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In 1** (`in1`) | audio | 0 | A signal to mix. Each side of a stereo cable is mixed on its own. |
| **In 2** (`in2`) | audio | 0 | A signal to mix. Each side of a stereo cable is mixed on its own. |
| **In 3** (`in3`) | audio | 0 | A signal to mix. Each side of a stereo cable is mixed on its own. |
| **In 4** (`in4`) | audio | 0 | A signal to mix. Each side of a stereo cable is mixed on its own. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The sum of the inputs, each times its level, muted inputs silent. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Level 1** (`level1`) | 0 to 2 | 1 | How loud this input is in the mix. |
| **Level 2** (`level2`) | 0 to 2 | 1 | How loud this input is in the mix. |
| **Level 3** (`level3`) | 0 to 2 | 1 | How loud this input is in the mix. |
| **Level 4** (`level4`) | 0 to 2 | 1 | How loud this input is in the mix. |
| **Mute 1** (`mute1`) | 0 to 1, whole numbers | 0 | Silences this input. The input fades out over 5 ms. |
| **Mute 2** (`mute2`) | 0 to 1, whole numbers | 0 | Silences this input. The input fades out over 5 ms. |
| **Mute 3** (`mute3`) | 0 to 1, whole numbers | 0 | Silences this input. The input fades out over 5 ms. |
| **Mute 4** (`mute4`) | 0 to 1, whole numbers | 0 | Silences this input. The input fades out over 5 ms. |

## Panel

12 by 5 grid units.

- Knob: **Level 1** (`level1`)
- Knob: **Level 2** (`level2`)
- Knob: **Level 3** (`level3`)
- Knob: **Level 4** (`level4`)
- Toggle: **Mute 1** (`mute1`)
- Toggle: **Mute 2** (`mute2`)
- Toggle: **Mute 3** (`mute3`)
- Toggle: **Mute 4** (`mute4`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Mixer to combine up to four signals with dedicated volume controls and click-free mutes. Muting an input fades the signal down smoothly over 5 milliseconds to prevent pops. Stereo signals are mixed independently on left and right channels.

## Example patch

Mixes three oscillator waveforms into a composite voice.

```ron
Patch(
    version: 1,
    name: "Four Channel Mix",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.osc.pulse@1"),
        PatchModule(id: 4, kind: "loom.osc.triangle@1"),
        PatchModule(id: 5, kind: "loom.mix.mixer@1", params: {"level1": 0.7, "level2": 0.5, "level3": 0.6, "level4": 0}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (1, "phase"), to: (3, "phase")),
        PatchCable(from: (1, "phase"), to: (4, "phase")),
        PatchCable(from: (2, "out"), to: (5, "in1")),
        PatchCable(from: (3, "out"), to: (5, "in2")),
        PatchCable(from: (4, "out"), to: (5, "in3")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Crossfade](loom.mix.crossfade.md)
- [Stereo cables](../learn/stereo-cables.md)
