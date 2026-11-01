# Noise

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

White, pink, or brown noise. The seed makes every render the same.

| | |
|---|---|
| ID | `loom.osc.noise` |
| Version | 1 |
| Family | Sources |
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
| **Colour** (`colour`) | White, Pink, or Brown | White | White falls flat across the spectrum, pink falls 3 dB per octave, and brown falls 6 dB per octave. |

## Panel

6 by 4 grid units.

- Radio buttons: **Colour** (`colour`) (White, Pink, Brown)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Noise for sounds without a pitch, such as wind, breath, or the body of a snare, or as a random source for Sample and Hold.

White noise is flat across the spectrum, pink falls 3 dB per octave, and brown falls 6 dB per octave, so each sounds darker than the last. The seed picks the pattern: the same seed gives the same noise in every render.

## Example patch

Filters pink noise with a resonant filter to create wind textures.

```ron
Patch(
    version: 1,
    name: "Wind Noise",
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1", params: {"colour": 1, "seed": 42}),
        PatchModule(id: 2, kind: "loom.filter.svf@1", params: {"cutoff": 500, "resonance": 0.6}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "bandpass"), to: (3, "in")),
    ],
)
```

## See also

- [Sample and Hold](loom.rand.sample-hold.md)
- [SVF](loom.filter.svf.md)
