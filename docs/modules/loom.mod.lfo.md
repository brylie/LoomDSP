# LFO

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A slow wave. Free mode runs at a rate in Hz; synced mode follows a phase, by default the bar phase.

| | |
|---|---|
| ID | `loom.mod.lfo` |
| Version | 1 |
| Category | Modulation |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | In synced mode, the phase to follow. Follows the bar phase while no cable is connected. Free mode ignores it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The wave, from -1 to 1. |
| **Out +90°** (`out90`) | modulation | bipolar | The same wave a quarter cycle ahead. With Out on an XY scope, a sine draws a circle. |
| **Phase** (`phase`) | phase | unipolar | The phase, from 0 to 1, that the wave is using. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Sync** (`sync`) | 0 to 1, whole numbers, choices Free, Synced | 0 | Free: run at the rate. Synced: follow the phase input. |
| **Rate** (`rate`) | 0.01 to 50 Hz, logarithmic | 1 Hz | How many waves per second in free mode. |
| **Cycles** (`cycles`) | 1 to 16, whole numbers | 1 | How many waves fit in one cycle of the phase input in synced mode. |
| **Shape** (`shape`) | 0 to 4, whole numbers, choices Sine, Triangle, Ramp up, Ramp down, Square | 0 | The wave: sine, triangle, ramp up, ramp down, or square. |

## Panel

15 by 11 grid units.

- Shape preview: **Wave** (`out`) (read-only)
- Radio buttons: **Sync** (`sync`) (Free, Synced)
- Radio buttons: **Shape** (`shape`) (Sine, Triangle, Ramp up, Ramp down, Square)
- Knob: **Rate** (`rate`)
- Knob: **Cycles** (`cycles`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use LFO to produce cyclical modulation signals for vibrato, tremolo, filter sweeps, or pulse width modulation. Free mode runs at a specified frequency in Hertz, while Synced mode locks to bar phase or an external phase ramp.

## Example patch

Applies slow sine modulation to filter cutoff.

```ron
Patch(
    version: 1,
    name: "Filter LFO",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"sync": 0, "rate": 0.5, "shape": 0}),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.filter.svf@1", params: {"cutoff": 1200, "resonance": 0.3}),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (4, "cutoff")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "lowpass"), to: (5, "in")),
    ],
)
```

## See also

- [Modulation without cables](../learn/modulation-without-cables.md)
- [Modulation patch](../../examples/patches/modulation.ron)
