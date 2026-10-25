# Clock Divide

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Fires on every Nth trigger. Offset fires earlier in the count. Reset starts the count again.

| | |
|---|---|
| ID | `loom.logic.divide` |
| Version | 1 |
| Category | Logic |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | 0 | The triggers to count. |
| **Reset** (`reset`) | gate | 0 | Starts the count again. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | unipolar | Fires on every Nth trigger. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Division** (`n`) | 1 to 64, whole numbers | 2 | Fires on every Nth trigger. |
| **Offset** (`offset`) | 0 to 63, whole numbers | 0 | Fires this many triggers earlier in the count. |

## Panel

6 by 3 grid units.

- Knob: **Division** (`n`)
- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
