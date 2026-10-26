# 1-Sample Delay

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Delays the input by one sample. Marks where a feedback loop is broken.

| | |
|---|---|
| ID | `loom.time.delay1` |
| Version | 1 |
| Category | Time |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to delay. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The input, one sample later. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use 1-Sample Delay to explicitly break feedback loops in the audio or modulation graph. In DSP graphs, recursive feedback loops require at least one sample of delay. Placing delay1 allows you to pick exactly where the loop is delayed.

## Example patch

Feedback FM oscillator loop broken by a 1-sample delay.

```ron
Patch(
    version: 1,
    name: "Feedback Loop",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.math.mul@1", params: {"gain": 0.15}),
        PatchModule(id: 4, kind: "loom.time.delay1@1"),
        PatchModule(id: 5, kind: "loom.math.add@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "a")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "out"), to: (5, "b")),
        PatchCable(from: (5, "out"), to: (1, "pitch")),
        PatchCable(from: (2, "out"), to: (6, "left")),
    ],
)
```

## See also

- [Feedback loops and the unit delay](../learn/feedback-loops.md)
- [Why feedback FM turns to noise](../learn/feedback-fm.md)
- [FM feedback patch](../../examples/patches/fm-feedback-delay1.ron)
