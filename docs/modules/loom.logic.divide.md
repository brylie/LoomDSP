# Clock Divide

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Fires on every Nth trigger. Offset fires earlier in the count. Reset starts the count again.

| | |
|---|---|
| ID | `loom.logic.divide` |
| Version | 1 |
| Category | Logic |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | 0 | The triggers to count. |
| **Reset** (`reset`) | gate | 0 | Starts the count again. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | unipolar | Fires on every Nth trigger. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Division** (`n`) | 1 to 64, whole numbers | 2 | Fires on every Nth trigger. |
| **Offset** (`offset`) | 0 to 63, whole numbers | 0 | Fires this many triggers earlier in the count. |

## Panel

6 by 5 grid units.

- Knob: **Division** (`n`)
- Knob: **Offset** (`offset`)
- Light: **Fired** (`fired`): Flashes each time the output fires.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Clock Divide to generate sub-rhythms and polyrhythms from a master trigger source. It fires an output trigger once every N input triggers. An offset parameter shifts which step in the cycle fires the trigger.

## Example patch

Divides a fast clock to trigger an envelope every 4th pulse.

```ron
Patch(
    version: 1,
    name: "Clock Divider",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 8, "shape": 4}),
        PatchModule(id: 2, kind: "loom.logic.divide@1", params: {"n": 4, "offset": 0}),
        PatchModule(id: 3, kind: "loom.env.envelope@1", params: {"attack": 0.005, "decay": 0.1, "sustain": 0, "release": 0.1}),
        PatchModule(id: 4, kind: "loom.osc.noise@1"),
        PatchModule(id: 5, kind: "loom.level.amp@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "trigger")),
        PatchCable(from: (2, "trigger"), to: (3, "gate")),
        PatchCable(from: (4, "out"), to: (5, "in")),
        PatchCable(from: (3, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Phase Trigger](loom.phase.trigger.md)
