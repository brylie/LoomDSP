# Multiply

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Multiplies two signals and a gain, for ring modulation. To control loudness, use Amplifier.

| | |
|---|---|
| ID | `loom.math.mul` |
| Version | 1 |
| Family | Logic and math |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | audio | 0 | The signal to scale. |
| **B** (`b`) | modulation | 0 | Scales A. Reads 1 while no cable is connected. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | A times B times the gain. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Gain** (`gain`) | -10 to 10. Control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 1 | A fixed amount the product is multiplied by. |

## Panel

3 by 3 grid units.

- Knob: **Gain** (`gain`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Multiply to multiply two signals, for example for ring modulation: two audio signals multiplied give the sum and difference of their frequencies. B reads 1 with no cable, so with one input connected, Multiply is a fixed gain.

To control loudness with an envelope, Amplifier is usually the better choice.

## Example patch

Ring-modulates two oscillators together for bell-like metallic timbres.

```ron
Patch(
    version: 1,
    name: "Ring Modulation",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1", params: {"tune": 0}),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.phase.generator@1", params: {"tune": 7}),
        PatchModule(id: 4, kind: "loom.osc.sine@1"),
        PatchModule(id: 5, kind: "loom.math.mul@1", params: {"gain": 0.5}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (3, "phase"), to: (4, "phase")),
        PatchCable(from: (2, "out"), to: (5, "a")),
        PatchCable(from: (4, "out"), to: (5, "b")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Amplifier](loom.level.amp.md)
- [Divide](loom.math.div.md)
