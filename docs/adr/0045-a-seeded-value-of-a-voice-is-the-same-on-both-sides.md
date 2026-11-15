# ADR-0045: A seeded value that belongs to a voice is the same on both of its sides

- **Status:** Accepted
- **Date:** 2026-11-15
- **Related requirements:** FR-VOICE-05, FR-SIG-07, NFR-SND-04
- **Specification:** [Polyphony, Voice spread](../design/polyphony.md#voice-spread-mt-voice-03), [Stereo cables](../design/stereo.md) rules 15 and 27, [M2 modules](../design/p2-modules.md#rules-for-all-modules-here) rule 1
- **Refines:** [ADR-0036](0036-a-seeded-route-keeps-both-sides-together.md), which allows the exception only for a module whose randomness chooses a route.
- **Background:** [Stereo cables](../learn/stereo-cables.md), [Repeatable randomness](../learn/repeatable-randomness.md)
- **Issue:** [#675](https://github.com/brylie/LoomDSP/issues/675)

## Context

A seeded module mixes its stereo side into its seed, so the right side does not play the left side's numbers (ADR-0022, stereo rule 27). ADR-0036 made one exception: a module whose randomness chooses a route. It asked each later seeded module to say which kind it is.

Voice spread (MT-VOICE-03) is neither kind. In Random mode it gives each voice a fixed offset from one draw. The draw is not a stream that plays, and it does not choose a route. It is a property of the voice, like that voice's detune. Voice spread's input is `PerChannel`, so a stereo voice runs as two copies.

## Options considered

1. **Follow the rule.** Each side draws its own offset. No new exception, and a stereo voice gets a small left-right difference for free, which can sound wide on a pad. A player who wants one offset per voice still has ways to get it: spread the pitch before it turns stereo, which is the usual case, or take one side out with Split (`loom.mix.split`), spread it, and feed both sides of a Join from it. But each of those is a workaround the player must know, and a stereo cutoff or a stereo pitch made from two notes has no "before stereo" to spread. Under option 3 the wide case stays easy too: two Voice spreads with different seeds, one per side, into a Join.
2. **Make Voice spread's ports `Stereo`,** so one instance serves both sides. A `Stereo` output is always stereo (stereo rule 7), so every mono chain after Voice spread would double.
3. **Leave the side out of the draw, and copy the state as it is.** The offset depends on the seed and the voice only. Both sides agree, also when a voice turns stereo while it plays.

## Decision

Option 3. A seeded module whose random value belongs to a voice, rather than to a signal or a route, leaves the stereo side out of its seed and copies its state as it is. Voice spread is the only such module so far.

## Consequences

- A stereo voice keeps one detune on both sides. A wide detune, with the sides apart, is two Voice spreads with different seeds into a Join.
- Voices still differ: the voice is in the seed (polyphony rule 16).
- A stereo modulation source assigned to `amount` or `seed` still gives each side its own value (stereo rule 17), as it does on Selector. The guarantee covers the offset, not what an assignment adds.
- The proof is an engine test: one 8-voice source fed into both inputs of a Join makes a stereo bundle with equal sides, which runs through Voice spread in Random and then Voice split. For each voice, `out - in` is not zero, differs from the other voices', and is the same on both sides. It compares `out - in` because the source already differs per voice, so equal sides and different voices alone would pass with every offset at 0. The strict state-copy check also applies, so Voice spread joins Selector in the list of seeded modules that copy their state as it is (`SEEDED_SIDES_TOGETHER` in `crates/loom-modules/tests/common/mod.rs`, which should get a name that covers both kinds). That check compares a copy with the instance it copied, so on its own it does not show that the two sides of a running voice agree.
- The rule now has three kinds: the numbers are the signal, they choose a route, or they belong to a voice. A later seeded module says which.
