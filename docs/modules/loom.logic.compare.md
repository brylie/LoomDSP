# Compare

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Makes a gate from two signals: high while A is above B, or while A is below B. Hysteresis keeps a noisy signal from chattering.

| | |
|---|---|
| ID | `loom.logic.compare` |
| Version | 1 |
| Category | Logic |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The signal to test. |
| **B** (`b`) | modulation | 0 | The threshold A is compared with. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Gate** (`gate`) | gate | unipolar | High while the comparison holds. Equal values count as low. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 1, whole numbers | 0 | Above: high while A is above B. Below: high while A is below B. |
| **Hysteresis** (`hysteresis`) | 0 to 1 | 0 | A dead band around B. Once the gate is high, A must go this far past B the other way to end it. |

## Panel

6 by 4 grid units.

- Radio buttons: **Mode** (`mode`) (Above, Below)
- Knob: **Hysteresis** (`hysteresis`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
