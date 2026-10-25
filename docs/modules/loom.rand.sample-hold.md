# Sample and hold

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values.

| | |
|---|---|
| ID | `loom.rand.sample-hold` |
| Version | 1 |
| Category | Random |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to sample. |
| **Trigger** (`trigger`) | gate | 0 | A rising edge takes a new value (Sample), or lets the input through while high (Track). |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The held value. It starts at 0. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 1, whole numbers | 0 | Sample: take the value on the rising edge. Track: follow the input while the trigger is high. |

## Panel

3 by 3 grid units.

- Radio buttons: **Mode** (`mode`) (Sample, Track)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
