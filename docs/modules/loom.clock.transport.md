# Transport

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

The patch's tempo, time signature, and play state. Every patch has exactly one.

| | |
|---|---|
| ID | `loom.clock.transport` |
| Version | 1 |
| Category | Clock |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Run** (`run`) | gate | 0 | High runs, low stops. While connected, it decides over the Run toggle. |
| **Reset** (`reset`) | gate | 0 | A rising edge returns to bar 1. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Bar** (`bar-phase`) | phase | unipolar | Rises from 0 to 1 once per bar. |
| **Beat** (`beat-phase`) | phase | unipolar | Rises from 0 to 1 once per quarter-note beat. |
| **Playing** (`playing`) | gate | unipolar | High while the transport runs. |
| **On bar** (`bar`) | gate | unipolar | Fires at the start of each bar while the transport runs. |
| **On beat** (`beat`) | gate | unipolar | Fires at the start of each beat while the transport runs. |
| **Jump** (`jump`) | gate | unipolar | Fires when the position jumps: a reset or a new time signature. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Tempo** (`tempo`) | 20 to 300 BPM | 120 BPM | Quarter-note beats per minute. |
| **Beats** (`numerator`) | 1 to 32, whole numbers | 4 | The top number of the time signature: how many beats in a bar. |
| **Note** (`denominator`) | 0 to 4, whole numbers, choices 1, 2, 4, 8, 16 | 2 | The bottom number of the time signature: the note value of one beat. |
| **Run** (`run`) | 0 to 1, whole numbers | 1 | On runs the transport, off stops it and holds the position. |
| **Reset** (`reset`) | 0 to 1, whole numbers, not saved | 0 | Returns to bar 1 when pressed. Never saved. |

## Panel

15 by 5 grid units.

- Toggle: **Run** (`run`)
- Light: **Beat** (`beat`): Flashes on each beat.
- Button: **Reset** (`reset`) (momentary)
- Knob: **Tempo** (`tempo`)
- Knob: **Beats** (`numerator`)
- Knob: **Note** (`denominator`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Every patch has one Transport, and you cannot add a second or delete it. It is where the patch's musical time is set: the tempo, the time signature, and whether time runs. Modules that follow the transport (an LFO in synced mode, Phase Scale, the Clock) read it without cables.

- **Run** starts and stops time. Stopping holds the position, so synced modules hold their phase; running continues from there. Space does the same when no panel control has focus.
- **Reset** returns to bar 1 and fires **Jump** once.
- A gate into the **run** input decides over the toggle while it is connected, and the toggle shows what it sets. A trigger into **reset** resets. Both inputs are read on a grid of 32 frames, so they take effect up to 0.7 ms later at 48 kHz, the same at any buffer size.
- A new time signature is a jump too: the bar grid moves at once, **Jump** fires, and no skipped bars fire.
- **On bar** and **On beat** fire on the same frames as a Phase Trigger on the **Bar** and **Beat** phases.

## Example patch

Starts and stops the patch from a button: patch a toggle Button into **run** and put the Button on the front panel.

```ron
Patch(
    version: 1,
    name: "Start and stop",
    modules: [
        PatchModule(id: 1, kind: "loom.clock.transport@1", params: {"tempo": 96}),
        PatchModule(id: 2, kind: "loom.ui.button@1", params: {"on": 1}),
        PatchModule(id: 3, kind: "loom.clock.clock@1", params: {"rate": 15, "width": 10}),
        PatchModule(id: 4, kind: "loom.env.adsr@1", params: {"attack": 0.001, "decay": 0.05, "sustain": 0, "release": 0.05}),
        PatchModule(id: 5, kind: "loom.osc.noise@1"),
        PatchModule(id: 6, kind: "loom.level.amp@1"),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (2, "gate"), to: (1, "run")),
        PatchCable(from: (3, "gate"), to: (4, "gate")),
        PatchCable(from: (5, "out"), to: (6, "in")),
        PatchCable(from: (4, "out"), to: (6, "level")),
        PatchCable(from: (6, "out"), to: (7, "left")),
        PatchCable(from: (6, "out"), to: (7, "right")),
    ],
)
```

## See also

- [Clock and transport](../design/clock.md)
- [Clock](loom.clock.clock.md)
- [Phase and cycles](../learn/phase-and-cycles.md)
