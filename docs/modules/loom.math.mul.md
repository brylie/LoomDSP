# Multiply

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Multiplies two signals and a gain. Use it as a ring modulator. To control loudness, use Amplifier.

| | |
|---|---|
| ID | `loom.math.mul` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | audio | 0 | The signal to scale. |
| **B** (`b`) | modulation | 0 | Scales A. Reads 1 while no cable is connected. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | A times B times the gain. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Gain** (`gain`) | -10 to 10 | 1 | A fixed amount the product is multiplied by. |

## Panel

3 by 3 grid units.

- Knob: **Gain** (`gain`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
