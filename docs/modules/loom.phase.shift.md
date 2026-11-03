# Phase Shift

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Moves a phase along its cycle, at the same speed. Put it before a Phase Trigger to fire at a chosen point.

| | |
|---|---|
| ID | `loom.phase.shift` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | The phase to move. With no cable, it follows the bar. |
| **Offset** (`offset`) | modulation | 0 | Added to the Offset knob, in cycles. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | The input phase, moved by the offset and wrapped to 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Offset** (`offset`) | -1 to 1 | 0 | How far to move the phase, in cycles. |

## Panel

3 by 3 grid units.

- Knob: **Offset** (`offset`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use it to fire at a chosen point of a cycle. A Phase Trigger fires where a phase wraps, so shifting the phase moves the wrap: a shift of -0.3 fires as the phase passes 0.3, in either direction. See [crossing a point](../learn/phase-and-cycles.md#crossing-a-point-shift-then-detect).

It also moves one part of a patch against another that shares the same phase, so two waves or two rhythms stay locked with a fixed gap between them. The speed never changes.

With nothing on **Phase**, it follows the bar, and it holds when the transport stops.

## Example patch

Two sines that share one phase, a quarter of a cycle apart, one on each side.

```ron
Patch(
    version: 1,
    name: "Quarter Cycle Apart",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.phase.shift@1", params: {"offset": 0.25}),
        PatchModule(id: 3, kind: "loom.osc.sine@1"),
        PatchModule(id: 4, kind: "loom.osc.sine@1"),
        PatchModule(id: 5, kind: "loom.mix.mixer@1"),
        PatchModule(id: 6, kind: "loom.math.mul@1", params: {"gain": 0.3}),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (1, "phase"), to: (3, "phase")),
        PatchCable(from: (2, "phase"), to: (4, "phase")),
        PatchCable(from: (3, "out"), to: (5, "in1")),
        PatchCable(from: (4, "out"), to: (5, "in2")),
        PatchCable(from: (5, "out"), to: (6, "a")),
        PatchCable(from: (6, "out"), to: (7, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Phase Trigger](loom.phase.trigger.md)
- [Phase Reverse](loom.phase.reverse.md)
