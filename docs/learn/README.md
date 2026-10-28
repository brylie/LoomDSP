# Learn

Explainers: the theory behind how LoomDSP works. Each one teaches an idea from modular sound design, such as phase or triggers, and shows how LoomDSP uses it.

LoomDSP is also a way to learn modular sound design, for musicians and for the people who build it ([product principle 11](../product/vision.md#product-principles)). A specification says *what* a feature does. An explainer says *why it has to work that way*: what the signal really is, why the obvious approach fails, and how Eurorack, VCV Rack, and Bitwig Grid handle the same problem.

## Index

| Explainer | Teaches |
|---|---|
| [Phase and cycles](phase-and-cycles.md) | What a phase is, how to find the end of a cycle, crossings and turnarounds. |
| [Stereo cables](stereo-cables.md) | Why stereo is inferred and not on every cable, why most modules run as copies, and how each signal type is downmixed to mono. |
| [Output safety](output-safety.md) | Why DC harms speakers, why a hard clip is not enough, what a limiter's lookahead costs, and why audio fades in. |
| [Hot patching](hot-patching.md) | Why a new cable into a running patch can click, what state must carry over, and why a crossfade does not hide a cold start. |
| [Feedback loops and the unit delay](feedback-loops.md) | Why a loop needs a one-sample delay, why loops run one sample at a time, and where the compiler puts the delay. |
| [Why feedback FM turns to noise](feedback-fm.md) | Why pitch feedback swings by octaves, why the loop turns chaotic, how aliasing adds noise, and why phase feedback is tamer. |
| [Triggers and gates](triggers-and-gates.md) | Why a trigger is one sample, why inputs count rising edges, and why a 1 ms pulse was rejected. |
| [Pitch at 0.1 per octave](pitch.md) | How pitch relates to 1 V per octave, why 20 octaves fit in -1 to 1, and how to convert to Hz and MIDI notes. |
| [Block-size invariance](block-size-invariance.md) | Why a module must sound the same at any block size, what breaks it, and how the harness checks it. |
| [Real-time safety](real-time-safety.md) | Why the audio thread must not allocate, lock, or wait, and how LoomDSP keeps to that. |
| [Modulation without cables](modulation-without-cables.md) | Why depth is a share of the knob, how polarity remaps a source, the stored-value trap, and why assignments are delayed before cables. |
| [Stopping a stream without a click](stopping-a-stream.md) | Why the engine fades out, why the host waits for silent buffers, and why a new sample rate paused a stream on macOS. |
| [How a ladder filter sings](ladder-filter.md) | Why inverted feedback through stages makes a peak, why four stages sing at the cutoff and two never do, how a loop runs with no unit delay, and how to change the slope without a burst. |
| [Delays and interpolation](delays-and-interpolation.md) | How a circular buffer works, why linear interpolation dulls sweeps and loops, why the all-pass needs two samples of delay, and how to tune a loop. |

## Writing one

- **When:** a feature rests on an idea that a newcomer to modular would not know, or an edge case took real thought to settle. If you had to explain it in a review or a chat, it belongs here.
- **Start from the signal.** Show real sample values, a table, or a small example patch. Readers learn from cases, not from rules alone.
- **Show the wrong turns.** Say which obvious approach fails, and why. That is what stops the next person from "fixing" the code back.
- **Compare with other systems.** Name how Eurorack, VCV Rack, or Bitwig Grid does it, where that helps.
- **Link both ways.** Link the spec, ADR, and code from the explainer, and link the explainer from them.
- **Keep it true.** An explainer is living. Update it in the same change as the behaviour it explains.
- **Keep decisions out.** The options and the choice go in an ADR, and the exact rules go in a spec. The explainer teaches the idea behind them.
