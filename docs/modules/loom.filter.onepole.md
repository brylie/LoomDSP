# One-Pole

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A gentle 6 dB per octave filter with low-pass and high-pass outputs; low cutoffs smooth control signals. The cutoff input adds pitch: +0.1 doubles the cutoff.

| | |
|---|---|
| ID | `loom.filter.onepole` |
| Version | 1 |
| Family | Filters and effects |
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

## When to use it

Use One-Pole for a gentle slope: soften a bright sound, take away some low end, or smooth a stepped control signal. It cuts 6 dB per octave and has no resonance, so it never rings.

Lowpass and Highpass come from the same cutoff, so together they split a signal into a low part and a high part. The cutoff input tracks pitch at 0.1 per octave.

## Example patch

Smooths white noise through a lowpass filter.

```ron
Patch(
    version: 1,
    name: "Filtered Noise",
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.filter.onepole@1", params: {"cutoff": 800}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "lowpass"), to: (3, "in")),
    ],
)
```

## See also

- [Pitch at 0.1 per octave](../learn/pitch.md)
- [SVF](loom.filter.svf.md)
