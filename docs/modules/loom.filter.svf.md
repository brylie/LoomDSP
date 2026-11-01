# SVF

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A resonant filter with low-pass, band-pass, high-pass, and notch outputs. The cutoff input adds pitch: +0.1 doubles the cutoff.

| | |
|---|---|
| ID | `loom.filter.svf` |
| Version | 1 |
| Family | Filters and effects |
| Category | Filter |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The audio to filter. |
| **Cutoff** (`cutoff`) | pitch | 0 | Moves the cutoff as pitch: +0.1 doubles it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Lowpass** (`lowpass`) | audio | bipolar | Keeps the frequencies below the cutoff. |
| **Bandpass** (`bandpass`) | audio | bipolar | Keeps the frequencies near the cutoff. |
| **Highpass** (`highpass`) | audio | bipolar | Keeps the frequencies above the cutoff. |
| **Notch** (`notch`) | audio | bipolar | Cuts a narrow band around the cutoff and passes the rest. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Cutoff** (`cutoff`) | 20 to 20000 Hz, logarithmic | 1000 Hz | The frequency where the filter starts to cut. |
| **Resonance** (`resonance`) | 0 to 1 | 0.2 | Boosts the frequencies near the cutoff. High values ring. |

## Panel

8 by 3 grid units.

- Knob: **Cutoff** (`cutoff`)
- Knob: **Resonance** (`resonance`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use SVF when you want a resonant filter with several outputs at once. Lowpass, Bandpass, Highpass, and Notch all follow the same cutoff and resonance, so you can blend them or switch between them by moving one cable.

It cuts 12 dB per octave. Raise the resonance to boost the frequencies near the cutoff; high values ring. The cutoff input tracks pitch at 0.1 per octave, so an envelope or the note's pitch can move it.

## Example patch

Filters a sawtooth wave with resonance, controlled by a MIDI note.

```ron
Patch(
    version: 1,
    name: "Resonant Filter",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.filter.svf@1", params: {"cutoff": 600, "resonance": 0.4}),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "lowpass"), to: (5, "in")),
    ],
)
```

## See also

- [Subtractive patch](../../examples/patches/subtractive.ron)
- [One-Pole](loom.filter.onepole.md)
