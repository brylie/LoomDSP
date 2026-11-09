# Selector

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Picks one of up to eight inputs: the next on a trigger, the one a value points at, or one at random.

| | |
|---|---|
| ID | `loom.route.selector` |
| Version | 1 |
| Family | Level and routing |
| Category | Routing |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In 1** (`in1`) | audio | 0 | A signal to pick. |
| **In 2** (`in2`) | audio | 0 | A signal to pick. |
| **In 3** (`in3`) | audio | 0 | A signal to pick. |
| **In 4** (`in4`) | audio | 0 | A signal to pick. |
| **In 5** (`in5`) | audio | 0 | A signal to pick. |
| **In 6** (`in6`) | audio | 0 | A signal to pick. |
| **In 7** (`in7`) | audio | 0 | A signal to pick. |
| **In 8** (`in8`) | audio | 0 | A signal to pick. |
| **Step** (`step`) | gate | 0 | A rising edge moves to the next input, in Step mode, or picks one, in Random mode. |
| **Select** (`select`) | modulation | 0 | Which input to pick, in Select mode: 0 is the first, and 1 is the last in use. |
| **Reset** (`reset`) | gate | 0 | A rising edge goes to input 1 and restarts the random picks from the seed. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The picked input, faded from the last one. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | Step, Select, or Random | Step | Step: each trigger moves to the next input. Select: a value picks it. Random: each trigger picks one. |
| **Count** (`count`) | 2 to 8, whole numbers | 4 | How many inputs are in use. |
| **Fade** (`fade`) | 0 to 1 s | 0.005 s | The time a switch takes, in a straight line. At 0 it is instant, which clicks on audio. |
| **Repeats** (`repeats`) | Avoid or Allow | Avoid | Avoid: a random pick is never the current input. Allow: it can be. |
| **Seed** (`seed`) | 1 to 65535, whole numbers | 1 | Picks the sequence of random picks. The same seed gives the same picks. |

## Panel

12 by 5 grid units.

- Segmented control: **Mode** (`mode`)
- Knob: **Count** (`count`)
- Knob: **Fade** (`fade`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use it to choose between signals while a patch plays. **Step** moves to the next input on each trigger, and the first step after a start or a reset stays on input 1, so a bar clock on **Reset** starts each bar there. **Select** picks by a value, so an LFO or a knob sweeps through the inputs. **Random** picks one on each trigger, and the same seed gives the same picks each time the patch loads.

**Fade** keeps a switch from clicking on audio. Set it to 0 for gates, triggers, and pitches, where a fade would make false values between the inputs.

On a stereo cable, both sides make the same picks, so the signal stays a matched pair. The exception is a stereo modulation source assigned to one of its parameters.

## Example patch

Three pitches of one oscillator, stepped by the beat. The pitches stay exact, because **Fade** is 0.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Three Note Run"),
    modules: [
        PatchModule(id: 1, kind: "loom.clock.transport@1"),
        PatchModule(id: 2, kind: "loom.pitch.constant@1", params: {"semitones": 0}),
        PatchModule(id: 3, kind: "loom.pitch.constant@1", params: {"semitones": 3}),
        PatchModule(id: 4, kind: "loom.pitch.constant@1", params: {"semitones": 7}),
        PatchModule(id: 5, kind: "loom.route.selector@1", params: {"count": 3, "fade": 0.0}),
        PatchModule(id: 6, kind: "loom.osc.saw@1"),
        PatchModule(id: 7, kind: "loom.math.mul@1", params: {"gain": 0.2}),
        PatchModule(id: 8, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "beat"), to: (5, "step")),
        PatchCable(from: (2, "out"), to: (5, "in1")),
        PatchCable(from: (3, "out"), to: (5, "in2")),
        PatchCable(from: (4, "out"), to: (5, "in3")),
        PatchCable(from: (5, "out"), to: (6, "pitch")),
        PatchCable(from: (6, "out"), to: (7, "a")),
        PatchCable(from: (7, "out"), to: (8, "in")),
    ],
)
```

## See also

- [Repeatable randomness](../learn/repeatable-randomness.md)
- [Stereo cables](../learn/stereo-cables.md)
- [Crossfade](loom.mix.crossfade.md)
