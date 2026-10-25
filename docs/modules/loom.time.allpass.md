# All-Pass

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Delay-line all-pass filter. Passes all frequencies at equal steady-state level while smearing phase, for reverbs and diffusers.

| | |
|---|---|
| ID | `loom.time.allpass` |
| Version | 1 |
| Category | Time |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to pass through the filter. |
| **Time** (`time`) | modulation | 0 | Adds to the delay time, in seconds. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The all-pass filtered signal. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Time** (`time`) | 0.0001 to 0.2 s, logarithmic | 0.01 s | Delay time of the all-pass loop. |
| **Gain** (`gain`) | -0.95 to 0.95 | 0.5 | Feedback and feed-forward coefficient. 0 is a plain delay. |

## Panel

6 by 3 grid units.

- Knob: **Time** (`time`)
- Knob: **Gain** (`gain`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
