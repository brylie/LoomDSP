# Phase wrap and fold

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Brings any signal into the phase range, 0 to 1. Wrap goes round the edge; fold turns back at the edges.

| | |
|---|---|
| ID | `loom.phase.wrap` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to bring into the phase range. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | phase | unipolar | A phase from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 1, whole numbers, choices Wrap, Fold | 0 | Wrap goes round the edge. Fold turns back at the edges. |

## Panel

3 by 3 grid units.

- Radio buttons: **Mode** (`mode`) (Wrap, Fold)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
