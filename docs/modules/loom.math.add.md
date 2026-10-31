# Add

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Adds two signals and an offset.

| | |
|---|---|
| ID | `loom.math.add` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The first signal to add. |
| **B** (`b`) | modulation | 0 | The second signal to add. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A plus B plus the offset. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Offset** (`offset`) | -10 to 10, control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 0 | A fixed amount added to the sum. |

## Panel

3 by 3 grid units.

- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Add to sum two audio or modulation signals with a manual offset. It is commonly used for transposing pitch signals, mixing control voltages, or creating complex modulation waveforms.

## Example patch

Adds an LFO vibrato to a keyboard pitch before the oscillator phase generator.

```ron
Patch(
    version: 1,
    name: "Pitch Vibrato",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.mod.lfo@1", params: {"rate": 5}),
        PatchModule(id: 3, kind: "loom.level.scale@1", params: {"factor": 0.01}),
        PatchModule(id: 4, kind: "loom.math.add@1"),
        PatchModule(id: 5, kind: "loom.phase.generator@1"),
        PatchModule(id: 6, kind: "loom.osc.triangle@1"),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (4, "a")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "b")),
        PatchCable(from: (4, "out"), to: (5, "pitch")),
        PatchCable(from: (5, "phase"), to: (6, "phase")),
        PatchCable(from: (6, "out"), to: (7, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [Subtract](loom.math.sub.md)
