# Phase Reset

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Restarts a phase at zero on a trigger. After that, the phase moves at the input's speed.

| | |
|---|---|
| ID | `loom.phase.reset` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | The phase to restart. With no cable, it follows the bar. |
| **Reset** (`reset`) | gate | 0 | A rising edge makes the output 0 on that frame. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | The input phase, counted from the last reset. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
