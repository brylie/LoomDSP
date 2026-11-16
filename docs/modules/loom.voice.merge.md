# Voice Merge

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Sums or averages voices, or follows one voice selected by its gate.

| | |
|---|---|
| ID | `loom.voice.merge` |
| Version | 1 |
| Family | Level and routing |
| Category | Voice |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The voices to merge. |
| **Gate** (`gate`) | gate | 1 | Which voices are playing. A gate edge updates the selected voice. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The merged signal, with the input's stereo sides. |
| **Gate** (`gate`) | gate | unipolar | High while the voice Out follows is playing. In Sum and Average, while any voice plays. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | Sum, Average, Last, Lowest, or Highest | Sum | Sum or average all voices, or select the last, lowest, or highest playing voice. |

## Panel

20 by 2 grid units.

- Segmented control: **Mode** (`mode`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Voice merge where a bundle should become one signal. Sum adds the voices for audio. Average divides their sum by their count for modulation. Last, Lowest, and Highest follow one playing voice. A gate edge updates the selection; moving signals between edges cannot switch it. Switching into Last, Lowest, or Highest selects an active voice immediately. If a count edit removes the selected voice, an active surviving voice takes over. With no gates playing, the last selection stays. A stereo voice keeps both sides together.

The **Gate** output says whether the voice Out follows is playing. In Last, Lowest, and Highest it carries that voice's gate. When the choice moves from one held voice to another, it stays high and does not retrigger. When every voice stops, it falls, and Out keeps the last choice. In Sum and Average it is high while any voice plays. With nothing patched to the Gate input, every voice counts as playing, so the output is 1.

A voice whose In value is not a number, or is infinite, never wins Lowest or Highest over a voice with a real value. If every playing voice is broken, the lowest playing voice is chosen and its value passes through.

## A bass line from the lowest note

Take the lowest note of a chord as a mono bass. Patch MIDI Note's `pitch` bundle to Voice merge's In and its `gate` bundle to Voice merge's Gate, and set Mode to Lowest. Out is the lowest held pitch, and the Gate output plays one envelope for the bass. Releasing the lowest key hands the bass to the next lowest without a new attack, as on a mono keyboard.

## Example patch

The two note velocities are averaged into one signal and shown by a meter. A released voice retains its last velocity.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Average two velocities"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", params: {"voices": 2}),
        PatchModule(id: 2, kind: "loom.voice.merge@1", params: {"mode": 1}),
        PatchModule(id: 3, kind: "loom.display.meter@1"),
    ],
    cables: [
        PatchCable(from: (1, "velocity"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)
```

## See also

- [Voices and polyphony](../learn/voices-and-polyphony.md)
- [MIDI Note](loom.midi.note.md)
- [Voice Assigner](loom.voice.assign.md)
- [Polysynth patch](../../examples/patches/polysynth.ron)
