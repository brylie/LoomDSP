# Phase Scale

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Runs a phase at multiply / divide times its speed, locked to the input. Divide counts the input's wraps from the first cycle or the last reset.

| | |
|---|---|
| ID | `loom.phase.scale` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | The phase to scale. |
| **Reset** (`reset`) | gate | 0 | A rising edge starts counting the input's wraps again from zero. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | The scaled phase, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Multiply** (`multiply`) | 1 to 16, whole numbers | 1 | How many times faster the output runs. |
| **Divide** (`divide`) | 1 to 16, whole numbers | 1 | How many times slower the output runs: it wraps once every this many input wraps. |

## Panel

6 by 3 grid units.

- Knob: **Multiply** (`multiply`)
- Knob: **Divide** (`divide`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
