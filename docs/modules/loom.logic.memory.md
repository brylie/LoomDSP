# Memory

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Remembers a gate. Set and reset, toggle, or data (a D latch) mode. Reset wins a tie.

| | |
|---|---|
| ID | `loom.logic.memory` |
| Version | 1 |
| Category | Logic |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | gate | 0 | In Set and reset, a rising edge turns the output on. In Toggle, a rising edge flips it. In Data, this is the level to remember. |
| **B** (`b`) | gate | 0 | A rising edge turns the output off (Set and reset, Toggle), or is the clock that remembers A (Data). |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | gate | unipolar | The remembered gate. It starts low. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 2, whole numbers, choices Set and reset, Toggle, Data | 0 | Set and reset, Toggle, or Data. Changing it keeps the state. |

## Panel

9 by 4 grid units.

- Radio buttons: **Mode** (`mode`) (Set and reset, Toggle, Data)
- Light: **State** (`state`): Lit while the remembered gate is high.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Memory as a flip-flop or digital latch. It supports Set and reset, Toggle, and Data modes. Reset takes priority when both inputs receive simultaneous triggers.

## Example patch

Toggles an audio gate on and off with each press of a button.

```ron
Patch(
    version: 1,
    name: "Toggle Memory",
    modules: [
        PatchModule(id: 1, kind: "loom.ui.button@1", params: {"mode": 1}),
        PatchModule(id: 2, kind: "loom.logic.memory@1", params: {"mode": 1}),
        PatchModule(id: 3, kind: "loom.phase.generator@1"),
        PatchModule(id: 4, kind: "loom.osc.sine@1"),
        PatchModule(id: 5, kind: "loom.level.amp@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "gate"), to: (2, "a")),
        PatchCable(from: (3, "phase"), to: (4, "phase")),
        PatchCable(from: (4, "out"), to: (5, "in")),
        PatchCable(from: (2, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "left")),
    ],
)
```

## See also

- [Button](loom.ui.button.md)
- [Triggers and gates](../learn/triggers-and-gates.md)
