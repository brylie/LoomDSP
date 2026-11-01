# Pitch Constant

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Outputs a fixed pitch, set in semitones from middle C.

| | |
|---|---|
| ID | `loom.pitch.constant` |
| Version | 1 |
| Family | Notes and pitch |
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

## When to use it

Use Pitch Constant when a patch needs a fixed pitch, set in semitones from middle C: a drone, a fixed interval to add to a note, or a test tone. 12 semitones is one octave, which is 0.1 in pitch.

## Example patch

Sets an oscillator to play a fixed pitch 7 semitones above middle C.

```ron
Patch(
    version: 1,
    name: "Fixed Fifth Pitch",
    modules: [
        PatchModule(id: 1, kind: "loom.pitch.constant@1", params: {"semitones": 7}),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.sine@1"),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [MIDI Note](loom.midi.note.md)
