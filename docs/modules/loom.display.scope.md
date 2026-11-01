# Scope

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Shows its input over time. Passes the input through unchanged.

| | |
|---|---|
| ID | `loom.display.scope` |
| Version | 1 |
| Family | Interface |
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

9 by 8 grid units.

- Knob: **Window** (`window`)
- Toggle: **Auto** (`auto`)
- Knob: **Level** (`level`)
- Scope: **Out** (`out`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Scope to see a signal's shape over time: an oscillator's wave, an envelope's rise and fall, or how far a modulation moves.

Window sets how much time the display shows. With Auto on, the trace starts where the signal crosses halfway between its lowest and highest values, so a steady wave stands still. Turn Auto off to set that level yourself. The input passes through unchanged.

## Example patch

Displays an LFO triangle wave before it modulates a filter cutoff.

```ron
Patch(
    version: 1,
    name: "Scope Waveform",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"shape": 1}),
        PatchModule(id: 2, kind: "loom.display.scope@1"),
        PatchModule(id: 3, kind: "loom.phase.generator@1"),
        PatchModule(id: 4, kind: "loom.osc.saw@1"),
        PatchModule(id: 5, kind: "loom.filter.svf@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (5, "cutoff")),
        PatchCable(from: (3, "phase"), to: (4, "phase")),
        PatchCable(from: (4, "out"), to: (5, "in")),
        PatchCable(from: (5, "lowpass"), to: (6, "in")),
    ],
)
```

## See also

- [Modulation without cables](../learn/modulation-without-cables.md)
- [Level Meter](loom.display.meter.md)
