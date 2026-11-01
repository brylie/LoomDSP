# Sample and Hold

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values.

| | |
|---|---|
| ID | `loom.rand.sample-hold` |
| Version | 1 |
| Family | Modulation |
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
| **Mode** (`mode`) | Sample or Track | Sample | Sample: take the value on the rising edge. Track: follow the input while the trigger is high. |

## Panel

7 by 2 grid units.

- Segmented control: **Mode** (`mode`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Sample and Hold to take a signal's value at a moment and keep it. With Noise on the input and a clock on Trigger, it makes a new random value on each step.

In Sample mode it takes a value on each rising edge. In Track mode it follows the input while the trigger is high and holds the last value while it is low. The output starts at 0.

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
        PatchCable(from: (6, "out"), to: (7, "in")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Noise](loom.osc.noise.md)
