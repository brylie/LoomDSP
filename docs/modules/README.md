# Module help pages

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

One page for each built-in module, made from its descriptor. After you change a descriptor, run `mise run help-pages` and commit the pages. Longer notes and example patches go below the marker line at the end of a page. See the [help pages specification](../design/help-pages.md).

## Sources

| Module | ID | Description |
|---|---|---|
| [Noise](loom.osc.noise.md) | `loom.osc.noise` | White, pink, or brown noise. The seed makes renders repeatable (NFR-SND-04). |
| [Pulse](loom.osc.pulse.md) | `loom.osc.pulse` | Band-limited pulse wave shaped from a phase input (polyBLEP). |
| [Saw](loom.osc.saw.md) | `loom.osc.saw` | Band-limited saw wave shaped from a phase input (polyBLEP). |
| [Sine](loom.osc.sine.md) | `loom.osc.sine` | Sine wave shaped from a phase input. |
| [Triangle](loom.osc.triangle.md) | `loom.osc.triangle` | Band-limited triangle wave shaped from a phase input (polyBLAMP). |

## Filters and effects

| Module | ID | Description |
|---|---|---|
| [All-Pass](loom.time.allpass.md) | `loom.time.allpass` | Delay-line all-pass filter. Passes all frequencies at equal steady-state level while smearing phase, for reverbs and diffusers. |
| [Delay Line](loom.time.delay.md) | `loom.time.delay` | Interpolated delay line. The time input adds seconds to the time knob. |
| [Ladder](loom.filter.ladder.md) | `loom.filter.ladder` | A resonant low-pass with a 12, 18, or 24 dB slope. High resonance thins the bass and, at 18 and 24 dB, sings on its own. The cutoff input adds pitch: +0.1 doubles the cutoff. |
| [One-pole](loom.filter.onepole.md) | `loom.filter.onepole` | A gentle first-order filter, 6 dB per octave. The cutoff input adds pitch: +0.1 doubles the cutoff. Low cutoffs smooth control signals. |
| [SVF](loom.filter.svf.md) | `loom.filter.svf` | State variable filter (trapezoidal integration). The cutoff input adds pitch: +0.1 doubles the cutoff. |

## Modulation

| Module | ID | Description |
|---|---|---|
| [Envelope](loom.env.envelope.md) | `loom.env.envelope` | A level from 0 to 1, started by a gate. ADSR holds while the gate is high; AD runs once, so a trigger also starts it. Curve bends every stage. |
| [LFO](loom.mod.lfo.md) | `loom.mod.lfo` | A slow wave. Free mode runs at a rate in Hz; synced mode follows a phase, by default the bar phase. |
| [Sample and hold](loom.rand.sample-hold.md) | `loom.rand.sample-hold` | Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values. |

## Time and rhythm

| Module | ID | Description |
|---|---|---|
| [Clock](loom.clock.clock.md) | `loom.clock.clock` | A phase, trigger, and gate at a musical rate, locked to the transport's bar 1. |
| [Clock Divide](loom.logic.divide.md) | `loom.logic.divide` | Fires on every Nth trigger. Offset fires earlier in the count. Reset starts the count again. |
| [Phase](loom.phase.generator.md) | `loom.phase.generator` | Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs. |
| [Phase Scale](loom.phase.scale.md) | `loom.phase.scale` | Runs a phase at multiply / divide times its speed, locked to the input. Divide counts the input's wraps from the first cycle or the last reset. |
| [Phase Trigger](loom.phase.trigger.md) | `loom.phase.trigger` | Fires a trigger each time the phase wraps, forward or backward. |
| [Phase wrap and fold](loom.phase.wrap.md) | `loom.phase.wrap` | Brings any signal into the phase range, 0 to 1. Wrap goes round the edge; fold turns back at the edges. |
| [Transport](loom.clock.transport.md) | `loom.clock.transport` | The patch's tempo, time signature, and play state. Every patch has exactly one. |

## Notes and pitch

| Module | ID | Description |
|---|---|---|
| [Constant (pitch)](loom.pitch.constant.md) | `loom.pitch.constant` | Outputs a fixed pitch, set in semitones from middle C. |

## Logic and math

