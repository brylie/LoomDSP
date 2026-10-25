# Pulse

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Band-limited pulse wave shaped from a phase input (polyBLEP).

| | |
|---|---|
| ID | `loom.osc.pulse` |
| Version | 1 |
| Category | Oscillator |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | Position in the wave, from 0 to 1. One ramp is one cycle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Pulse wave from -1 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Width** (`width`) | 0.01 to 0.99 | 0.5 | Share of the cycle spent high, from 0.01 to 0.99. 0.5 is a square wave. |

## Panel

3 by 3 grid units.

- Knob: **Width** (`width`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
