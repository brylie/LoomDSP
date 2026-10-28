# Ladder

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A resonant low-pass with a 12, 18, or 24 dB slope. High resonance thins the bass and, at 18 and 24 dB, sings on its own. The cutoff input adds pitch: +0.1 doubles the cutoff.

| | |
|---|---|
| ID | `loom.filter.ladder` |
| Version | 1 |
| Category | Filter |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The audio to filter. |
| **Cutoff** (`cutoff`) | pitch | 0 | Moves the cutoff as pitch: +0.1 doubles it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Keeps the frequencies below the cutoff. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Cutoff** (`cutoff`) | 20 to 20000 Hz, logarithmic | 1000 Hz | The frequency where the filter starts to cut. |
| **Resonance** (`resonance`) | 0 to 1 | 0.2 | Feeds the output back to boost the cutoff. At 1, the 18 and 24 dB slopes oscillate on their own. |
| **Drive** (`drive`) | 0 to 1 | 0 | Raises the input level, up to 8 times (+18 dB), so the feedback saturates sooner. |
| **Slope** (`slope`) | 0 to 2, whole numbers, choices 12 dB, 18 dB, 24 dB | 2 | How steeply the filter cuts above the cutoff: two, three, or four stages in the loop. |

## Panel

9 by 6 grid units.

- Radio buttons: **Slope** (`slope`) (12 dB, 18 dB, 24 dB)
- Knob: **Cutoff** (`cutoff`)
- Knob: **Resonance** (`resonance`)
- Knob: **Drive** (`drive`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Ladder for the classic subtractive sound: a low-pass that thickens as you raise the resonance. Pick the slope on the panel: 24 dB is dark and steep, 18 dB is a little brighter, and 12 dB keeps more of the high end. At resonance 1 the 24 dB slope whistles at the cutoff and the 18 dB slope at about 1.7 times it, so with nothing at the input the filter becomes a sine oscillator. The 12 dB slope never whistles.

High resonance thins the bass, as on a classic ladder. Turn up Drive to get the level back and to push the feedback into saturation. Drive can alias at high settings. The cutoff input tracks pitch at 0.1 per octave, so an envelope or the note's pitch can move it.

## Example patch

A saw through the ladder, with an envelope opening the cutoff on every note.

```ron
Patch(
    version: 1,
    name: "Ladder Sweep",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.env.envelope@1", params: {"attack": 0.005, "decay": 0.4, "sustain": 0.2, "curve": 0.5}),
        PatchModule(id: 5, kind: "loom.filter.ladder@1", params: {"cutoff": 300, "resonance": 0.7}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (1, "gate"), to: (4, "gate")),
        PatchCable(from: (3, "out"), to: (5, "in")),
        PatchCable(from: (4, "out"), to: (5, "cutoff")),
        PatchCable(from: (5, "out"), to: (6, "left")),
        PatchCable(from: (5, "out"), to: (6, "right")),
    ],
)
```

## See also

- [How a ladder filter sings](../learn/ladder-filter.md)
- [Pitch at 0.1 per octave](../learn/pitch.md)
- [SVF](loom.filter.svf.md)
- [One-pole](loom.filter.onepole.md)
