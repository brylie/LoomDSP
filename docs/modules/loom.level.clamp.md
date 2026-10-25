# Clamp

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Limits a signal to the range between two limits. NaN becomes 0 first, so Clamp makes a signal safe.

| | |
|---|---|
| ID | `loom.level.clamp` |
| Version | 1 |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to limit. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input, held between the two limits. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Low** (`low`) | -10 to 10 | -1 | One limit. If it is above High, the two swap. |
| **High** (`high`) | -10 to 10 | 1 | The other limit. If it is below Low, the two swap. |

## Panel

6 by 3 grid units.

- Knob: **Low** (`low`)
- Knob: **High** (`high`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
