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
| **Offset** (`offset`) | -10 to 10, control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 0 | A fixed amount added to the sum. |

## Panel

3 by 3 grid units.

- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
