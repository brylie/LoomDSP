# Scale

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Multiplies a signal by a factor and adds an offset. A factor of -1 inverts. Bipolar to unipolar is factor 0.5 with offset 0.5.

| | |
|---|---|
| ID | `loom.level.scale` |
| Version | 1 |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to scale. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input times the factor, plus the offset. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Factor** (`factor`) | -1 to 1 | 1 | What the input is multiplied by. -1 flips it upside down. |
| **Offset** (`offset`) | -10 to 10 | 0 | A fixed amount added after the factor. |

## Panel

6 by 3 grid units.

- Knob: **Factor** (`factor`)
- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
