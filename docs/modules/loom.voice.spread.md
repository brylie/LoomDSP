# Voice Spread

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Gives each voice its own offset from one control. On a one-voice cable it does nothing.

| | |
|---|---|
| ID | `loom.voice.spread` |
| Version | 1 |
| Family | Level and routing |
| Category | Voice |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to offset. Each voice gets its own offset. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The input plus this voice's offset times the amount. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Amount** (`amount`) | -10 to 10. Control ranges -0.01 to 0.01, -0.1 to 0.1, -1 to 1, -10 to 10 (default -0.1 to 0.1) | 0 | How far apart the voices are. Each voice adds its offset times this amount. |
| **Mode** (`mode`) | Even, Series, or Random | Even | Even spreads the voices from minus to plus the amount, Series steps them from 0 up to the amount, and Random gives each voice a fixed random share. |
| **Seed** (`seed`) | 1 to 65535, whole numbers | 1 | Picks the offsets in Random mode. The same seed gives the same offsets. |

## Panel

12 by 5 grid units.

- Segmented control: **Mode** (`mode`)
- Knob: **Amount** (`amount`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Voice spread where every voice of a bundle should get its own value from one knob. On a pitch cable it detunes the voices of a chord, or the unison copies of one note.

- **Even** places the voices evenly from minus the amount to plus the amount. Voice 1 gets minus the amount and the last voice gets plus the amount. With an odd number of voices, the middle one stays where it is.
- **Series** steps the voices from 0 up to the amount. Voice 1 stays where it is and the last voice gets the whole amount.
- **Random** gives each voice a fixed share of the amount, from minus to plus, picked by the seed. The same seed gives the same shares every time the patch loads. Raising the voice count keeps the shares of the voices you already have, so they do not move.

On a one-voice cable it does nothing in every mode, so it can sit in a patch before the patch is polyphonic.

Pitch is 0.1 per octave, so one cent is about 0.0000833. A detune of 10 cents is about 0.00083, and 0.0005 is 6 cents. Pick the range -0.01 to 0.01 in the inspector to set an amount that small; a 1% step on that knob is 2.4 cents.

A stereo voice gets the same offset on both sides. For a wider sound, with the sides apart, spread each side with its own Voice spread and seed, and join them.

## Example patch

Four voices from MIDI Note, each detuned by its own random amount of up to 6 cents. Play a chord to hear the voices drift apart.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Detuned chord"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", params: {"voices": 4}),
        PatchModule(id: 2, kind: "loom.voice.spread@1", params: {"mode": 2, "amount": 0.0005}, ranges: {"amount": (-0.01, 0.01)}),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.env.envelope@1", params: {"attack": 0.005, "release": 0.4}),
        PatchModule(id: 5, kind: "loom.math.mul@1", params: {"gain": 0.2}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "pitch")),
        PatchCable(from: (3, "out"), to: (5, "a")),
        PatchCable(from: (1, "gate"), to: (4, "gate")),
        PatchCable(from: (4, "out"), to: (5, "b")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Voices and polyphony](../learn/voices-and-polyphony.md)
- [Repeatable randomness](../learn/repeatable-randomness.md)
- [Voice Merge](loom.voice.merge.md)
- [MIDI Note](loom.midi.note.md)