| Module | ID | Description |
|---|---|---|
| [Add](loom.math.add.md) | `loom.math.add` | Adds two signals and an offset. |
| [Compare](loom.logic.compare.md) | `loom.logic.compare` | Makes a gate from two signals: high while A is above B, or while A is below B. Hysteresis keeps a noisy signal from chattering. |
| [Constant](loom.math.constant.md) | `loom.math.constant` | Outputs a constant value. Useful for pitch offsets, gates, and testing. |
| [Convert](loom.math.convert.md) | `loom.math.convert` | Converts between decibels and level, and between pitch and frequency in Hz. |
| [Divide](loom.math.div.md) | `loom.math.div` | Divides signal A by signal B. Outputs 0 while B is near zero. |
| [Logic](loom.logic.logic.md) | `loom.logic.logic` | Combines up to four gates with And, Or, or Xor. Only inputs with a cable count. Invert gives Nand, Nor, Xnor, and Not. |
| [Memory](loom.logic.memory.md) | `loom.logic.memory` | Remembers a gate. Set and reset, toggle, or data (a D latch) mode. Reset wins a tie. |
| [Multiply](loom.math.mul.md) | `loom.math.mul` | Multiplies two signals and a gain. Use it as a ring modulator. To control loudness, use Amplifier. |
| [Subtract](loom.math.sub.md) | `loom.math.sub` | Subtracts signal B from signal A. |

## Level and routing

| Module | ID | Description |
|---|---|---|
| [1-Sample Delay](loom.time.delay1.md) | `loom.time.delay1` | Delays the input by one sample. Marks where a feedback loop is broken. |
| [Amplifier](loom.level.amp.md) | `loom.level.amp` | Scales a signal by a control signal, with a linear or exponential response. |
| [Clamp](loom.level.clamp.md) | `loom.level.clamp` | Limits a signal to the range between two limits. NaN becomes 0 first, so Clamp makes a signal safe. |
| [Constant (level)](loom.level.constant.md) | `loom.level.constant` | Outputs a fixed level, set in decibels. 0 dB is a factor of 1. |
| [Crossfade](loom.mix.crossfade.md) | `loom.mix.crossfade` | Blends two signals. Equal power keeps the loudness of audio steady; linear is exact for control signals. |
| [Join](loom.mix.join.md) | `loom.mix.join` | Combines two signals into a stereo cable. |
| [Mixer](loom.mix.mixer.md) | `loom.mix.mixer` | Adds four signals, each with its own level and mute. Two mixers in a row make a bigger mix. |
| [Scale](loom.level.scale.md) | `loom.level.scale` | Multiplies a signal by a factor and adds an offset. A factor of -1 inverts. Bipolar to unipolar is factor 0.5 with offset 0.5. |
| [Slew](loom.level.slew.md) | `loom.level.slew` | Limits how fast a signal can rise and fall. Times are per 1.0 of change, so a glide takes the same time on any signal. |
| [Split](loom.mix.split.md) | `loom.mix.split` | Separates a stereo cable into its two sides. |

## Interface

| Module | ID | Description |
|---|---|---|
| [Audio In](loom.io.audio-in.md) | `loom.io.audio-in` | Brings a pair of channels from the selected audio input into the patch. |
| [Audio Out](loom.io.audio-out.md) | `loom.io.audio-out` | Sends a mono or stereo signal to the audio output. |
| [Button](loom.ui.button.md) | `loom.ui.button` | Sends a gate from a large button. Toggle keeps it. Momentary is high only while held. |
| [Level Meter](loom.display.meter.md) | `loom.display.meter` | Shows the peak and RMS level of its input. Passes the input through unchanged. |
| [MIDI Controller](loom.midi.controller.md) | `loom.midi.controller` | Turns a MIDI controller, the pitch bend, or channel aftertouch into a signal. Controllers and aftertouch go from 0 to 1, bend from -1 to 1. |
| [MIDI Note](loom.midi.note.md) | `loom.midi.note` | Monophonic note input (last-note priority). Pitch includes pitch bend. |
| [Scope](loom.display.scope.md) | `loom.display.scope` | Shows its input over time. Passes the input through unchanged. |
