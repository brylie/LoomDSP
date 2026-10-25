# Constant (pitch)

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Outputs a fixed pitch, set in semitones from middle C.

| | |
|---|---|
| ID | `loom.pitch.constant` |
| Version | 1 |
| Category | Pitch |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | pitch | bipolar | The pitch, at 0.1 per octave. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Semitones** (`semitones`) | -48 to 48 st | 0 st | How far from middle C the pitch is. 12 is one octave up. |

## Panel

3 by 3 grid units.

- Knob: **Semitones** (`semitones`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
