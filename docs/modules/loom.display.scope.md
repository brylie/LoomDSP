# Scope

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Shows its input over time. Passes the input through unchanged.

| | |
|---|---|
| ID | `loom.display.scope` |
| Version | 1 |
| Category | Display |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to show. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The input, unchanged. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Window** (`window`) | 0.001 to 1 s, logarithmic | 0.02 s | The time shown across the display. |
| **Auto** (`auto`) | 0 to 1, whole numbers | 1 | Triggers halfway between the signal's lowest and highest values. |
| **Level** (`level`) | -1 to 1 | 0 | The trigger level while Auto is off. |

## Panel

9 by 3 grid units.

- Knob: **Window** (`window`)
- Toggle: **Auto** (`auto`)
- Knob: **Level** (`level`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
