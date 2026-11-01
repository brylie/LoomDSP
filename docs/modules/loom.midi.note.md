# MIDI Note

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Monophonic note input (last-note priority). Pitch includes pitch bend.

| | |
|---|---|
| ID | `loom.midi.note` |
| Version | 1 |
| Family | Interface |
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

## When to use it

Use MIDI Note to play a patch from a keyboard. It gives the pitch of the last key held, a gate that is high while any key is held, and the velocity of the last key.

Pitch uses 0.1 per octave with 0 at middle C, and includes pitch bend. Bend Range sets how far a full bend moves it. Channel picks one MIDI channel, or 0 for all.

## Example patch

Plays a monophonic synthesizer voice from incoming MIDI notes.

```ron
Patch(
    version: 1,
    name: "Monophonic Synth",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", params: {"channel": 0, "bend-range": 2}),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.env.envelope@1"),
        PatchModule(id: 5, kind: "loom.level.amp@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (1, "gate"), to: (4, "gate")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (5, "in")),
        PatchCable(from: (4, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [Triggers and gates](../learn/triggers-and-gates.md)
- [Sine patch](../../examples/patches/sine.ron)
