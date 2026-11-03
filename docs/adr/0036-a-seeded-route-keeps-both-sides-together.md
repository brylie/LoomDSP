# ADR-0036: A seeded module that chooses a route makes the same picks on both sides

- **Status:** Accepted
- **Date:** 2026-11-03
- **Related requirements:** FR-LIB-30, FR-SIG-07, NFR-SND-04
- **Refines:** [ADR-0022](0022-copies-take-state-from-the-running-instance.md), for the seeded modules that choose a route.
- **Specification:** [Stereo cables](../design/stereo.md) rules 15 and 27, [M2 modules](../design/p2-modules.md#selector-mt-route-01)
- **Issue:** [#310](https://github.com/brylie/LoomDSP/issues/310)

## Context

A stereo cable into a module with `PerChannel` ports runs the module as two copies, one per side (ADR-0020). ADR-0022 says a seeded module must not copy its random state as it is, because the right side would play the left side's numbers, which is mono in disguise. So each copy mixes its channel into its seed.

That is right when the random numbers are the signal, as on Noise and Random. Selector's numbers are not the signal. They choose which input to play. If each side chooses for itself, a stereo signal into Selector's inputs can play input 2 on the left and input 3 on the right: not wider, but apart.

## Options considered

1. **Follow the rule.** Left and right pick for themselves, as Random and Chance do. No exception to explain. A stereo signal comes apart on every random pick, which no one patching a stereo source into a selector would expect.
2. **Make Selector's ports `Stereo`,** so one instance sees both sides and picks once. A `Stereo` output is always stereo ([stereo](../design/stereo.md), rule 7), so a mono control signal would turn stereo after a Selector, and everything after it would double.
3. **Leave the channel out of the seed, and copy the stream as it is.** Two copies keep making the same picks, also when a module turns stereo while it plays. `step`, `select`, and `reset` must downmix, or a stereo cable on them would part the sides.

## Decision

Option 3. A seeded module whose randomness chooses a route, not a signal, leaves the channel out of its seed and copies its random stream as it is. Selector is the only such module so far.

Both sides make the same picks unless a stereo modulation source is assigned to a Selector parameter.

## Consequences

- **A stereo signal stays a matched pair** through a random Selector, on load and when it turns stereo during playback.
- **A stereo modulation source can still part the sides.** Stereo rule 17 gives each copy its own side of such a source, on purpose. So the guarantee has to name this case, and a test shows it.
- **Selector passes the strict state-copy check,** `check_copy_state`, not the check for seeded modules. That check is what proves the sides match.
- **The rule has one more clause.** A later seeded module must decide which kind it is: its numbers are the signal, or they choose a route. A Distributor (MT-ROUTE-02) is likely to be the second of the kind.
