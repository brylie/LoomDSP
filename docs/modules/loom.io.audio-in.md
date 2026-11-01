# Audio In

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Brings a pair of channels from the selected audio input into the patch.

| | |
|---|---|
| ID | `loom.io.audio-in` |
| Version | 1 |
| Family | Interface |
| Category | I/O |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio, stereo | bipolar | The selected input pair, scaled by gain and mute. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Pair** (`pair`) | 1 to 4, whole numbers, choices 1, 2, 3, 4 | 1 | The input channel pair to read. |
| **Gain** (`gain`) | 0 to 4 | 1 | Scales the input level. |
| **Mute** (`mute`) | 0 to 1, whole numbers | 0 | Fades the input to silence. |

## Panel

12 by 6 grid units.

- Radio buttons: **Pair** (`pair`) (1, 2, 3, 4)
- Knob: **Gain** (`gain`)
- Toggle: **Mute** (`mute`)
- Light: **Active** (`active`): Lit when the selected input pair carries sound.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Audio In to bring a microphone or instrument into a patch. Pick the pair your interface uses. Use headphones or keep the gain low to avoid feedback.

## Example patch

Connects the first input pair to the audio output.

```ron
Patch(
    version: 1,
    name: "Live input",
    modules: [
        PatchModule(id: 1, kind: "loom.io.audio-in@1"),
        PatchModule(id: 2, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
    ],
)
```

## See also

- [Stereo cables](../learn/stereo-cables.md)
- [Output safety](../learn/output-safety.md)
