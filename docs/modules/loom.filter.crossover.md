# Crossover

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Splits a signal into two or three frequency bands that sum to a flat level.

| | |
|---|---|
| ID | `loom.filter.crossover` |
| Version | 1 |
| Family | Filters and effects |
| Category | Filter |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to split. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Low** (`low`) | audio | bipolar | The frequencies below the lower split. |
| **Mid** (`mid`) | audio | bipolar | The frequencies between the splits. Silent with two bands. |
| **High** (`high`) | audio | bipolar | The frequencies above the upper split. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Bands** (`bands`) | Two or Three | Three | How many bands it makes. |
| **Low split** (`low_split`) | 20 to 16000 Hz, logarithmic | 200 Hz | Where the low band ends. |
| **High split** (`high_split`) | 40 to 20000 Hz, logarithmic | 2000 Hz | Where the high band starts. It stays at or above the low split. |

## Panel

6 by 5 grid units.

- Segmented control: **Bands** (`bands`)
- Knob: **Low split** (`low_split`)
- Knob: **High split** (`high_split`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Crossover to treat low, mid, and high frequencies differently: compress the bass alone, or add delay only to the highs. Send each band through its own effect, then mix them back together.

The bands sum to a flat level: put them back together untouched and you hear the same balance of frequencies. They do not sum to the same waveform, because the phase turns around each split. So mixing the bands with the dry signal gives dips. Mix one band or the other, not both with the dry signal.

With two bands, only Low split acts, and the Mid output is silent. The splits stay in order: if High split is below Low split, both use Low split.

## Example patch

Noise split in two, with only the low and high bands sent on, so the middle is gone.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Noise Without Its Middle"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.filter.crossover@1", params: {"low_split": 300, "high_split": 3000}),
        PatchModule(id: 3, kind: "loom.mix.crossfade@1"),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "low"), to: (3, "a")),
        PatchCable(from: (2, "high"), to: (3, "b")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)
```

## See also

- [Dynamics](loom.dyn.dynamics.md)
- [EQ](loom.eq.eq.md)
