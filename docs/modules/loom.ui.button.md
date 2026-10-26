# Button

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Sends a gate from a large button. Toggle keeps it. Momentary is high only while held.

| | |
|---|---|
| ID | `loom.ui.button` |
| Version | 1 |
| Category | Control |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Gate** (`gate`) | gate | unipolar | High while the button is on. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 1, whole numbers, choices Toggle, Momentary | 0 | Toggle keeps the gate. Momentary is high only while the button is held. |
| **On** (`on`) | 0 to 1, whole numbers | 0 | The gate the button keeps in Toggle mode. |
| **Hold** (`hold`) | 0 to 1, whole numbers, not saved | 0 | High while the button is held in Momentary mode. Never saved. |

## Panel

6 by 3 grid units.

- Button: **On** (`on`) (toggle), **Hold** (`hold`) (momentary), chosen by **Mode** (`mode`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
