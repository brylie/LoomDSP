# Learn

Explainers: the theory behind how LoomDSP works. Each one teaches an idea from modular sound design, such as phase or triggers, and shows how LoomDSP uses it.

LoomDSP is also a way to learn modular sound design, for musicians and for the people who build it ([product principle 11](../product/vision.md#product-principles)). A specification says *what* a feature does. An explainer says *why it has to work that way*: what the signal really is, why the obvious approach fails, and how Eurorack, VCV Rack, and Bitwig Grid handle the same problem.

## Index

| Explainer | Teaches |
|---|---|
| [Phase and cycles](phase-and-cycles.md) | What a phase is, how to find the end of a cycle, crossings and turnarounds. |
| [Stereo cables](stereo-cables.md) | Why stereo is inferred and not on every cable, why most modules run as copies, and why folding to mono uses the mean. |
| [Output safety](output-safety.md) | Why DC harms speakers, why a hard clip is not enough, what a limiter's lookahead costs, and why audio fades in. |

## Writing one

- **When:** a feature rests on an idea that a newcomer to modular would not know, or an edge case took real thought to settle. If you had to explain it in a review or a chat, it belongs here.
- **Start from the signal.** Show real sample values, a table, or a small example patch. Readers learn from cases, not from rules alone.
- **Show the wrong turns.** Say which obvious approach fails, and why. That is what stops the next person from "fixing" the code back.
- **Compare with other systems.** Name how Eurorack, VCV Rack, or Bitwig Grid does it, where that helps.
- **Link both ways.** Link the spec, ADR, and code from the explainer, and link the explainer from them.
- **Keep it true.** An explainer is living. Update it in the same change as the behaviour it explains.
- **Keep decisions out.** The options and the choice go in an ADR, and the exact rules go in a spec. The explainer teaches the idea behind them.
