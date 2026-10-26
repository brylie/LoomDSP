# Triggers and gates

What a gate and a trigger are, why a LoomDSP trigger lasts exactly one sample, why inputs count rising edges, and why a 1 ms pulse was rejected. For musicians and developers who wire events between modules.

- **Kind:** explainer.
- **Used by:** [ADR-0007](../adr/0007-polymorphic-signal-model.md#triggers) (signal model), [clock and transport specification](../design/clock.md), [arrangement specification](../design/arrangement.md).
- **Related:** [Phase and cycles](phase-and-cycles.md), which makes triggers from wraps.
- **Code:** `loom_core::signal::trigger`, `loom_core::signal::EdgeDetector`, `loom_core::signal::gate_is_high`.

## Gates and triggers

A **gate** is a level. It is high while something is true: a key is held, a note is on. Its length means something, because an envelope stays at its sustain level for as long as the gate stays high.

A **trigger** is an event. It says "now": a loop ended, a bar began, a clock ticked. Its length means nothing. Only the moment it starts matters.

Both travel on the same kind of cable, a yellow gate cable. A trigger has no type of its own. It is a gate that happens to be very short ([ADR-0007](../adr/0007-polymorphic-signal-model.md#triggers)). So any gate can be used as a trigger, and any trigger can open a gate.

## Rule 1: high means above 0.5

An input reads a value as high when it is **above 0.5**. A value of exactly 0.5 is low. Outputs send exactly 1.0 or 0.0. Under the "1.0 = 10 V" model, 0.5 is 5 V, which is well clear of the 0 V and 10 V a gate normally sends. A slow ramp through the threshold flips the input once, at one sample.

## Rule 2: a trigger is one sample long

A trigger output is 1.0 for exactly one sample and 0.0 on every other sample:

| Sample | 0 | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|---|
| Trigger | 0.0 | 0.0 | 1.0 | 0.0 | 0.0 | 0.0 |

*Why one sample:* it is the only length that is exact. The event happened on sample 2, and the cable says so.

### The wrong turn: a 1 ms pulse

Eurorack modules usually send triggers as short pulses of a few milliseconds, and a pulse that long is easy to see on a scope. A fixed pulse looks friendly, but it fails in two ways.

- **Close triggers merge.** At 48 kHz, 1 ms is 48 samples. Two triggers 20 samples apart would each start a 48-sample pulse, and the second starts while the first is still high. The input sees one long high, and so one trigger. A counter that counts bars would lose a bar. The arrangement features ([ADR-0012](../adr/0012-arrangement-from-phase-driven-primitives.md)) count triggers, so a lost trigger is a wrong bar count.
- **The length changes with the sample rate.** 1 ms is 48 samples at 48 kHz and 44.1 samples at 44.1 kHz. A fraction of a sample cannot be sent, so the pulse would differ by device. LoomDSP promises the same output for the same patch ([determinism](../../AGENTS.md#hard-rules)).

The price of one sample is that it is invisible on a scope. That is a display problem, so the display solves it: scopes and cable activity stretch a trigger so people can see it. The signal on the cable stays one sample long.

## Rule 3: inputs count rising edges

An input does not ask "is the signal high?" for a trigger. It asks "is the signal high now, and was it low one sample ago?" That is a **rising edge**.

| Sample | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|---|
| Input | 0.0 | 1.0 | 1.0 | 1.0 | 0.0 | 1.0 | 0.0 |
| Rising edge | no | **yes** | no | no | no | **yes** | no |

A held gate counts once, when it starts. That is why a gate works as a trigger: a key that is held for a second still gives one event. And it is why a one-sample trigger works: it is high for one sample, and that sample follows a low one.

There is a minimum gap. Two triggers need at least one low sample between them. Two high samples in a row are one rising edge.

## The first sample

When a module starts, there is no sample before it. LoomDSP counts the one before the first as **low**. An input that is high from the very first sample fires once, on sample 0. *Why:* the transport's bar trigger fires on the first sample of bar 1. If the first sample did not count, a patch that starts at the beginning of a bar would miss bar 1. After `Module::reset` the input counts as low again.

## No hysteresis

Some hardware uses two thresholds: a higher one to turn on and a lower one to turn off. This stops a noisy signal that hovers near one threshold from firing many times. LoomDSP uses one threshold, 0.5, for every on/off input. One number is easier to explain, and since signals in a patch are exact numbers and not noisy voltages, chattering is a rare case. A patch is deterministic either way.

## In LoomDSP

| Piece | What it does |
|---|---|
| `signal::trigger(fired)` | Gives 1.0 for the sample of the event, 0.0 otherwise. Every trigger output uses it. |
| `signal::EdgeDetector` | Finds rising edges on an input. Keep one in the module, call `rising` once per sample. Its state carries over between blocks, so the result does not depend on block size ([block-size invariance](block-size-invariance.md)). |
| `signal::gate_is_high` | The 0.5 test, for inputs that need a level and not an edge. |

## Further reading

- [VCV Rack voltage standards](https://vcvrack.com/manual/VoltageStandards), on how gates and triggers are sent and read in a modular system.
