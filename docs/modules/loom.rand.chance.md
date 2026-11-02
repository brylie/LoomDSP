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
