# Phase Reset

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Restarts a phase at zero on a trigger. After that, the phase moves at the input's speed.

| | |
|---|---|
| ID | `loom.phase.reset` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | The phase to restart. With no cable, it follows the bar. |
| **Reset** (`reset`) | gate | 0 | A rising edge makes the output 0 on that frame. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | The input phase, counted from the last reset. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use it when a part of a patch should start again at a moment you choose, such as an LFO that restarts when the chorus begins. The phase keeps its speed: only its starting point moves. For a phase with its own rate, use [Phase](loom.phase.generator.md).

With nothing on **Phase**, it follows the bar, and it holds when the transport stops.

## Example patch

A wave that starts again from zero on every beat, whatever the running phase was doing.

```ron
Patch(
    version: 1,
    name: "Restarted Wave",
    modules: [
        PatchModule(id: 1, kind: "loom.clock.transport@1"),
        PatchModule(id: 2, kind: "loom.phase.scale@1", params: {"multiply": 4}),
        PatchModule(id: 3, kind: "loom.phase.trigger@1"),
        PatchModule(id: 4, kind: "loom.phase.generator@1"),
        PatchModule(id: 5, kind: "loom.phase.reset@1"),
        PatchModule(id: 6, kind: "loom.osc.sine@1"),
        PatchModule(id: 7, kind: "loom.math.mul@1", params: {"gain": 0.3}),
        PatchModule(id: 8, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "trigger"), to: (5, "reset")),
        PatchCable(from: (4, "phase"), to: (5, "phase")),
        PatchCable(from: (5, "phase"), to: (6, "phase")),
        PatchCable(from: (6, "out"), to: (7, "a")),
        PatchCable(from: (7, "out"), to: (8, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Phase Trigger](loom.phase.trigger.md)
