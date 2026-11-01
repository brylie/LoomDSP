# Level Meter

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Shows the peak and RMS level of its input. Passes the input through unchanged.

| | |
|---|---|
| ID | `loom.display.meter` |
| Version | 1 |
| Family | Interface |
| Category | Display |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to measure. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The input, unchanged. |

## Parameters

None.

## Panel

3 by 6 grid units.

- Level meter: **Out** (`out`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Level Meter to watch how loud a signal is at any point in a patch. It shows the peak and the RMS level, so you can see when a signal gets near the top of its range before the output stage has to limit it.

The input passes through unchanged, so you can put the meter in the middle of a chain.

## Example patch

Measures the output of an oscillator before sending it to the main audio output.

```ron
Patch(
    version: 1,
    name: "Metered Sine",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.display.meter@1"),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)
```

## See also

- [Output safety](../learn/output-safety.md)
- [Scope](loom.display.scope.md)
