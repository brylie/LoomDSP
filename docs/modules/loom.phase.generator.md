# Phase

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs.

| | |
|---|---|
| ID | `loom.phase.generator` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | 0 | Sets the frequency. 0 is middle C, and +0.1 is one octave up. |
| **Reset** (`reset`) | gate | 0 | A rising edge sets the phase back to 0. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | A ramp from 0 up to 1, once per cycle. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Tune** (`tune`) | -48 to 48 st | 0 st | Shifts the pitch in semitones. |
| **Octave** (`octave`) | -10 to 10 oct | 0 oct | Shifts the pitch in octaves. |

## Panel

6 by 3 grid units.

- Knob: **Tune** (`tune`)
- Knob: **Octave** (`octave`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
