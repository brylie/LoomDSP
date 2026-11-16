# Voices and polyphony

What a voice is, why a polysynth has to steal, why LoomDSP infers a bundle instead of asking for one, and why a gate on its sample is all the timing a note needs.

- **Specification:** [Polyphony and voice allocation](../design/polyphony.md).
- **Decision:** [ADR-0038](../adr/0038-notes-travel-as-per-voice-signals.md).
- **Code:** `crates/loom-core/src/width.rs` (width inference), `crates/loom-core/src/compile.rs` (copies and buffers), `crates/loom-dsp/src/voice.rs` (allocation), `crates/loom-modules/src/midi.rs` (MIDI notes and expression), and `crates/loom-modules/src/voice.rs` (Voice merge, Voice split, Voice spread, and Voice assigner).

## A voice is a chain, not a note

A monophonic synth is one chain: oscillator, filter, envelope. Play one key and the chain plays it. Play a second key while the first is held, and the chain has to choose, because it has one pitch input and one gate. That is last-note priority, and it is what `loom.midi.note` does at one voice in Legato mode.

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

The mono LFO into a copied module is the common case, and it is free: every copy reads the same cable. That is the fan-out FR-VOICE-03 asks for, and it needs no module, for the same reason a mono cable into a stereo filter needs none.

## When an input has fewer voices

A single voice fans out to every copy. A wider cable supplies its own voices,
then later copies read that input's default. This lets an 8-voice gate and a
4-voice pitch reach one oscillator without inventing four more pitches.

A phase input shows why a default is more than a buffer of zeros. If its cable
has four voices, voices 5 to 8 have no phase value from that cable. Their
default is the oscillator's own running phase. Giving them a zero phase instead
would hold them still. The cable still exists; it simply supplies no phase for
those later voices. The engine tells the shared own-phase helper which inputs
read their defaults, separately from whether the patch has a cable.

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

## A note belongs to its input

Two keyboards can hold middle C on the same channel. Each needs its own note
identity, or unplugging one keyboard can release the other's note. LoomDSP uses
the input, channel, and pitch together. The engine remembers held notes per input
and sends that input's note-offs when it disconnects.

MPE uses a different channel for each note's expression. The manager channel
adds a shared bend, while member channels carry note bends, pressure, and timbre.
An RPN 6 message announces which channels belong to the zone. That announcement
is device configuration, rather than part of a patch: replacing a patch must not
forget it. The rig retains it and seeds each new engine before sending more MIDI.
Inside a buffer, the engine retains configuration changes at their sample offsets,
so an announcement later in the buffer cannot change an earlier note.

## From one note stream to many voices

Note input hands out voices to MIDI notes. **Voice assigner** (`loom.voice.assign`) does the same for notes that arrive on cables: a sequencer, a Euclidean pattern, a random melody. Each rising edge on its `gate` input starts a note, and each fall ends it. The allocator gives the note an output voice, exactly as for a key.

Why bother, when a mono sequencer can play a mono synth? Because of the release. On one voice, a new note cuts off the tail of the one before. Into an 8-voice assigner, each note takes a new voice, so its tail rings on while the next note plays. A sequencer that should do this needs a gap between its gates: a tied step, with the gate held while the pitch moves, is one note on one voice, with no new attack.

**A note is known by its input voice, not its pitch.** Each input voice of `gate` carries one note at a time, so the assigner does not need a note number. Two input voices on the same pitch are two notes, because something upstream already decided they are separate. While a note sounds, its output voice follows its own input voice's pitch, velocity, pressure, and timbre.

**A stolen note stays silent.** When every voice is busy, a new note steals one by the mode. The stolen note does not come back when a voice frees, even if its gate is still high: it would sound like a note nobody played. Its input voice plays again on its next rising edge.

**Unison** gives each note several voices, taken and released together. They play the same pitch until a Voice spread after the assigner detunes them. This is a different thing from a [stack](unison.md), which copies one sound inside a module.

Voice merge can go the other way: it takes one voice out of a bundle. In Lowest mode with the pitch bundle on `in`, its `out` is the lowest held note and its `gate` output drives one envelope for the bass. Releasing the lowest key hands the bass to the next one without a new attack, as a mono keyboard does.

## Why there is no event cable

The other way to move notes between modules is an **event cable**: a cable that carries "note on, pitch 60, velocity 100, at sample 37" as a message. Many environments have one, as a second kind of signal next to audio. It is exact, it carries any number of notes, and it looks natural for an arpeggiator.

