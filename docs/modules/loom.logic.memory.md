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
| **Mode** (`mode`) | 0 to 2, whole numbers | 0 | Set and reset, Toggle, or Data. Changing it keeps the state. |

## Panel

6 by 4 grid units.

- Radio buttons: **Mode** (`mode`) (Set and reset, Toggle, Data)
- Light: `state`

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
