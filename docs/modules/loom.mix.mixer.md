# Mixer

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Adds four signals, each with its own level and mute. Two mixers in a row make a bigger mix.

| | |
|---|---|
| ID | `loom.mix.mixer` |
| Version | 1 |
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
