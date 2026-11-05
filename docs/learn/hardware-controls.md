# Hardware controls and decibel faders

A knob on a MIDI controller sends a number from 0 to 127. It sounds simple to turn that number into a parameter value, but two problems appear on stage: the knob and the screen disagree, and some knobs do not send a position at all. A third problem is on the screen: what a level control's travel should mean.

## Why a mapped knob jumps

A hardware knob stays where you left it. A patch does not know that. You load a patch with the filter cutoff low, but the knob on your controller is at the top. The first small move of that knob sends a value near 127, and the cutoff leaps from low to high. In a performance, that is a loud mistake.

**Pickup** solves it by ignoring the knob until it reaches the parameter. Turn the knob down, and nothing happens until the knob passes the cutoff's position. From there, the knob and the parameter move as one. Nothing jumps, because at the moment they join, they are in the same place.

The catch is that the player cannot see where the knob is. So while a pickup waits and its knob moves, LoomDSP draws a hollow ghost mark at the knob's position. You turn the knob until the ghost reaches the value.

Three details took some thought.

- **The first move.** With no earlier position there is nothing to cross, so a pickup can only catch when the first value lands within one step of the parameter. The second value can then cross it.
- **A value the knob can never reach.** If a mapping covers only part of the travel, the parameter may sit outside that part. The knob could never cross it, and the mapping would never work. So it catches at the nearest end of its span instead.
- **Who keeps the position.** The comparison has to use the value that plays, so it happens in the engine. A module does not report its parameter back, so the engine keeps its own record of each mapped control's position.

## Why an endless encoder is different

A potentiometer has two ends, so it can send a position. An endless encoder turns forever, so it has no position to send. It sends a step instead: "up by 2", "down by 1".

Seven bits must hold both a direction and a size, and hardware makers settled on three ways of writing that:

| Encoding | Up | Down | Why it exists |
|---|---|---|---|
| Offset 64 | 65 is up 1, 66 is up 2 | 63 is down 1, 62 is down 2 | 64 means no move, so the numbers read around a centre. |
| Two's complement | 1 is up 1, 2 is up 2 | 127 is down 1, 126 is down 2 | The same way computers write negative numbers. |
| Sign bit | 1 is up 1, 2 is up 2 | 65 is down 1, 66 is down 2 | The top bit says "down", and the rest is the size. |

The same byte means opposite things: 65 is up in Offset 64 and down in Sign bit, so the mode has to be chosen for each encoder.

A step also breaks something that was true before. With a position, you can work out the value again from the byte at any time. With a step, you cannot: a step applied twice moves twice as far. So the engine reports the exact value it set, and nothing else works it out again.

There is one more trap. On a whole-number parameter, a small step rounds back to the same number, so if each step started from the rounded value, an encoder would never move it. The engine keeps the unrounded position, so small steps add up.

## Why a level control is in decibels

Hearing follows ratios. Doubling a gain sounds like the same change at any level, so a level control should give equal changes for equal travel in ratio, which is what decibels measure. A linear fader puts most of the useful range in a small part of its travel.

A decibel scale has a problem at the bottom: silence is a gain of 0, and 0 is minus infinity in decibels. No straight line reaches it. So a decibel control picks a **floor**, here −60 dB. The bottom of the travel is exactly silence, and the very next position is the floor. Below −60 dB the step to silence is too small to matter.

LoomDSP's mapping is one straight line in dB from the floor to the top. On a Mixer level from 0 to 2, that puts 0 dB at about 91% of the travel, and one MIDI step is about 0.52 dB. Many hardware faders put 0 dB lower and bend the scale, so more of the travel sits near unity. We kept the straight line because it is easy to read and to explain, and we may revisit it.

The stored value stays a linear gain. A module still multiplies by it, and a saved patch keeps its numbers. Only how the control turns and reads changes.

Modulation does work in position, so it does change: a modulated level now swings evenly in dB, not in gain. That is the better behaviour, and it is the one case where a new mapping changes how a saved patch sounds.

## Where this lives

- Specification: [MIDI learn: Mapping modes](../design/midi-learn.md#mapping-modes-fr-midi-09), [Modulation: Parameter mapping](../design/modulation.md#parameter-mapping).
- Code: `ControlState` in `crates/loom-core/src/midi_map.rs`, and `ParamMapping::Decibels` in `crates/loom-core/src/descriptor/param.rs`.
