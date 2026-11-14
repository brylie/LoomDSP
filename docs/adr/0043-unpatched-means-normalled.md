# ADR-0043: Unpatched means normalled

- **Status:** Accepted
- **Date:** 2026-11-14
- **Related requirements:** FR-ENCAP-08, FR-GRAPH-05, FR-LIB-01
- **Specification:** [Sub-patches, Slots](../design/sub-patches.md#slots), [Changing a running patch, rule 9](../design/live-patching.md#rules), [Polyphony, rule 6](../design/polyphony.md#rules)
- **Refines:** [ADR-0042](0042-unpatched-phase-inputs-are-wired-for-the-player.md). It names own phase as one of the switched inputs below, and does not change it.
- **Explainer:** [Normals: what an input reads when nothing is patched](../learn/normals.md)

## Context

An input can have nothing behind it in several ways. It may have no cable at all. A cable may come from a port with nothing behind it (flattening rule 8). A voice may lie past the last voice of a narrower bundle (polyphony rule 6). And, with this ADR's batch, a slot may be empty. Each case needs an answer for what the input reads.

The answers grew one by one. ADR-0042 gave phase inputs a normal. Polyphony padded voices with the port's default. Flattening dropped cables from ports with nothing behind them. Some modules made their own value in code, such as the Amplifier's `level`, which declared a default of 0 but played 1 while unconnected. An empty slot needed a rule of its own, and the obvious ones disagreed with each other: silence everywhere, or a hidden module that outputs a connected zero.

A semi-modular instrument has one answer for all of these. Each input jack is **normalled**: wired inside to a default. Plugging in a cable breaks that link, and pulling the cable restores it. A player learns the rule once.

## Options considered

1. **A connected zero for every absence.** A hidden module stands in for what is missing, so every input sees a cable. Rejected. It needs hidden modules the player cannot see. It hides the normal the input's own hover text shows. And an Amplifier fed an empty Envelope slot goes silent, where pulling the same cable would leave it open.
2. **Silence for every empty path.** An empty slot outputs nothing, and an audio path through it is cut. Rejected. Taking one effect out of a chain would silence the whole chain.
3. **Normals and thrus.** An input with nothing behind it reads its normal. A place that holds an audio path and has nothing in it passes its audio input through. (Chosen.)

## Decision

Option 3.

1. **An absence reads the normal.** When nothing reaches an input (no cable, a pulled cable, an empty slot, or a voice past a narrower bundle), the input reads its normal: the `PortSpec` default. That default is zero, a fixed value, a transport signal, or the module's own phase.
2. **A module never makes its own normal *value* in code.** The value the port's hover text shows is the one that plays. A registry-wide test checks every module whose input has a fixed-value default: the module plays the same with the input unconnected as with a constant cable at that value.
3. **Switched inputs are allowed where their spec says so.** Some inputs do more than read a value when nothing reaches them: they switch to another source, or they take no part. They are not normals, and the test lists each one with its reason:
   - **Own phase** (ADR-0042): an oscillator runs its own phase.
   - **The Dynamics sidechain:** it keys from the module's own input.
   - **Chance lanes:** a lane follows the lane above.
   - **Logic inputs:** an unconnected input does not vote.
   - **Transport `run` and `reset`:** a cable takes over from the panel control only while connected.

   Inputs whose default is a transport signal are filled by the engine and need no rule.
4. **An empty audio place is a thru.** A slot whose role has a required audio input `in` and a required audio output `out`, and which is marked as a thru role, passes `in` to `out` while it is empty. Filter and Effect are thru roles. The flattener routes it as it routes a pass-through port (flattening rule 9), and adds no module.
5. **LoomDSP never adds a hidden module to fake an absence.** A change between a normal and a signal is a cable change. Live-patching rule 9 is what makes it click-free, once it is built ([#21](https://github.com/brylie/LoomDSP/issues/21)). Until then, a slot swap into or out of an empty thru slot is softened by the swap fade, and every other such change steps, as a hand-plugged cable does.

## Consequences

- A player learns one rule: an absence behaves like a pulled cable. An empty Envelope slot into an Amplifier's `level` plays at the Amplifier's normal, which is open.
- The Amplifier's `level`, Multiply's `b`, and Divide's `b` declare a default of 1 and lose their `is_connected` fallbacks. They play the same, except that a padded voice of a narrower bundle into them now reads 1, where it read 0.
- Live-patching rule 9 can ramp from the value the engine actually fed, because that value is now the normal.
- A new module whose unconnected input does something other than read its default must be added to the switched-input list, with its reason and its spec. The test fails until it is.
- A future role with an audio path, such as Shaper, can become a thru role by setting one field.

## Prior art

On a Eurorack module, a normalled jack is wired inside to a default: a VCA's level input may be normalled to fully open, and a filter's input to the oscillator before it. A semi-modular synthesiser such as a Moog Mother-32 or a Make Noise 0-Coast plays with no cables because every important jack is normalled. Bitwig Grid calls its version pre-cords. A mixing desk's insert point is a switched jack: with nothing plugged in, the signal passes through. LoomDSP takes the convention, not any product's wiring ([ADR-0018](0018-clean-room-module-development.md)).
