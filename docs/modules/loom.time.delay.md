# Delay Line

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Interpolated delay line. The time input adds seconds to the time knob.

| | |
|---|---|
| ID | `loom.time.delay` |
| Version | 1 |
| Category | Time |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to delay. |
| **Time** (`time`) | modulation | 0 | Adds to the delay time, in seconds. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The delayed signal. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Time** (`time`) | 0.0001 to 2 s, logarithmic | 0.2 s | How long the signal is delayed. |

## Panel

3 by 3 grid units.

- Knob: **Time** (`time`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Delay to hold audio or control signals in a circular buffer and replay them later. It creates echoes, slapback, comb filters, and chorus effects. The delay time parameter modulates smoothly with cubic interpolation.

## Example patch

Applies an echo delay to an envelope-shaped oscillator pulse.

```ron
Patch(
    version: 1,
    name: "Echo Delay",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.triangle@1"),
        PatchModule(id: 4, kind: "loom.env.envelope@1", params: {"attack": 0.01, "decay": 0.1, "sustain": 0, "release": 0.1}),
        PatchModule(id: 5, kind: "loom.level.amp@1"),
        PatchModule(id: 6, kind: "loom.time.delay@1", params: {"time": 0.25}),
        PatchModule(id: 7, kind: "loom.mix.crossfade@1", params: {"mix": 0.4}),
        PatchModule(id: 8, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (1, "gate"), to: (4, "gate")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (5, "in")),
        PatchCable(from: (4, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "in")),
        PatchCable(from: (5, "out"), to: (7, "a")),
        PatchCable(from: (6, "out"), to: (7, "b")),
        PatchCable(from: (7, "out"), to: (8, "left")),
    ],
)
```

## See also

- [Delays and interpolation](../learn/delays-and-interpolation.md)
- [Feedback loops and the unit delay](../learn/feedback-loops.md)
