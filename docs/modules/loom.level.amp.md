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

3 by 6 grid units.

- Knob: **Gain** (`gain`)
- Radio buttons: **Response** (`response`) (Linear, Exponential)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
