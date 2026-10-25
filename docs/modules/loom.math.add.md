# Add

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Adds two signals and an offset.

| | |
|---|---|
| ID | `loom.math.add` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The first signal to add. |
| **B** (`b`) | modulation | 0 | The second signal to add. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A plus B plus the offset. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Offset** (`offset`) | -10 to 10 | 0 | A fixed amount added to the sum. |

## Panel

3 by 3 grid units.

- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
