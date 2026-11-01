# Logic

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Combines up to four gates with And, Or, or Xor; only inputs with a cable count. Invert gives Nand, Nor, Xnor, and Not.

| | |
|---|---|
| ID | `loom.logic.logic` |
| Version | 1 |
| Family | Logic and math |
| Category | Logic |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In 1** (`in1`) | gate | 0 | A gate to combine. An input with no cable is ignored, not counted as low. |
| **In 2** (`in2`) | gate | 0 | A gate to combine. An input with no cable is ignored, not counted as low. |
| **In 3** (`in3`) | gate | 0 | A gate to combine. An input with no cable is ignored, not counted as low. |
| **In 4** (`in4`) | gate | 0 | A gate to combine. An input with no cable is ignored, not counted as low. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | gate | unipolar | The result: exactly 0 or 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Operation** (`op`) | And, Or, or Xor | And | And: all connected inputs are high. Or: any is high. Xor: an odd number are high. |
| **Invert** (`invert`) | 0 to 1, whole numbers | 0 | Flips the result. |

## Panel

9 by 4 grid units.

- Radio buttons: **Operation** (`op`) (And, Or, Xor)
- Toggle: **Invert** (`invert`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Logic to combine gates into one: And is high when every connected input is high, Or when any is, and Xor when an odd number are. Invert flips the result, which gives Nand, Nor, and Xnor, and Not with one input.

An input with no cable is left out, not counted as low, so you can use as many inputs as you need.

## Example patch

Combines two gate signals with an And operation to trigger an envelope only when both gates are active.

```ron
Patch(
    version: 1,
    name: "Coincidence Gate",
    modules: [
        PatchModule(id: 1, kind: "loom.ui.button@1", params: {"mode": 0, "on": 1}),
        PatchModule(id: 2, kind: "loom.midi.note@1"),
        PatchModule(id: 3, kind: "loom.logic.logic@1", params: {"op": 0, "invert": 0}),
        PatchModule(id: 4, kind: "loom.env.envelope@1"),
        PatchModule(id: 5, kind: "loom.phase.generator@1"),
        PatchModule(id: 6, kind: "loom.osc.sine@1"),
        PatchModule(id: 7, kind: "loom.level.amp@1"),
        PatchModule(id: 8, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "gate"), to: (3, "in1")),
        PatchCable(from: (2, "gate"), to: (3, "in2")),
        PatchCable(from: (3, "out"), to: (4, "gate")),
        PatchCable(from: (2, "pitch"), to: (5, "pitch")),
        PatchCable(from: (5, "phase"), to: (6, "phase")),
        PatchCable(from: (6, "out"), to: (7, "in")),
        PatchCable(from: (4, "out"), to: (7, "level")),
        PatchCable(from: (7, "out"), to: (8, "in")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Memory](loom.logic.memory.md)
