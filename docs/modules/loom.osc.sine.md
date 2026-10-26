# Sine

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Sine wave shaped from a phase input.

| | |
|---|---|
| ID | `loom.osc.sine` |
| Version | 1 |
| Category | Oscillator |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | Position in the wave, from 0 to 1. One ramp is one cycle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Sine wave from -1 to 1. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Sine to produce a pure fundamental tone containing no harmonic overtones. It is ideal for clean sub-bass, pure test tones, and carrier or modulator signals in frequency modulation synthesis.

## Example patch

Generates a pure sine tone driven by a phase generator.

```ron
Patch(
    version: 1,
    name: "Pure Sine",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "left")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Why feedback FM turns to noise](../learn/feedback-fm.md)
- [Sine patch](../../examples/patches/sine.ron)
