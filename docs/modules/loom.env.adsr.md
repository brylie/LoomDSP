# ADSR

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Envelope from 0 to 1, triggered by a gate. Linear attack, exponential decay and release.

| | |
|---|---|
| ID | `loom.env.adsr` |
| Version | 1 |
| Category | Envelope |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Gate** (`gate`) | gate | 0 | Going high starts the envelope. Going low releases it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | unipolar | The envelope level, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Attack** (`attack`) | 0.0005 to 10 s, logarithmic | 0.01 s | Time to rise from 0 to 1 after the gate goes high. |
| **Decay** (`decay`) | 0.0005 to 10 s, logarithmic | 0.2 s | Time to fall from 1 to the sustain level. |
| **Sustain** (`sustain`) | 0 to 1 | 0.7 | The level held while the gate stays high. |
| **Release** (`release`) | 0.0005 to 10 s, logarithmic | 0.3 s | Time to fall to 0 after the gate goes low. |

## Panel

6 by 8 grid units.

- Knob: **Attack** (`attack`)
- Knob: **Decay** (`decay`)
- Knob: **Sustain** (`sustain`)
- Knob: **Release** (`release`)
- Light: **Level** (`level`): The envelope's level, from dark at 0 to full at 1.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
