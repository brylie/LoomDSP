# Divide

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Divides signal A by signal B. Outputs 0 while B is near zero.

| | |
|---|---|
| ID | `loom.math.div` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The signal to divide. |
| **B** (`b`) | modulation | 0 | The divisor. Reads 1 while no cable is connected. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A divided by B, or 0 while B is within 1e-9 of zero. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
