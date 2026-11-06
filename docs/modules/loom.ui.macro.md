# Macro

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

One knob whose value is a signal. Assign it to many parameters, so one knob shapes a whole sound.

| | |
|---|---|
| ID | `loom.ui.macro` |
| Version | 1 |
| Family | Interface |
| Category | Control |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | unipolar | The knob's value, from 0 to 1, smoothed so it never steps. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Value** (`value`) | 0 to 1 | 0 | The value on the output. |

## Panel

3 by 3 grid units.

- Knob: **Value** (`value`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use it when one gesture should shape a whole sound. Add a Macro, then assign its output to as many parameters as you like, each with its own depth. Turning the one knob then moves them all together, and it is the knob to map to a MIDI controller when you perform.

The output is the knob's value, smoothed over 5 ms so a fast turn never clicks. There is no polarity setting, because each assignment already sets its own.

## Example patch

One knob that opens the filter and raises the drive together. Turn **Value** up.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "One Knob Filter"),
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.filter.ladder@1", params: {"cutoff": 300.0}),
        PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.2}),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
        PatchModule(id: 6, kind: "loom.ui.macro@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "a")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
    modulation: [
        PatchModulation(source: (6, "out"), target: (3, "cutoff"), depth: 0.4),
        PatchModulation(source: (6, "out"), target: (3, "drive"), depth: 0.6),
    ],
)
```

## See also

- [Modulation without cables](../learn/modulation-without-cables.md)
- [Button](loom.ui.button.md)
