# Convert

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Converts between decibels and level, and between pitch and frequency in Hz.

| | |
|---|---|
| ID | `loom.math.convert` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The value to convert. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The converted value. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 3, whole numbers | 0 | Which conversion to do: dB to level, level to dB, pitch to Hz, or Hz to pitch. |

## Panel

3 by 5 grid units.

- Radio buttons: **Mode** (`mode`) (dB to level, Level to dB, Pitch to Hz, Hz to pitch)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
