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
- [Polysynth patch](../../examples/patches/polysynth.ron)
