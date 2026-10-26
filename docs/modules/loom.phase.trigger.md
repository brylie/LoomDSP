# Phase Trigger

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Fires a trigger each time the phase wraps, forward or backward.

| | |
|---|---|
| ID | `loom.phase.trigger` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | The phase to watch. Each wrap fires a trigger. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Trigger** (`trigger`) | gate | unipolar | Fires once each time the phase wraps. |

## Parameters

None.

## Panel

3 by 2 grid units.

- Light: **Fired** (`fired`): Flashes each time the phase wraps.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
