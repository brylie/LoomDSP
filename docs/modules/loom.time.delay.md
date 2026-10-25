# Delay Line

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Interpolated delay line. The time input adds seconds to the time knob.

| | |
|---|---|
| ID | `loom.time.delay` |
| Version | 1 |
| Category | Time |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to delay. |
| **Time** (`time`) | modulation | 0 | Adds to the delay time, in seconds. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The delayed signal. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Time** (`time`) | 0.0001 to 2 s, logarithmic | 0.2 s | How long the signal is delayed. |

## Panel

3 by 3 grid units.

- Knob: **Time** (`time`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
