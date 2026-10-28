# Sample and hold

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values.

| | |
|---|---|
| ID | `loom.rand.sample-hold` |
| Version | 1 |
| Category | Random |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to sample. |
| **Trigger** (`trigger`) | gate | 0 | A rising edge takes a new value (Sample), or lets the input through while high (Track). |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The held value. It starts at 0. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 1, whole numbers, choices Sample, Track | 0 | Sample: take the value on the rising edge. Track: follow the input while the trigger is high. |

## Panel

6 by 3 grid units.

- Radio buttons: **Mode** (`mode`) (Sample, Track)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Sample and hold to capture and freeze instantaneous values on incoming trigger edges. When fed with white noise, it produces classic stepped random control voltages. In Track mode, it passes the input while trigger is high.

## Example patch

Samples noise on every clock pulse to generate stepped random pitches.

```ron
Patch(
    version: 1,
    name: "Random Arpeggiator",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 6, "shape": 4}),
        PatchModule(id: 2, kind: "loom.osc.noise@1"),
        PatchModule(id: 3, kind: "loom.rand.sample-hold@1", params: {"mode": 0}),
        PatchModule(id: 4, kind: "loom.level.scale@1", params: {"factor": 0.1}),
        PatchModule(id: 5, kind: "loom.phase.generator@1"),
        PatchModule(id: 6, kind: "loom.osc.sine@1"),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (3, "trigger")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "out"), to: (5, "pitch")),
        PatchCable(from: (5, "phase"), to: (6, "phase")),
        PatchCable(from: (6, "out"), to: (7, "left")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Noise](loom.osc.noise.md)
