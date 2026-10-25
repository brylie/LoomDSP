# One-pole

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A gentle first-order filter, 6 dB per octave. The cutoff input adds pitch: +0.1 doubles the cutoff. Low cutoffs smooth control signals.

| | |
|---|---|
| ID | `loom.filter.onepole` |
| Version | 1 |
| Category | Filter |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to filter. |
| **Cutoff** (`cutoff`) | pitch | 0 | Moves the cutoff as pitch: +0.1 doubles it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Lowpass** (`lowpass`) | audio | bipolar | Keeps the frequencies below the cutoff. |
| **Highpass** (`highpass`) | audio | bipolar | Keeps the frequencies above the cutoff. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Cutoff** (`cutoff`) | 1 to 20000 Hz, logarithmic | 1000 Hz | The frequency where the output is 3 dB down. |

## Panel

3 by 3 grid units.

- Knob: **Cutoff** (`cutoff`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
