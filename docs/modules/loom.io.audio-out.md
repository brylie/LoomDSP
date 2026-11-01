# Audio Out

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Sends a mono or stereo signal to the audio output.

| | |
|---|---|
| ID | `loom.io.audio-out` |
| Version | 1 |
| Family | Interface |
| Category | I/O |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio, stereo | 0 | The signal to play. A mono cable plays on both sides. |

## Outputs

None.

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Audio Out as the terminal sink for audio leaving your patch. It connects directly to your sound card, audio interface, or file renderer. A mono cable plays on both sides. A stereo cable keeps its two channels.

## Example patch

Connects a sine oscillator directly to the stereo output.

```ron
Patch(
    version: 1,
    name: "Direct Output",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)
```

## See also

- [Stereo cables](../learn/stereo-cables.md)
- [Output safety](../learn/output-safety.md)
- [Stopping a stream without a click](../learn/stopping-a-stream.md)
