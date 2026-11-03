# Dynamics

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Compresses, limits, expands, gates, or ducks a signal, with a sidechain that can come from another signal.

| | |
|---|---|
| ID | `loom.dyn.dynamics` |
| Version | 1 |
| Family | Filters and effects |
| Category | Dynamics |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio, stereo | 0 | The signal to process. |
| **Side** (`sidechain`) | audio | 0 | When connected, sets the level instead of the input. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio, stereo | bipolar | The processed signal. |
| **Gain** (`gain`) | modulation | unipolar | The gain from the threshold and ratio, from 0 to 1, to drive anything else. It leaves out match, output, and mix. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | Compress, Limit, Expand, Gate, or Duck | Compress | What it does about the level. |
| **Threshold** (`threshold`) | -60 to 0 dB | -18 dB | The level where it starts to act. |
| **Ratio** (`ratio`) | 1 to 20, logarithmic | 4 | How strongly it acts past the threshold. |
| **Knee** (`knee`) | 0 to 24 dB | 6 dB | How gently it starts, as a width around the threshold. |
| **Range** (`range`) | 0 to 96 dB | 40 dB | The most the gain can fall. |
| **Attack** (`attack`) | 0.0001 to 0.2 s, logarithmic | 0.01 s | How fast the gain moves when the signal rises. |
| **Release** (`release`) | 0.01 to 2 s, logarithmic | 0.15 s | How fast the gain moves when the signal falls. |
| **Detect** (`detect`) | Peak or RMS | Peak | How it reads the level: each peak, or the average power. |
| **Match** (`match`) | Off or On | Off | Turns the result to the loudness of the input, so you compare the sound and not the level. |
| **Output** (`output`) | -24 to 24 dB | 0 dB | The level after everything else. |
| **Mix** (`mix`) | 0 to 1 | 1 | 0 is the input alone, and 1 is the processed signal alone. |

## Panel

15 by 14 grid units.

- Shape preview: **Curve** (`out`) (read-only)
- Meter: **Reduction** (`reduction`): How far the gain has fallen.
- Select: **Mode** (`mode`)
- Segmented control: **Detect** (`detect`)
- Toggle: **Match** (`match`)
- Knob: **Threshold** (`threshold`)
- Knob: **Ratio** (`ratio`)
- Knob: **Knee** (`knee`)
- Knob: **Range** (`range`)
- Knob: **Attack** (`attack`)
- Knob: **Release** (`release`)
- Knob: **Output** (`output`)
- Knob: **Mix** (`mix`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Dynamics to even out a vocal, hold a peak down, clean up noise between phrases, or make room for a kick. Pick the mode first, and the knobs that do nothing in that mode hide themselves.

Watch the curve to see what the settings mean, and the meter to see how hard it works. Everything to the right of the threshold on the curve is where it compresses or limits, and everything to the left is where it expands or gates.

A cable on Side sets the level instead of the input. Duck with a kick on Side lowers a pad on each hit. The Gain output carries the same gain, from 0 to 1, so one detector can move anything else in a patch.

Attack and release always describe the signal. Attack is how fast it reacts when the signal rises, and release is how fast it reacts when the signal falls, in every mode.

Turn on Match to hear the sound of a setting without its change in level. It is slow on purpose, and it is for comparing. Turn it off when you have chosen.

Its output is always stereo, because both sides get one gain and the stereo image holds still. Limit has no lookahead, so it adds no delay, and a fast peak is turned down on its own sample, which can add some distortion.

Limit's ceiling applies before Match, Output, and Mix, so leave Match off when a ceiling must hold. The ceiling shapes the sound. It does not protect your speakers or ears: output safety does that, at the end of the patch. [What a limiter's ceiling promises](../learn/dynamics-detection.md#what-a-limiters-ceiling-promises) explains why.

## Example patch

A held saw that ducks on each note, with an envelope as the key.

```ron
Patch(
    version: 1,
    name: "Ducked Saw",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.env.envelope@1", params: {"mode": 1, "decay": 0.3}),
        PatchModule(id: 5, kind: "loom.dyn.dynamics@1", params: {"mode": 4, "threshold": -30, "range": 18, "release": 0.2}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (1, "gate"), to: (4, "gate")),
        PatchCable(from: (3, "out"), to: (5, "in")),
        PatchCable(from: (4, "out"), to: (5, "sidechain")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [How a dynamics processor hears a level](../learn/dynamics-detection.md)
- [Output safety](../learn/output-safety.md)
- [Crossover](loom.filter.crossover.md)
- [EQ](loom.eq.eq.md)
