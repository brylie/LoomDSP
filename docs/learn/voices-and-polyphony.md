# Voices and polyphony

What a voice is, why a polysynth has to steal, why LoomDSP infers a bundle instead of asking for one, and why a gate on its sample is all the timing a note needs.

- **Specification:** [Polyphony and voice allocation](../design/polyphony.md).
- **Decision:** [ADR-0038](../adr/0038-notes-travel-as-per-voice-signals.md).
- **Code:** none yet. It will extend the width inference in `crates/loom-core/src/width.rs` and add an allocator to `loom-dsp`.

## A voice is a chain, not a note

A monophonic synth is one chain: oscillator, filter, envelope. Play one key and the chain plays it. Play a second key while the first is held, and the chain has to choose, because it has one pitch input and one gate. That is last-note priority, and it is what `loom.midi.note` does today.

To play two notes at once you need two chains. A **voice** is one such chain, with its own pitch, its own gate, its own envelope state, and its own filter state. An 8-voice polysynth is eight chains that happen to share one set of knobs.

In a hardware polysynth the chains are eight circuit boards. In LoomDSP they are eight **copies** of each module, which is the same machinery the engine already uses for stereo: a module fed a stereo cable runs as two copies, left and right ([stereo cables](stereo-cables.md)). A module fed an 8-voice cable runs as eight copies. The module's author did nothing.

## Why the bundle is inferred

The obvious design is a switch on the patch: "this patch is polyphonic, 8 voices", and then every cable is 8 wide. It fails in two ways.

First, cost. An LFO that sweeps the filter is the same for every voice, and a mixer after the voices is one signal again. Running them eight times is waste, and most of a patch is control signals that are the same for every voice. FR-SIG-07 made the same point about stereo: do not pay twice for a signal that is mono.

Second, where the count lives. A patch with a keyboard and a chord sequencer has two things handing out voices, and they may want different counts. A sub-patch used twice may be polyphonic in one place and mono in the other.

So the count lives on the thing that hands out voices, the **source**: Note input, Voice assigner, and later the note cycle and the chord sequencer. The compiler follows the cables from each source and makes every module after it as wide as what reaches it, until a module that reads every voice (Voice merge, Audio Out) turns it back into one. Nothing is marked, and a cable is as wide as it needs to be, exactly as stereo cables are.

Here is a chain, with the width of each cable:

| Cable | Width | Why |
|---|---|---|
| Note input `pitch` → Saw `pitch` | 8 voices | Note input has `voices` set to 8. |
| LFO `out` → SVF `cutoff` modulation | 1 voice | The LFO is not after a source. |
| Saw `out` → SVF `in` | 8 voices | The saw runs as 8 copies, so its output is 8 wide. |
| SVF `out` → Envelope `in` | 8 voices | The same. |
| Envelope `out` → Voice merge `in` | 8 voices | Voice merge reads every voice itself. |
| Voice merge `out` → Audio Out | 1 voice | Voice merge summed them. |

Notice that the saw's phase input and the envelope's gate input are declared `Downmix` for stereo, so that a stereo patch does not run them twice for a control signal that is the same on both sides. Voices are the opposite case: a gate differs per voice by definition. So a bundle copies a module whatever its inputs declare, and the stereo declaration keeps its stereo meaning inside each voice. Today's modules become polyphonic without a change.

The mono LFO into a copied module is the common case, and it is free: every copy reads the same cable. That is the "send one signal to all voices" that FR-VOICE-03 asks for, and it needs no module, for the same reason a mono cable into a stereo filter needs none.

## Why a polysynth steals

Eight voices, nine keys. Something has to give. There is no right answer, only conventions, which is why stealing is a mode:

- **Oldest.** Take the voice whose note started first. It is probably the one the player has stopped thinking about. The default.
- **Quietest.** Take the voice whose envelope has decayed furthest. Best for sounds with long releases, where the oldest note might be the loudest. The engine cannot know which signal is "loud", so the player patches the envelope into the source's `level` input, and the source compares those.

Hardware polysynths of the 1980s settled both of these, and every software synth since has copied them; LoomDSP names them in its own words.

A subtler rule is **which free voice to take** when there is one. Taking the voice that has been free the longest is the kind choice: its release tail is the oldest and the least audible when it is cut, and the voices wear evenly. The naive choice, the lowest-numbered free voice, makes voice 1 do most of the work and cut most of the tails.

## Why a stolen note is one sample late

A voice's gate is high while its note is held. An envelope restarts on a **rising edge**: a sample above 0.5 after one at or below it ([triggers and gates](triggers-and-gates.md)). When a voice is stolen, its gate is already high. If the new note simply set the pitch, the envelope would never notice, and the new note would start mid-decay at the old note's level.

So the stolen voice's gate goes low for exactly one sample, then high again. That sample cannot be in the past: the engine has already written it. It has to be the note's own frame, and the note sounds from the next one. One sample at 48 kHz is 21 microseconds, far below anything a player can hear, and it is the same at every block size, which is what matters ([block-size invariance](block-size-invariance.md)). A note that lands on a free voice has no such delay.

## Why a chord change keeps the shared notes

C major is C, E, G. A minor 7 is A, C, E, G. If the chord sequencer sent "all notes off, then four notes on" at the change, every voice would restart, and the player would hear the whole chord re-attack, which is not how a pianist moves between those chords.

LoomDSP puts the rule on the source, not the allocator: a chord change is note-offs for the pitches that left and note-ons for the pitches that arrived. G and C and E are never re-sent, so their voices never see an edge, and their envelopes carry on. A, which arrived, takes a free voice. That is voice leading, and it falls out of two rules that a keyboard needed anyway: a held pitch keeps its voice, and a voice only changes on an edge.

## Why there is no event cable

The other way to move notes between modules is an **event cable**: a cable that carries "note on, pitch 60, velocity 100, at sample 37" as a message. Many environments have one, as a second kind of signal next to audio. It is exact, it carries any number of notes, and it looks natural for an arpeggiator.

LoomDSP decided against it ([ADR-0038](../adr/0038-notes-travel-as-per-voice-signals.md)), for three reasons that are worth knowing, because the pull toward events will come back whenever someone writes a note effect:

1. **A gate on its sample already carries the timing.** "Note on at sample 37" and "the gate rises at sample 37" are the same information. The gate is just stored in the signal instead of in a message.
2. **One kind of cable is one set of rules.** Events would need their own queue sizes, their own scheduling inside feedback loops, their own drawing, and a rule for every place where an event meets a signal input. Per-voice signals need none of that, because they are signals.
3. **Anything can modulate a note.** A voice's pitch is a signal, so a slew, an LFO, or a quantiser can act on it with no converter in between. In an event model each of those is a special module.

The cost is real and was paid on purpose: every module that makes or changes notes needs voice allocation, and a bundle has a fixed count. The allocation is paid once, as one algorithm in `loom-dsp` that every source calls. The fixed count is the same limit every hardware polysynth has.

## Compared with other systems

- **Eurorack** has no polyphony as such. A polyphonic patch is N copies of the same modules, patched by hand, with a MIDI-to-CV interface handing out notes to N sets of pitch and gate jacks. LoomDSP's copies are those N module sets, made for you.
- **VCV Rack** carries up to 16 voices on one cable, and modules declare whether they handle polyphony. A module that does not handle it reads the first voice. LoomDSP instead copies the module, so every module handles it.
- **Bitwig's Grid** runs the whole grid once per voice, with a voice count on the device and a few "global" modules outside the voices. LoomDSP infers which modules are per voice from the cables, so the LFO before the filter is global without being moved anywhere.
