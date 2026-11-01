# Button

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Sends a gate from a large button. Toggle keeps it on; Momentary is high only while held.

| | |
|---|---|
| ID | `loom.ui.button` |
| Version | 1 |
| Family | Interface |
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
| **Mode** (`mode`) | Toggle or Momentary | Toggle | Toggle keeps the gate. Momentary is high only while the button is held. |
| **On** (`on`) | 0 to 1, whole numbers | 0 | The gate the button keeps in Toggle mode. |
| **Hold** (`hold`) | 0 to 1, whole numbers, not saved | 0 | High while the button is held in Momentary mode. Never saved. |

## Panel

6 by 3 grid units.

- Button: **On** (`on`) (toggle), **Hold** (`hold`) (momentary), chosen by **Mode** (`mode`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Button to send a gate by hand, for example to start an envelope or reset a sequence while you play.

In Toggle mode each click turns the gate on or off. In Momentary mode the gate is high only while you hold the button.

## Example patch

Controls an audio mute or envelope gate with a toggle button.

```ron
Patch(
    version: 1,
    name: "Manual Gate Button",
    modules: [
        PatchModule(id: 1, kind: "loom.ui.button@1", params: {"mode": 0, "on": 1}),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.sine@1"),
        PatchModule(id: 4, kind: "loom.level.amp@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "gate"), to: (4, "level")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Memory](loom.logic.memory.md)
