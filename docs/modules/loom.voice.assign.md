# Voice Assigner

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Gives notes from any source to this module's own voices, with unison.

| | |
|---|---|
| ID | `loom.voice.assign` |
| Version | 1 |
| Family | Level and routing |
| Category | Voice |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | 0 | The pitch of each input voice's note. 0 is middle C. |
| **Gate** (`gate`) | gate | 0 | Each rising edge starts a note on this input voice. Nothing plays without a cable here. |
| **Velocity** (`velocity`) | modulation | 1 | The velocity of each input voice's note, from 0 to 1. |
| **Pressure** (`pressure`) | modulation | 0 | The pressure of each input voice's note, from 0 to 1. It follows the note while it sounds. |
| **Timbre** (`timbre`) | modulation | 0 | The timbre of each input voice's note, from 0 to 1. It follows the note while it sounds. |
| **Level** (`level`) | modulation | 0 | The loudness of each output voice, patched back from its envelope, for Quietest stealing. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | bipolar | The pitch of this voice's note. 0 is middle C. |
| **Gate** (`gate`) | gate | unipolar | High while this voice holds a note. |
| **Velocity** (`velocity`) | modulation | unipolar | The velocity of this voice's note, from 0 to 1. |
| **Pressure** (`pressure`) | modulation | unipolar | The pressure of this voice's note, from 0 to 1. |
| **Timbre** (`timbre`) | modulation | unipolar | The timbre of this voice's note, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Voices** (`voices`) | 1 to 16, whole numbers | 1 | Number of simultaneous voices. |
| **Mode** (`mode`) | Oldest, Quietest, Legato, or Retrigger | Legato | How notes steal voices, or how a single voice articulates notes. |
| **Unison** (`unison`) | 1 to 4, whole numbers | 1 | How many voices each note plays. |

## Panel

12 by 6 grid units.

- Segmented control: **Mode** (`mode`)
- Number: **Voices** (`voices`)
- Light: **Gate** (`gate`): Lit while a note is held.
- Number: **Unison** (`unison`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Voice assigner to play notes from any source on several voices: a sequencer, a Euclidean pattern, a random melody, or another bundle. Note input does this for MIDI. Voice assigner does it for notes that arrive on cables.

- **Nothing plays with no cable on Gate.** Each rising edge on an input voice's gate starts a note, and its fall ends it.
- **How many notes can arrive at once** is the widest of the Pitch, Gate, Velocity, Pressure, and Timbre cables. A mono pitch and a mono gate give one note per edge. A 4-voice pitch bundle with a mono gate gives a 4-note chord per edge.
- **A note follows its input voice.** While its gate stays high, its output voice follows that input voice's pitch, velocity, pressure, and timbre. A pitch that moves while the gate stays high glides on the same voice, with no new attack. So a sequencer needs a gap between its gates for release tails to overlap on different voices.
- **A stolen note stays silent** until its input gate rises again.
- **Voices 1, the default, is a mono pass-through** with last-note priority, in Legato or Retrigger. When the newest note ends, the one held before it returns, at its own velocity. Note input plays a returned note at the velocity of the last key pressed.
- **Unison** gives each note that many voices, taken and released together. Put a Voice spread after the module to detune them. Unison is hidden in the one-voice modes, where it does nothing.
- **Quietest** needs the envelopes after the module patched back to Level. With nothing on Level, Quietest steals like Oldest.

## Example patch

A clock plays a random melody, one note per beat, on 8 voices. Each note's release tail overlaps the next note on another voice.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Random melody on 8 voices"),
    modules: [
        PatchModule(id: 1, kind: "loom.clock.clock@1"),
        PatchModule(id: 2, kind: "loom.rand.random@1", params: {"low": -0.1, "high": 0.1}),
        PatchModule(id: 3, kind: "loom.voice.assign@1", params: {"voices": 8, "mode": 0}),
        PatchModule(id: 4, kind: "loom.osc.saw@1"),
        PatchModule(id: 5, kind: "loom.env.envelope@1", params: {"release": 1.5}),
        PatchModule(id: 6, kind: "loom.level.amp@1", params: {"gain": 0.2}),
        PatchModule(id: 7, kind: "loom.voice.merge@1"),
        PatchModule(id: 8, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "trigger"), to: (2, "trigger")),
        PatchCable(from: (1, "gate"), to: (3, "gate")),
        PatchCable(from: (2, "out"), to: (3, "pitch")),
        PatchCable(from: (3, "pitch"), to: (4, "pitch")),
        PatchCable(from: (3, "gate"), to: (5, "gate")),
        PatchCable(from: (4, "out"), to: (6, "in")),
        PatchCable(from: (5, "out"), to: (6, "level")),
        PatchCable(from: (6, "out"), to: (7, "in")),
        PatchCable(from: (7, "out"), to: (8, "in")),
    ],
)
```

## See also

- [Voices and polyphony](../learn/voices-and-polyphony.md)
- [MIDI Note](loom.midi.note.md)
- [Voice Merge](loom.voice.merge.md)
- [Voice Spread](loom.voice.spread.md)
