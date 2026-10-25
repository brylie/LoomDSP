# Noise

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

White, pink, or brown noise. The seed makes renders repeatable (NFR-SND-04).

| | |
|---|---|
| ID | `loom.osc.noise` |
| Version | 1 |
| Category | Oscillator |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Noise with the same loudness in every colour. White stays within -1 to 1; pink and brown can peak a little above. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Seed** (`seed`) | 1 to 65535, whole numbers | 1 | Picks the noise pattern. The same seed gives the same noise. |
| **Colour** (`colour`) | 0 to 2, whole numbers | 0 | White falls flat across the spectrum, pink falls 3 dB per octave, and brown falls 6 dB per octave. |

## Panel

3 by 4 grid units.

- Radio buttons: **Colour** (`colour`) (White, Pink, Brown)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
