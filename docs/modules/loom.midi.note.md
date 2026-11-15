# MIDI Note

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Allocates MIDI notes to voices, with pitch bend, pressure, and timbre.

| | |
|---|---|
| ID | `loom.midi.note` |
| Version | 1 |
| Family | Interface |
| Category | MIDI |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Level** (`level`) | modulation | 0 | Per-voice envelope level for Quietest stealing. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | bipolar | Pitch of this voice, with pitch bend. 0 is middle C. |
| **Gate** (`gate`) | gate | unipolar | High while this voice holds a note. |
| **Velocity** (`velocity`) | modulation | unipolar | How hard this voice was played, from 0 to 1. |
| **Pressure** (`pressure`) | modulation | unipolar | Per-note pressure, from 0 to 1. |
| **Timbre** (`timbre`) | modulation | unipolar | Per-note CC74 expression, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Voices** (`voices`) | 1 to 16, whole numbers | 1 | Number of simultaneous voices. |
| **Mode** (`mode`) | Oldest, Quietest, Legato, or Retrigger | Legato | How notes steal voices, or how a single voice articulates notes. |
| **Channel** (`channel`) | 0 to 16, whole numbers | 0 | The MIDI channel to listen to, from 1 to 16. 0 listens to all. |
| **Bend Range** (`bend-range`) | 0 to 48 st, whole numbers | 2 st | How far a full pitch bend moves the pitch. |

## Panel

12 by 5 grid units.

- Segmented control: **Mode** (`mode`)
- Number: **Voices** (`voices`)
- Light: **Gate** (`gate`): Lit while a key is held.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use MIDI Note to play a patch from a keyboard. Voices sets how many notes can overlap. Pitch, gate, velocity, pressure, and timbre travel together as one signal per voice. The modules after it run one copy per voice automatically. At one voice, Legato keeps the gate high while any key is held; Retrigger gives each new note a low gate sample first.

Pitch uses 0.1 per octave with 0 at middle C, and includes pitch bend. Bend Range sets how far a full bend moves ordinary MIDI. Channel picks one MIDI channel, or 0 for all. An announced MPE zone uses its member bend range (48 semitones initially) plus its manager bend (2 semitones initially). Channel 1 follows the lower zone, and channel 16 the upper zone. Pressure and timbre follow each note. Zone announcements survive New and Open; if a device announced before LoomDSP started listening, toggle its MPE mode or reconnect it to announce again.

Oldest steals the note that started first. Quietest compares the voices on Level, usually patched from an envelope. Without that cable it uses Oldest. The Level feedback cable closes a loop; the compiler reports where it inserts a one-sample delay.

## Example patch

Plays a monophonic synthesizer voice from incoming MIDI notes.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Monophonic Synth"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", params: {"channel": 0, "bend-range": 2}),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.env.envelope@1"),
        PatchModule(id: 4, kind: "loom.level.amp@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (1, "gate"), to: (3, "gate")),
        PatchCable(from: (2, "out"), to: (4, "in")),
        PatchCable(from: (3, "out"), to: (4, "level")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [Triggers and gates](../learn/triggers-and-gates.md)
- [Sine patch](../../examples/patches/sine.ron)
- [Polysynth patch](../../examples/patches/polysynth.ron)
