# Clock

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A phase, trigger, and gate at a musical rate, locked to the transport's bar 1.

| | |
|---|---|
| ID | `loom.clock.clock` |
| Version | 1 |
| Category | Clock |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | Rises from 0 to 1 once per cycle of the rate, counted from bar 1. |
| **Trigger** (`trigger`) | gate | unipolar | Fires once at the start of each cycle while the transport runs. |
| **Gate** (`gate`) | gate | unipolar | High for the first part of each cycle, as long as the gate width says. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Rate** (`rate`) | 0 to 23, whole numbers, choices 4 bars, 4 bars dotted, 4 bars triplet, 2 bars, 2 bars dotted, 2 bars triplet, 1 bar, 1 bar dotted, 1 bar triplet, 1/2, 1/2 dotted, 1/2 triplet, 1/4, 1/4 dotted, 1/4 triplet, 1/8, 1/8 dotted, 1/8 triplet, 1/16, 1/16 dotted, 1/16 triplet, 1/32, 1/32 dotted, 1/32 triplet | 12 | The length of one cycle: 4 bars to 1/32, straight, dotted, or triplet. |
| **Width** (`width`) | 1 to 99 % | 50 % | How much of each cycle the gate is high. |

## Panel

6 by 5 grid units.

- Knob: **Rate** (`rate`)
- Knob: **Width** (`width`)
- Light: **Fired** (`fired`): Flashes on each trigger.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Clock when something should happen at a musical rate: a beat for an envelope, a step for a sequencer, a gate for a hi-hat. It needs no cables in. It reads the patch transport, so it follows the tempo and stays in line with bar 1.

- **Phase** is the cycle itself. Patch it into a phase-driven module, such as an LFO in synced mode, to move it in time.
- **Trigger** fires one sample at the start of each cycle. A cycle that starts on bar 1 fires too.
- **Gate** is high for the first part of each cycle. A width of 50 % gives an even on and off; a short width gives a short pulse.

Bar rates follow the time signature: in 7/8, 1 bar is 3.5 beats. Note rates do not: 1/4 is always one quarter note. Dotted is one and a half times as long, triplet two thirds as long.

While the transport is stopped, the phase and the gate hold, and the trigger does not fire. A reset to bar 1 fires once. Turning the rate moves the phase at once, but never fires a trigger by itself.

## Example patch

Plays a short noise burst on every eighth note.

```ron
Patch(
    version: 1,
    name: "Eighth Notes",
    modules: [
        PatchModule(id: 1, kind: "loom.clock.clock@1", params: {"rate": 15, "width": 10}),
        PatchModule(id: 2, kind: "loom.env.envelope@1", params: {"attack": 0.001, "decay": 0.05, "sustain": 0, "release": 0.05}),
        PatchModule(id: 3, kind: "loom.osc.noise@1"),
        PatchModule(id: 4, kind: "loom.level.amp@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "gate"), to: (2, "gate")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (2, "out"), to: (4, "level")),
        PatchCable(from: (4, "out"), to: (5, "left")),
        PatchCable(from: (4, "out"), to: (5, "right")),
    ],
)
```

## See also

- [Clock and transport](../design/clock.md)
- [Phase and cycles](../learn/phase-and-cycles.md)
- [Triggers and gates](../learn/triggers-and-gates.md)
- [Phase Trigger](loom.phase.trigger.md)
- [Clock Divide](loom.logic.divide.md)
