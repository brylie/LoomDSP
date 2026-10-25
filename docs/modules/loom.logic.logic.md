# Logic

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Combines up to four gates with And, Or, or Xor. Only inputs with a cable count. Invert gives Nand, Nor, Xnor, and Not.

| | |
|---|---|
| ID | `loom.logic.logic` |
| Version | 1 |
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
| **Operation** (`op`) | 0 to 2, whole numbers | 0 | And: all connected inputs are high. Or: any is high. Xor: an odd number are high. |
| **Invert** (`invert`) | 0 to 1, whole numbers | 0 | Flips the result. |

## Panel

6 by 4 grid units.

- Radio buttons: **Operation** (`op`) (And, Or, Xor)
- Toggle: **Invert** (`invert`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
