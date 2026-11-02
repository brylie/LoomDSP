# Chance

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Four lanes that pass each trigger or gate to output A with a set chance, and to output B otherwise.

| | |
|---|---|
| ID | `loom.rand.chance` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Random |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Trigger 1** (`trigger1`) | gate | 0 | Lane 1's trigger or gate. Each rising edge makes a new choice. |
| **Trigger 2** (`trigger2`) | gate | 0 | Lane 2's trigger or gate. With no cable, it follows the lane above. |
| **Trigger 3** (`trigger3`) | gate | 0 | Lane 3's trigger or gate. With no cable, it follows the lane above. |
| **Trigger 4** (`trigger4`) | gate | 0 | Lane 4's trigger or gate. With no cable, it follows the lane above. |
| **Reset** (`reset`) | gate | 0 | A rising edge restarts every lane's choices from the seed. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **A 1** (`a1`) | gate | unipolar | Carries lane 1's input when the choice was A. |
| **A 2** (`a2`) | gate | unipolar | Carries lane 2's input when the choice was A. |
| **A 3** (`a3`) | gate | unipolar | Carries lane 3's input when the choice was A. |
| **A 4** (`a4`) | gate | unipolar | Carries lane 4's input when the choice was A. |
| **B 1** (`b1`) | gate | unipolar | Carries lane 1's input when the choice was B. |
| **B 2** (`b2`) | gate | unipolar | Carries lane 2's input when the choice was B. |
| **B 3** (`b3`) | gate | unipolar | Carries lane 3's input when the choice was B. |
| **B 4** (`b4`) | gate | unipolar | Carries lane 4's input when the choice was B. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Chance 1** (`chance1`) | 0 to 1 | 0.5 | How likely lane 1 is to pick A. At 0 it never does; at 1 it always does. |
| **Chance 2** (`chance2`) | 0 to 1 | 0.5 | How likely lane 2 is to pick A. At 0 it never does; at 1 it always does. |
| **Chance 3** (`chance3`) | 0 to 1 | 0.5 | How likely lane 3 is to pick A. At 0 it never does; at 1 it always does. |
| **Chance 4** (`chance4`) | 0 to 1 | 0.5 | How likely lane 4 is to pick A. At 0 it never does; at 1 it always does. |
| **Seed** (`seed`) | 1 to 65535, whole numbers | 1 | Picks the sequence of choices. The same seed gives the same choices. |

## Panel

12 by 3 grid units.

- Knob: **Chance 1** (`chance1`)
- Knob: **Chance 2** (`chance2`)
- Knob: **Chance 3** (`chance3`)
- Knob: **Chance 4** (`chance4`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use only **A** for a probability gate: at a chance of 0.75, about three of every four hits pass. Use **A** and **B** together to split one rhythm into two parts that never play at once.

A trigger stays a trigger and a gate keeps its length, so Chance works on clocks and on note gates alike. One clock into lane 1 feeds all four lanes, and each lane still chooses for itself.

Put a bar clock on **Reset** to make a random pattern repeat. The same seed always gives the same choices.

## Example patch

A clock with some hits taken out, playing a short envelope on a sine wave.

```ron
Patch(
    version: 1,
    name: "Sometimes",
    modules: [
        PatchModule(id: 1, kind: "loom.clock.transport@1"),
        PatchModule(id: 2, kind: "loom.phase.scale@1", params: {"multiply": 8}),
        PatchModule(id: 3, kind: "loom.phase.trigger@1"),
        PatchModule(id: 4, kind: "loom.rand.chance@1", params: {"chance1": 0.75}),
        PatchModule(id: 5, kind: "loom.env.envelope@1"),
        PatchModule(id: 6, kind: "loom.phase.generator@1"),
        PatchModule(id: 7, kind: "loom.osc.sine@1"),
        PatchModule(id: 8, kind: "loom.math.mul@1", params: {"gain": 0.3}),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "trigger"), to: (4, "trigger1")),
        PatchCable(from: (4, "a1"), to: (5, "gate")),
        PatchCable(from: (6, "phase"), to: (7, "phase")),
        PatchCable(from: (7, "out"), to: (8, "a")),
        PatchCable(from: (5, "out"), to: (8, "b")),
        PatchCable(from: (8, "out"), to: (9, "in")),
    ],
)
```

## See also

- [Repeatable randomness](../learn/repeatable-randomness.md)
- [Triggers and gates](../learn/triggers-and-gates.md)
- [Random](loom.rand.random.md)
