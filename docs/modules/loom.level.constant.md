# Constant (level)

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Outputs a fixed level, set in decibels. 0 dB is a factor of 1.

| | |
|---|---|
| ID | `loom.level.constant` |
| Version | 1 |
| Category | Level |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | unipolar | The level as a factor: 10 to the power of dB over 20. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Level** (`level`) | -60 to 12 dB | 0 dB | The level in decibels. 0 dB is 1.0 and -6 dB is about 0.5. |

## Panel

3 by 3 grid units.

- Knob: **Level** (`level`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
