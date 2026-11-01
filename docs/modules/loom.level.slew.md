# Slew

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Limits how fast a signal can rise and fall. Times are per 1.0 of change, so a glide takes the same time on any signal.

| | |
|---|---|
| ID | `loom.level.slew` |
| Version | 1 |
| Family | Level and routing |
| Category | Level |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to slow down. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input, moving toward it at a limited speed. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Rise** (`rise`) | 0.0005 to 10 s, logarithmic | 0.05 s | The time to go up by 1.0. |
| **Fall** (`fall`) | 0.0005 to 10 s, logarithmic | 0.05 s | The time to go down by 1.0. |

## Panel

6 by 3 grid units.

- Knob: **Rise** (`rise`)
- Knob: **Fall** (`fall`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Slew to limit the rate of change of any signal. It creates smooth portamento between pitch changes, softens sharp modulation steps, and eliminates clicks from abrupt gate transitions. Independent rise and fall times allow asymmetrical glides.

## Example patch

Adds pitch glide between incoming MIDI notes.

```ron
Patch(
    version: 1,
    name: "Portamento",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.level.slew@1", params: {"rise": 0.1, "fall": 0.1}),
        PatchModule(id: 3, kind: "loom.phase.generator@1"),
        PatchModule(id: 4, kind: "loom.osc.triangle@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "pitch")),
        PatchCable(from: (3, "phase"), to: (4, "phase")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [One-pole](loom.filter.onepole.md)
