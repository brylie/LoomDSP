# Euclidean Rhythm

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Spreads a number of hits as evenly as possible over a number of steps, driven by a phase or a clock.

| | |
|---|---|
| ID | `loom.gate.euclid` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Gate |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | bar phase | One cycle plays the whole pattern, in Phase mode. With no cable, it follows the bar. |
| **Clock** (`clock`) | gate | 0 | Each rising edge moves one step, in Clock mode. |
| **Reset** (`reset`) | gate | 0 | A rising edge makes the next clock play the first step, in Clock mode. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | unipolar | Fires once on entering each hit. |
| **Gate** (`gate`) | gate | unipolar | High for the whole of each hit step. |
| **Accent** (`accent`) | gate | unipolar | Fires once on the pattern's first hit. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Source** (`source`) | Phase or Clock | Phase | Phase: the step comes from the phase. Clock: each clock moves one step. |
| **Steps** (`steps`) | 1 to 32, whole numbers | 16 | How many steps the pattern has. |
| **Hits** (`hits`) | 0 to 32, whole numbers | 4 | How many of the steps are hits. Above the steps, every step is one. |
| **Rotation** (`rotation`) | 0 to 31, whole numbers | 0 | How many steps later the pattern plays. |

## Panel

15 by 4 grid units.

- Segmented control: **Source** (`source`)
- Number: **Steps** (`steps`)
- Number: **Hits** (`hits`)
- Number: **Rotation** (`rotation`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use it for rhythms that feel musical with very little setting up. Choose how many steps and how many hits, and it spreads the hits as evenly as it can: 3 in 8 is `x..x..x.`. Then turn **Rotation** to move the pattern against the others. Two or three of these at different settings make a full, interlocking groove.

In **Phase** mode the pattern follows a phase, so with no cable it fills one bar and holds when the transport stops. In **Clock** mode each clock moves one step. The module waits for its first clock, and **Reset** returns it to that wait, so the next clock always plays the first step.

**Trigger** fires on each hit, **Gate** is high for the whole of each hit step, and **Accent** fires on the first hit of the pattern. A change of the knobs is heard at the next step.

## Example patch

A short noise hit on 3 of 8 steps in each bar.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Three in Eight"),
    modules: [
        PatchModule(id: 1, kind: "loom.clock.transport@1"),
        PatchModule(id: 2, kind: "loom.gate.euclid@1", params: {"steps": 8, "hits": 3}),
        PatchModule(id: 3, kind: "loom.env.envelope@1", params: {"mode": 1, "attack": 0.001, "decay": 0.08}),
        PatchModule(id: 4, kind: "loom.osc.noise@1"),
        PatchModule(id: 5, kind: "loom.math.mul@1", params: {"gain": 0.3}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (2, "trigger"), to: (3, "gate")),
        PatchCable(from: (4, "out"), to: (5, "a")),
        PatchCable(from: (3, "out"), to: (5, "b")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Euclidean rhythms](../learn/euclidean-rhythms.md)
- [Triggers and gates](../learn/triggers-and-gates.md)
- [Phase Scale](loom.phase.scale.md)
