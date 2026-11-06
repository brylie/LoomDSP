# Phase Reverse

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Runs a phase backward, at the same speed. An input of 0 stays 0, so the cycles still line up.

| | |
|---|---|
| ID | `loom.phase.reverse` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | The phase to reverse. With no cable, it follows the bar. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | The input phase, running from 1 down to 0. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use it to run anything that reads a phase backward: a saw that falls instead of rising, or a pattern played from its end. The speed never changes.

An input of 0 gives 0, not 1, so a reversed phase still starts its cycle in the same place, and a Phase Trigger after it fires on the first frame as it does on the forward phase.

With nothing on **Phase**, it follows the bar, and it holds when the transport stops.

## Example patch

A saw read backward, so its ramp falls instead of rising.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Backward Saw"),
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.phase.reverse@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.3}),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "a")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Phase Shift](loom.phase.shift.md)
