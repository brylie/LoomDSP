# Voice Split

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Takes the voices of a bundle out on separate outputs, from voice First on. For the two sides of a stereo cable, use Split.

| | |
|---|---|
| ID | `loom.voice.split` |
| Version | 1 |
| Family | Level and routing |
| Category | Voice |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The voices to take apart. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out 1** (`out1`) | audio | bipolar | The voice First names. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 2** (`out2`) | audio | bipolar | The voice after Out 1's. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 3** (`out3`) | audio | bipolar | The voice after Out 2's. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 4** (`out4`) | audio | bipolar | The voice after Out 3's. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 5** (`out5`) | audio | bipolar | The voice after Out 4's. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 6** (`out6`) | audio | bipolar | The voice after Out 5's. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 7** (`out7`) | audio | bipolar | The voice after Out 6's. Past the input's last voice it is 0, the input's unpatched value. |
| **Out 8** (`out8`) | audio | bipolar | The voice after Out 7's. Past the input's last voice it is 0, the input's unpatched value. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **First** (`first`) | 1 to 16, whole numbers | 1 | The voice on Out 1. Each output after it carries the next voice. |

## Panel

5 by 2 grid units.

- Number: **First** (`first`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Voice split where one voice of a bundle should go its own way: to a meter, to a separate effect, or to a module that is not polyphonic. `out1` carries the voice that First names, and each output after it carries the next voice. So with First at 3, `out1` is voice 3 and `out2` is voice 4.

An output past the last voice of the bundle is 0, the input's unpatched value. A mono cable is voice 1 only: it reaches `out1`, and the other outputs hold 0. Eight outputs reach eight voices; for voices 9 to 16, add a second Voice split with First at 9.

First can be modulated. It switches on the sample, with no fade, because it picks voices rather than setting a level.

A stereo voice keeps both of its sides: each output is stereo when the bundle is. To take the two sides of a stereo cable apart, use Split.

## Example patch

Two voices from MIDI Note, each pitch on its own meter. Play two notes to see which voice took which note.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Two voices on meters"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", params: {"voices": 2}),
        PatchModule(id: 2, kind: "loom.voice.split@1"),
        PatchModule(id: 3, kind: "loom.display.meter@1"),
        PatchModule(id: 4, kind: "loom.display.meter@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "in")),
        PatchCable(from: (2, "out1"), to: (3, "in")),
        PatchCable(from: (2, "out2"), to: (4, "in")),
    ],
)
```

## See also

- [Voices and polyphony](../learn/voices-and-polyphony.md)
- [Voice Merge](loom.voice.merge.md)
- [Voice Spread](loom.voice.spread.md)
- [Split](loom.mix.split.md)
