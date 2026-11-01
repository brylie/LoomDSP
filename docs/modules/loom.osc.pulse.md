# Pulse

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A pulse wave with a set width, shaped from a phase input. Its sharp edges are smoothed so high notes stay clean.

| | |
|---|---|
| ID | `loom.osc.pulse` |
| Version | 1 |
| Family | Sources |
| Category | Oscillator |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | Position in the wave, from 0 to 1. One ramp is one cycle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Pulse wave from -1 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Width** (`width`) | 0.01 to 0.99 | 0.5 | Share of the cycle spent high, from 0.01 to 0.99. 0.5 is a square wave. |

## Panel

3 by 3 grid units.

- Knob: **Width** (`width`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Pulse for hollow and buzzy sounds. Width sets how much of each cycle is high: 0.5 is a square wave, and narrow widths sound thinner. Moving the width with an LFO gives a slowly changing tone.

It needs a phase input, usually from Phase. Its sharp edges are smoothed, so high notes do not gain harsh extra tones.

## Example patch

Plays a pulse wave driven by a phase generator.

```ron
Patch(
    version: 1,
    name: "Pulse Voice",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.pulse@1", params: {"width": 0.25}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Saw](loom.osc.saw.md)
