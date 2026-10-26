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
