# Slew

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Limits how fast a signal can rise and fall. Times are per 1.0 of change, so a glide takes the same time on any signal.

| | |
|---|---|
| ID | `loom.level.slew` |
| Version | 1 |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to slow down. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input, moving toward it at a limited speed. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Rise** (`rise`) | 0.0005 to 10 s, logarithmic | 0.05 s | The time to go up by 1.0. |
| **Fall** (`fall`) | 0.0005 to 10 s, logarithmic | 0.05 s | The time to go down by 1.0. |

## Panel

6 by 3 grid units.

- Knob: **Rise** (`rise`)
- Knob: **Fall** (`fall`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