LoomDSP decided against it ([ADR-0038](../adr/0038-notes-travel-as-per-voice-signals.md)), for three reasons that are worth knowing, because the pull toward events will come back whenever someone writes a note effect:

1. **A gate on its sample already carries the timing.** "Note on at sample 37" and "the gate rises at sample 37" are the same information. The gate is just stored in the signal instead of in a message.
2. **One kind of cable is one set of rules.** Events would need their own queue sizes, their own scheduling inside feedback loops, their own drawing, and a rule for every place where an event meets a signal input. Per-voice signals need none of that, because they are signals.
3. **Anything can modulate a note.** A voice's pitch is a signal, so a slew, an LFO, or a quantiser can act on it with no converter in between. In an event model each of those is a special module.

The cost is real and was paid on purpose: every module that makes or changes notes needs voice allocation, and a bundle has a fixed count. The allocation is paid once, as one algorithm in `loom-dsp` that every source calls. The fixed count is the same limit every hardware polysynth has.

## A different value for each voice

Copies of a module share its knobs, so eight copies of a saw play in perfect tune. Real polysynths drift: each voice is a little sharp or flat, and the chord sounds wider for it. **Voice spread** (`loom.voice.spread`) gives each voice its own offset from one `amount` knob. Put it on the pitch path, after Note input, and each voice is detuned by its own small amount.

A stack's spread does something that looks the same but is not. A [stack](unison.md) is one sound copied N times for **one** note, and its spread moves the copies apart: that is unison. Voice spread moves **different** notes apart, one offset per voice. In Even mode both use the same formula, offsets evenly spaced from -1 to 1, so they agree. Random mode gives each voice a fixed share from its seed instead, which is closer to how analog voices drift. The same seed gives the same offsets every time ([repeatable randomness](repeatable-randomness.md)), and a voice keeps its offset when the count changes.

On a one-voice cable Voice spread does nothing, so it can sit in a patch before the patch is polyphonic.

## Why the `level` cable makes a loop

Quietest stealing needs to know how loud each voice is, so the player cables each voice's envelope back into Note input's `level` input. Now the signal goes around a circle: Note input's gate starts the envelope, and the envelope feeds Note input. That is a feedback loop, and every loop needs a delay somewhere so that each sample can be computed from the one before ([feedback loops](feedback-loops.md)). The compiler inserts a one-sample delay and reports it, and the loop runs one sample at a time.

Note input only reads `level` to choose a voice to steal, so one sample of delay changes nothing you can hear. But the compiler cannot know that. It sees a cable, and a cable is an edge in the graph. So the loop costs more: Note input and every envelope in it are called once per sample, not once per block. On the reference laptop the polysynth with this cable costs about 40% more ([the benchmark](../benchmarks/2026-11-15-polysynth.md)). This is why Oldest is the default, and why the shipped polysynth leaves `level` unpatched.

## The polysynth example

`examples/patches/polysynth.ron` puts the ideas on this page in one patch. MIDI Note hands out 8 voices in Oldest mode. Voice Spread detunes each voice by its own amount, up to about 3.6 cents sharp or flat, in Random mode. A Phase module drives a Saw, then an SVF, an Envelope, and an Amplifier, all as 8 copies. One mono LFO sweeps every voice's filter through a Multiply, because a mono cable into a copied module is free. Voice Merge sums the voices into one cable for Audio Out.

Hover over an output in the voice chain and it says `8 voices`; after Voice Merge the cable is one voice. To play a chord, use a MIDI keyboard, or, on macOS and Linux, run `loom midi-port "Loom Pad"` and type `on 60`, `on 64`, and `on 67`.

The Amplifier's gain is low on purpose. Eight voices at their loudest add up, so the gain leaves room for all of them under -1 dBFS, and one note alone plays quietly. The patch's comment shows the arithmetic.

## Compared with other systems

- **Eurorack** has no polyphony as such. A polyphonic patch is N copies of the same modules, patched by hand, with a MIDI-to-CV interface handing out notes to N sets of pitch and gate jacks. LoomDSP's copies are those N module sets, made for you.
- **VCV Rack** carries up to 16 voices on one cable, and modules declare whether they handle polyphony. A module that does not handle it reads the first voice. LoomDSP instead copies the module, so every module handles it.
- **Bitwig's Grid** runs the whole grid once per voice, with a voice count on the device and a few "global" modules outside the voices. LoomDSP infers which modules are per voice from the cables, so the LFO before the filter is global without being moved anywhere.
