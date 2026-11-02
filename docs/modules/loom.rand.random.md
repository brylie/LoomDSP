# Random

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A new random value on each trigger, within a range, or a random walk. The seed makes it repeatable.

| | |
|---|---|
| ID | `loom.rand.random` |
| Version | 1 |
| Family | Modulation |
| Category | Random |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | 0 | A rising edge makes a new value on both outputs. |
| **Reset** (`reset`) | gate | 0 | A rising edge restarts the values from the seed. A walk also goes back to the middle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A random value. It starts in the middle of the range. |
| **Out 2** (`out2`) | modulation | bipolar | A second random value, independent of the first. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | Step or Walk | Step | Step: each trigger picks a new value. Walk: each trigger moves a little from the last value. |
| **Low** (`low`) | -10 to 10. Control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 0 | One end of the range. |
| **High** (`high`) | -10 to 10. Control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 1 | The other end of the range. |
| **Shape** (`shape`) | Uniform or Centred | Uniform | Uniform: every value is equally likely. Centred: values near the middle are more likely. |
| **Step** (`step`) | 0 to 1 | 0.1 | The largest move of a walk, as a fraction of the range. |
| **Glide** (`glide`) | 0 to 5 s | 0 s | The time each output takes to reach a new value, in a straight line. At 0 it steps. |
| **Seed** (`seed`) | 1 to 65535, whole numbers | 1 | Picks the sequence of values. The same seed gives the same values. |

## Panel

12 by 5 grid units.

- Segmented control: **Mode** (`mode`)
- Segmented control: **Shape** (`shape`)
- Knob: **Low** (`low`)
- Knob: **High** (`high`)
- Knob: **Step** (`step`)
- Knob: **Glide** (`glide`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
