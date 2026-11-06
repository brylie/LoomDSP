# Random

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A new random value on each trigger, within a range, or a random walk. The seed makes it repeatable.

| | |
|---|---|
| ID | `loom.rand.random` |
| Version | 1 |
| Family | Modulation |
| Category | Random |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | 0 | A rising edge makes a new value on both outputs. |
| **Reset** (`reset`) | gate | 0 | A rising edge restarts the values from the seed. A walk also goes back to the middle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A random value. It starts in the middle of the range. |
| **Out 2** (`out2`) | modulation | bipolar | A second random value, independent of the first. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | Step or Walk | Step | Step: each trigger picks a new value. Walk: each trigger moves a little from the last value. |
| **Low** (`low`) | -10 to 10. Control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 0 | One end of the range. |
| **High** (`high`) | -10 to 10. Control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 1 | The other end of the range. |
| **Shape** (`shape`) | Uniform or Centred | Uniform | Uniform: every value is equally likely. Centred: values near the middle are more likely. |
| **Step** (`step`) | 0 to 1 | 0.1 | The largest move of a walk, as a fraction of the range. |
| **Glide** (`glide`) | 0 to 5 s | 0 s | The time each output takes to reach a new value, in a straight line. At 0 it steps. |
| **Seed** (`seed`) | 1 to 65535, whole numbers | 1 | Picks the sequence of values. The same seed gives the same values. |

## Panel

12 by 7 grid units.

- Segmented control: **Mode** (`mode`)
- Segmented control: **Shape** (`shape`)
- Range: **Low** (`low`) to **High** (`high`)
- Knob: **Step** (`step`)
- Knob: **Glide** (`glide`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

**Step** picks a new value on each trigger, for a part that jumps. **Walk** moves a little from the last value, for a part that wanders. **Step** sets the largest move of a walk, as a fraction of the range.

**Out** and **Out 2** are unrelated, so one clock can drive a pitch and a level that do not follow each other. To send one value to several places, drag several cables from one output.

Turn up **Glide** to slide to each new value instead of jumping. Put a bar clock on **Reset** to make a phrase repeat. In Walk mode a reset also returns to the middle at once, so the same walk replays.

## Example patch

A pitch that wanders on each beat, from a walk with a short glide.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Wandering Tone"),
    modules: [
        PatchModule(id: 1, kind: "loom.clock.transport@1"),
        PatchModule(id: 2, kind: "loom.phase.scale@1", params: {"multiply": 4}),
        PatchModule(id: 3, kind: "loom.phase.trigger@1"),
        PatchModule(id: 4, kind: "loom.rand.random@1", params: {"mode": 1, "low": -0.2, "high": 0.2, "step": 0.3, "glide": 0.05}),
        PatchModule(id: 5, kind: "loom.phase.generator@1"),
        PatchModule(id: 6, kind: "loom.osc.sine@1"),
        PatchModule(id: 7, kind: "loom.math.mul@1", params: {"gain": 0.3}),
        PatchModule(id: 8, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "trigger"), to: (4, "trigger")),
        PatchCable(from: (4, "out"), to: (5, "pitch")),
        PatchCable(from: (5, "phase"), to: (6, "phase")),
        PatchCable(from: (6, "out"), to: (7, "a")),
        PatchCable(from: (7, "out"), to: (8, "in")),
    ],
)
```

## See also

- [Repeatable randomness](../learn/repeatable-randomness.md)
- [Chance](loom.rand.chance.md)
- [Sample and Hold](loom.rand.sample-hold.md)
