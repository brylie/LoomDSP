# MIDI Note

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Monophonic note input (last-note priority). Pitch includes pitch bend.

| | |
|---|---|
| ID | `loom.midi.note` |
| Version | 1 |
| Category | MIDI |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | bipolar | Pitch of the last note held, with pitch bend. 0 is middle C. |
| **Gate** (`gate`) | gate | unipolar | High while any note is held. |
| **Velocity** (`velocity`) | modulation | unipolar | How hard the last note was played, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Channel** (`channel`) | 0 to 16, whole numbers | 0 | The MIDI channel to listen to, from 1 to 16. 0 listens to all. |
| **Bend Range** (`bend-range`) | 0 to 48 st, whole numbers | 2 st | How far a full pitch bend moves the pitch. |

## Panel

3 by 2 grid units.

- Light: **Gate** (`gate`): Lit while a key is held.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
