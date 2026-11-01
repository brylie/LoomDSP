# MIDI Controller

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Turns a MIDI controller, the pitch bend, or channel aftertouch into a signal. Controllers and aftertouch go from 0 to 1, bend from -1 to 1.

| | |
|---|---|
| ID | `loom.midi.controller` |
| Version | 1 |
| Family | Interface |
| Category | MIDI |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The chosen source: 0 to 1 for a controller or aftertouch, -1 to 1 for pitch bend. 0 until a message arrives. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Source** (`source`) | Controller, Pitch bend, or Aftertouch | Controller | What to follow: a controller number, the pitch bend wheel, or channel aftertouch. |
| **Controller** (`controller`) | 0 to 127, whole numbers | 1 | The controller number to follow, from 0 to 127, when the source is Controller. |
| **Channel** (`channel`) | 0 to 16, whole numbers | 0 | The MIDI channel to listen to, from 1 to 16. 0 listens to all. |
| **Smoothing** (`smoothing`) | 0 to 0.5 s | 0.005 s | About how long the output takes to reach a new value. 0 jumps at once. |

## Panel

12 by 6 grid units.

- Radio buttons: **Source** (`source`) (Controller, Pitch bend, Aftertouch)
- Knob: **Controller** (`controller`)
- Knob: **Channel** (`channel`)
- Knob: **Smoothing** (`smoothing`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use MIDI Controller to play any part of a patch from a knob, fader, pedal, wheel, or key pressure on your MIDI controller. Pick the source on the panel. Controller follows one controller number, such as 1 for the modulation wheel or 64 for the sustain pedal, from 0 to 1. Pitch bend gives the raw wheel from -1 to 1, with the centre at 0. Aftertouch gives channel pressure from 0 to 1.

Most controllers send 128 steps, so a slow move can sound stepped. Smoothing glides between steps. Turn it to 0 for switches and pedals that should act at once. To move a knob on a module with no cable, use MIDI learn instead.

## Example patch

The modulation wheel opens a filter on a saw.

```ron
Patch(
    version: 1,
    name: "Mod Wheel Filter",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.midi.controller@1", params: {"controller": 1}),
        PatchModule(id: 5, kind: "loom.filter.ladder@1", params: {"cutoff": 200, "resonance": 0.5}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (5, "in")),
        PatchCable(from: (4, "out"), to: (5, "cutoff")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [MIDI Note](loom.midi.note.md)
- [Pitch at 0.1 per octave](../learn/pitch.md)
- [Ladder](loom.filter.ladder.md)
