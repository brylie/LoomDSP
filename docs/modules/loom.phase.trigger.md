# Phase Trigger

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Fires a trigger each time the phase wraps, forward or backward.

| | |
|---|---|
| ID | `loom.phase.trigger` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | The phase to watch. Each wrap fires a trigger. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | unipolar | Fires once each time the phase wraps. |

## Parameters

None.

## Panel

3 by 2 grid units.

- Light: **Fired** (`fired`): Flashes each time the phase wraps.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Phase Trigger to convert phase wraps into single-sample trigger impulses. Each time the input phase wraps, forward (1 to 0) or backward (0 to 1), an output trigger is emitted. This is ideal for syncing clocks, step sequencers, and sample-and-hold circuits.

## Example patch

Fires sample-and-hold random values once per cycle of an LFO phase.

```ron
Patch(
    version: 1,
    name: "Phase Clock",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 4}),
        PatchModule(id: 2, kind: "loom.phase.trigger@1"),
        PatchModule(id: 3, kind: "loom.osc.noise@1"),
        PatchModule(id: 4, kind: "loom.rand.sample-hold@1"),
        PatchModule(id: 5, kind: "loom.phase.generator@1"),
        PatchModule(id: 6, kind: "loom.osc.sine@1"),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "trigger"), to: (4, "trigger")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "out"), to: (5, "pitch")),
        PatchCable(from: (5, "phase"), to: (6, "phase")),
        PatchCable(from: (6, "out"), to: (7, "left")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Phase and cycles](../learn/phase-and-cycles.md)
