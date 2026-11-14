# Normals: what an input reads when nothing is patched

Every input in LoomDSP reads something, even with no cable in it. This explainer shows what it reads, why that one rule covers pulled cables, empty slots, and narrow bundles alike, and why an empty filter slot lets the sound through while an empty envelope slot does not.

- **Decision:** [ADR-0043](../adr/0043-unpatched-means-normalled.md). For phase, [ADR-0042](../adr/0042-unpatched-phase-inputs-are-wired-for-the-player.md).
- **Specification:** [Sub-patches, Slots](../design/sub-patches.md#slots), [Polyphony, rule 6](../design/polyphony.md#rules), [Changing a running patch, rule 9](../design/live-patching.md#rules).
- **Code:** `PortDefault` in `crates/loom-core/src/descriptor/port.rs`, the thru in `crates/loom-patch/src/flatten.rs`, and the registry-wide check in `crates/loom-modules/tests/normals.rs`.

## The jack behind the panel

On a hardware module, an input jack is a socket with a switch inside it. With no plug in, the switch connects the input to something wired inside the module: ground, a fixed voltage, or the output of the module next to it. A plug opens the switch and the cable takes over. Pull the plug and the switch closes again. That inside connection is called the **normal**, and the input is **normalled** to it.

A semi-modular synthesiser plays with no cables at all, because every important jack is normalled to the next stage. The oscillator is normalled into the filter, the filter into the amplifier, and the envelope into the amplifier's level. Patching only changes what you want to change.

## LoomDSP's rule

Every LoomDSP input has a normal: its port's default, shown in its hover text and on its help page. It is one of these:

| Normal | Example | What the input reads |
|---|---|---|
| Zero | A filter's `in` | Silence. |
| A fixed value | An Amplifier's `level`: 1 | That value, so an unpatched Amplifier is open. |
| A transport signal | A sequencer's `phase` | The bar or beat phase. |
| The module's own phase | An oscillator's `phase` | The oscillator runs its own phase at its pitch. |

The rule is short: **when nothing reaches an input, it reads its normal.** That covers four cases that look different on the canvas:

- **No cable.** The obvious one.
- **A cable from a port with nothing behind it.** A sub-patch output whose inside is not connected gives nothing, so the input it feeds reads its normal (flattening rule 8).
- **A voice past a narrower bundle.** A 4-voice pitch into an 8-voice oscillator: voices 5 to 8 read the pitch input's normal (polyphony rule 6).
- **An empty slot.** A slot with no part is a pulled cable. Whatever its output feeds reads its own normal.

So an empty Envelope slot cabled into an Amplifier's `level` plays the Amplifier's input at full level. That is what pulling the envelope's cable out of a hardware amplifier does too.

### Why a module must not make its own normal

Before this rule, the Amplifier declared a default of 0 for `level` but checked in its code whether a cable was there and played 1 if not. The hover text said 0 while the module played 1. That broke two things. A padded voice of a polyphonic bundle reads the declared default, so it played 0 where an unpatched input played 1. And a click-free cable change ramps from what the engine fed the input, which was 0, while the module had been playing 1. Now the normal lives in one place, the port description, and the engine fills it.

### Inputs that switch

A few inputs do more than read a value when nothing reaches them. They are not normals but **switched inputs**, and each is written in its module's spec:

- An oscillator's `phase` runs the oscillator's own phase (ADR-0042).
- The Dynamics sidechain keys from the module's own input.
- A Chance lane follows the lane above.
- A Logic input with no cable does not vote, so an And of two connected highs is high.
- The Transport's `run` and `reset` take over from the panel toggle only while connected.

A test lists them with their reasons, so a new one must be decided, not slipped in.

## The thru

Some places hold an audio path: a filter or an effect sits between a sound and where it goes. When such a place is empty, the useful normal is not silence but the sound itself. A mixing desk's insert point works this way: with nothing plugged in, the signal passes straight through.

A Filter or Effect slot with no part is a **thru**: its `out` carries whatever feeds its `in`. Take the delay out of an Effect chain and the chain keeps playing, now without the delay. An Envelope, Modulator, or Sound source slot has no audio input to pass on, so an empty one is a pulled cable.

## Clicks

Changing between a normal and a signal is a cable change: the input jumps from one value to another. Live-patching rule 9 ramps every cable change over 5 ms, so it does not click ([Hot patching](hot-patching.md) measures where a step becomes audible). Rule 9 is specified but not built yet ([#21](https://github.com/brylie/LoomDSP/issues/21)). Until it is:

- A swap into or out of an empty Filter or Effect slot fades over 10 ms with the slot's own swap fade, between the dry sound and the sound through the part. The fade is linear, not equal power, because the two signals are nearly the same: equal power would swell by up to 3 dB in the middle ([Abstraction](abstraction.md#why-equal-power-not-a-straight-line) explains the two laws).
- Every other change between a normal and a signal steps, as a hand-plugged cable does today.

## How other systems do it

Eurorack modules print their normals on the panel, often as a line from one jack to the next. Semi-modular synthesisers such as the Moog Mother-32 and the Make Noise 0-Coast are designed around them. Bitwig Grid fills some unpatched inputs with what it calls pre-cords. VCV Rack modules normal some inputs in their own code, one module at a time. LoomDSP keeps every normal in the port description, so it is shown, saved with the module's help, and the same for every voice.
