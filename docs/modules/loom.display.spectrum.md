# Spectrum

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Shows the frequency content of its input. Passes the input through unchanged.

| | |
|---|---|
| ID | `loom.display.spectrum` |
| Version | 1 |
| Family | Interface |
| Category | Display |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to show. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The input, unchanged. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Tilt** (`tilt`) | Flat or +4.5 dB per octave | Flat | Lifts the high frequencies on the display, so a full mix reads closer to level. |
| **Hold** (`hold`) | Off or On | Off | Keeps a line at each recent peak for two seconds. |

## Panel

9 by 5 grid units.

- Spectrum: **Out** (`out`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Spectrum to see which frequencies a signal holds: where a filter cuts, how bright an oscillator is, or which part of a mix is too loud.

Across is frequency, from 20 Hz to 20 kHz, on a scale where each octave has the same width. Up is level, from -90 dB to 0 dB. A full-scale sine peaks at 0 dB. Turn Tilt on to make a full mix read closer to level across the range, and Hold to keep a line at each recent peak for two seconds. The input passes through unchanged. Both settings are in the inspector, and neither changes the sound.

On a stereo cable it shows the mean of the two sides. A full-scale sine on one side only reads -6 dB, and sides in opposite phase cancel, so they read nothing.

## Example patch

Shows a saw wave's harmonics, with the filter cutting the highest ones.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Spectrum of a Saw"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.saw@1"),
        PatchModule(id: 2, kind: "loom.filter.svf@1"),
        PatchModule(id: 3, kind: "loom.display.spectrum@1", params: {"hold": 1}),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "lowpass"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)
```

## See also

- [Reading a spectrum](../learn/reading-a-spectrum.md)
- [Scope](loom.display.scope.md)
- [Level Meter](loom.display.meter.md)
