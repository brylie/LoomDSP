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

9 by 6 grid units.

- Radio buttons: **Sync** (`sync`) (Free, Synced)
- Radio buttons: **Shape** (`shape`) (Sine, Triangle, Ramp up, Ramp down, Square)
- Knob: **Rate** (`rate`)
- Knob: **Cycles** (`cycles`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
