# EQ

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Shapes the balance of frequencies with two cuts, two shelves, and a mid band.

| | |
|---|---|
| ID | `loom.eq.eq` |
| Version | 1 |
| Family | Filters and effects |
| Category | EQ |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal to shape. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The shaped signal. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Low cut** (`low_cut`) | 10 to 1000 Hz, logarithmic | 40 Hz | Removes what is below this frequency. |
| **Low slope** (`low_slope`) | Off, 12, or 24 | Off | How steep the low cut is, or Off. |
| **Low freq** (`low_freq`) | 20 to 2000 Hz, logarithmic | 120 Hz | The low shelf changes what is below this frequency. |
| **Low gain** (`low_gain`) | -24 to 24 dB | 0 dB | How much the low shelf raises or lowers. |
| **Mid freq** (`mid_freq`) | 40 to 16000 Hz, logarithmic | 1000 Hz | The centre of the mid band. |
| **Mid gain** (`mid_gain`) | -24 to 24 dB | 0 dB | How much the mid band raises or lowers at its centre. |
| **Mid Q** (`mid_q`) | 0.1 to 10, logarithmic | 1 | How narrow the mid band is. |
| **High freq** (`high_freq`) | 1000 to 20000 Hz, logarithmic | 8000 Hz | The high shelf changes what is above this frequency. |
| **High gain** (`high_gain`) | -24 to 24 dB | 0 dB | How much the high shelf raises or lowers. |
| **High cut** (`high_cut`) | 1000 to 22000 Hz, logarithmic | 16000 Hz | Removes what is above this frequency. |
| **High slope** (`high_slope`) | Off, 12, or 24 | Off | How steep the high cut is, or Off. |

## Panel

15 by 16 grid units.

- Shape preview: **Response** (`out`) (read-only)
- Segmented control: **Low slope** (`low_slope`)
- Segmented control: **High slope** (`high_slope`)
- Knob: **Low cut** (`low_cut`)
- Knob: **Low freq** (`low_freq`)
- Knob: **Low gain** (`low_gain`)
- Knob: **Mid freq** (`mid_freq`)
- Knob: **Mid gain** (`mid_gain`)
- Knob: **Mid Q** (`mid_q`)
- Knob: **High freq** (`high_freq`)
- Knob: **High gain** (`high_gain`)
- Knob: **High cut** (`high_cut`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use EQ to clean up and balance a sound. The signal passes through five sections in order:

- **Low cut** removes what is below its frequency, such as rumble. It is off until you pick a slope.
- **Low shelf** raises or lowers everything below its frequency by the same amount.
- **Mid band** is a bell: a bump or dip centred on its frequency. Mid Q sets how narrow it is.
- **High shelf** raises or lowers everything above its frequency by the same amount.
- **High cut** removes what is above its frequency, such as hiss. It is off until you pick a slope.

With every gain at 0 and both cuts off, the signal passes through untouched, bit for bit. For a second mid band, put a second EQ in the chain.

## Example patch

A saw with its rumble cut, a dip in the middle, and a little more air.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "EQ on a Saw"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.eq.eq@1", params: {"low_slope": 2, "low_cut": 80, "mid_freq": 500, "mid_gain": -6, "high_gain": 4}),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)
```

## See also

- [SVF](loom.filter.svf.md)
- [Crossover](loom.filter.crossover.md)
- [Dynamics](loom.dyn.dynamics.md)
