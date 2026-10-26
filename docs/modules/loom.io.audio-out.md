# Audio Out

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Sends audio out. A cable on only one side plays on both sides.

| | |
|---|---|
| ID | `loom.io.audio-out` |
| Version | 1 |
| Category | I/O |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Left** (`left`) | audio | 0 | The left channel. Plays on both sides if Right has no cable. |
| **Right** (`right`) | audio | 0 | The right channel. Plays on both sides if Left has no cable. |

## Outputs

None.

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Audio Out as the terminal sink for audio leaving your patch. It connects directly to your sound card, audio interface, or file renderer. Connecting only one channel automatically duplicates it to both sides.

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
        PatchCable(from: (2, "out"), to: (3, "left")),
    ],
)
```

## See also

- [Stereo cables](../learn/stereo-cables.md)
- [Output safety](../learn/output-safety.md)
- [Stopping a stream without a click](../learn/stopping-a-stream.md)
