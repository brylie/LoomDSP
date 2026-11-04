# ADR-0037: When finite inputs overflow, a module's result holds at the largest finite value

- **Status:** Accepted
- **Date:** 2026-11-04
- **Related requirements:** FR-SIG-01, FR-SDK-02
- **Specification:** [Module test harness, All inputs at once](../design/module-test-harness.md#all-inputs-at-once)
- **Refines:** [ADR-0007](0007-polymorphic-signal-model.md), which says cables are never clipped and leaves overflow open.

## Context

Cables carry any `f32` and are never clipped (ADR-0007), so a feedback patch can send huge values into a module. Two finite values can then make infinity in a module's own arithmetic, such as a product or a sum. Infinity times a gain or a mute of 0 is NaN, and once a feedback loop holds infinity it stays stuck until the patch reloads, because infinity plus or times anything stays infinite. Divide and Selector already held their results. Add, Multiply, and others did not, and the check with every input huge (#575) now finds this.

## Options considered

1. Let the result become infinity, and leave it to the final output to turn that into silence. Pros: a runaway loop goes silent, and the code stays simple. Cons: the loop stays stuck, a gain of 0 can give NaN, and modules disagree, because Divide already holds.
2. Hold the result at the largest finite value of its sign, in every module that does its own arithmetic on finite inputs. Pros: one rule, no NaN from a gain of 0, and a loop settles again when its gain falls. Cons: a runaway loop holds a value that reaches the output, and each such module has one more step per sample.
3. Stated exceptions, module by module. Pros: no code change in the modules. Cons: no rule for the next module to follow.

## Decision

When finite inputs overflow in a module's own arithmetic, the result holds at the largest finite value of that sign (`loom_core::signal::hold_finite`). This is not clipping: normal values never reach it, and they stay bit-identical. A non-finite input still passes through.

## Consequences

- A runaway feedback loop stays recoverable. When its gain falls, it settles again.
- A runaway loop no longer goes silent by itself. With output safety off, it gives full-scale DC until someone breaks the loop. With output safety on, the limiter holds the burst at its ceiling, the DC blocker removes what stays, and the sound dies away.
- That loop makes no bad samples, so the stats bar says **Output clipped**, not **Output: bad samples silenced**. The safety stage counts a finite sample above its input limit as clipped, or the loop would go quiet with no warning.
- Every new module that does its own arithmetic follows this rule. `crates/loom-modules/tests/extreme.rs` checks it for each module in `ALL`.
