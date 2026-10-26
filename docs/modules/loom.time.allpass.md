# All-Pass

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Delay-line all-pass filter. Passes all frequencies at equal steady-state level while smearing phase, for reverbs and diffusers.

| | |
|---|---|
| ID | `loom.time.allpass` |
| Version | 1 |
| Category | Time |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to pass through the filter. |
| **Time** (`time`) | modulation | 0 | Adds to the delay time, in seconds. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The all-pass filtered signal. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Time** (`time`) | 0.0001 to 0.2 s, logarithmic | 0.01 s | Delay time of the all-pass loop. |
| **Gain** (`gain`) | -0.95 to 0.95 | 0.5 | Feedback and feed-forward coefficient. 0 is a plain delay. |

## Panel

6 by 3 grid units.

- Knob: **Time** (`time`)
- Knob: **Gain** (`gain`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use All-Pass to alter phase relationships without changing frequency balance. It forms the core building block of reverb diffusers, phasers, and dispersion filters. Cascading multiple all-pass filters diffuses transient sounds into dense reverb tails.

## Example patch

Smears the phase of a pulse tone through an all-pass filter.

```ron
Patch(
    version: 1,
    name: "Allpass Diffuser",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.pulse@1", params: {"width": 0.1}),
        PatchModule(id: 3, kind: "loom.time.allpass@1", params: {"time": 0.02, "gain": 0.6}),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "left")),
    ],
)
```

## See also

- [Delays and interpolation](../learn/delays-and-interpolation.md)
- [Delay](loom.time.delay.md)
