# Constant

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Outputs a constant value. Useful for pitch offsets, gates, and testing.

| | |
|---|---|
| ID | `loom.math.constant` |
| Version | 1 |
| Category | Math |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The value set by the parameter. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Value** (`value`) | -10 to 10, control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 0 | The number the output holds. |

## Panel

3 by 3 grid units.

- Knob: **Value** (`value`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
