# Modulation without cables

Why a modulation depth is a share of a knob's range and not of its units, how polarity remaps a source, why a module must not store a modulated value in the field that `set_param` owns, and why the compiler delays an assignment before a cable to break a loop. For players who assign LFOs and envelopes to knobs, and for developers who write modules.

- **Kind:** explainer.
- **Used by:** [ADR-0014](../adr/0014-modulation-without-cables.md), [modulation specification](../design/modulation.md) (rules 2, 4, 8, and the engine section).
- **Related:** [Pitch at 0.1 per octave](pitch.md), [Feedback loops and the unit delay](feedback-loops.md), [Block-size invariance](block-size-invariance.md).
- **Code:** `loom_core::module::Param`, `ParamSpec::normalize`, `ParamSpec::denormalize`, `crates/loom-core/src/compile.rs`.

## What an assignment is

A **modulation assignment** links one output to one parameter, with a **depth** and a **polarity**, and no cable. You drag an output onto a knob. The knob shows a range arc. Any number of assignments can target one knob.

The engine computes the parameter's value for every sample:

1. Take the **base value** (where the knob is) and turn it into a position from 0 to 1 on the knob.
2. For each assignment, add `depth * source`.
3. Clamp to 0 to 1 and turn it back into a value.

## Depth is a share of the knob

The depth is not in hertz or seconds. It is a share of the knob's full travel, from -1 to 1.

The reason is that parameters live on very different scales. Take the filter cutoff, 20 Hz to 20 kHz. That range is **logarithmic**: equal steps of the knob give equal ratios, because we hear pitch as ratios.

| Depth | Share of the knob | Effect on cutoff, from the base |
|---|---|---|
| 0.1 | a tenth | about one octave (x2) |
| 0.25 | a quarter | about 2.5 octaves (x5.6) |
| 1 | all of it | the whole range, 20 Hz to 20 kHz |

(The cutoff range spans 1000 times. A step of `d` multiplies the cutoff by `1000^d`, so 0.1 gives 1.995, and 0.25 gives 5.62.)

### The wrong turn: depth in units

Suppose depth were in hertz, and you set +100 Hz. On a cutoff at 200 Hz that is a jump to 300 Hz, more than half an octave. On a cutoff at 5000 Hz it is 5100 Hz, a change you can barely hear. A depth that sounds right at one base value is wrong at another, and the same "100" means something completely different on a time or a gain.

With a share of the knob, the same depth sounds the same everywhere on the knob. And the range arc on the knob tells the truth, because it is the same maths in a position from 0 to 1. The inspector, the controls, and the engine all use the same two functions, `ParamSpec::normalize` and `ParamSpec::denormalize`, so they cannot disagree ([parameter mapping](../design/modulation.md#parameter-mapping)).

## Polarity: where the source starts

Sources differ in where they sit. An envelope goes from 0 up to 1 (**unipolar**). An LFO swings from -1 to 1 (**bipolar**).

What each should do to a cutoff is different:

- An envelope should push the cutoff **up from the base** and fall back to it.
- An LFO should swing **around the base**.

By default each source keeps its own polarity: the engine reads it from the output's declared range. An assignment can override it, and then the engine remaps the source `s` before applying the depth:

| Source is | Assignment uses | Value used | Result |
|---|---|---|---|
| bipolar | unipolar | `(s + 1) / 2` | An LFO that only pushes up from the base. |
| unipolar | bipolar | `2s - 1` | An envelope that swings below and above the base. |
| same | same | `s` | Unchanged. |

Only the polarity counts, not the span. A source is never clamped on the cable. Only the final parameter is.

## The stored-value trap

A module keeps its own copy of a parameter, written by `set_param` when the knob moves:

```rust
fn set_param(&mut self, index: usize, value: f32) {
    if index == 0 { self.cutoff = value; }
}
```

In `process`, a modulated parameter arrives as a **buffer**, one value per sample. The tempting code copies the modulated value into the field:

```rust
// Wrong: the field now holds the last modulated value.
self.cutoff = modulated[i];
```

It works while the assignment is there. Then the user removes the assignment. The engine sends no new `set_param`, because the knob did not move. The field still holds whatever the modulation last wrote, so the filter **stays stuck** at that value, and the knob shows another.

The fix is to leave the field to `set_param` and read the modulated value for the moment, without storing it:

```rust
let cutoff = io.param(0);                  // once per call
let c = cutoff.at_or(i, self.cutoff);      // the modulated value, or the field
```

`Param::at_or(frame, base)` gives the modulated value at that frame, or `base` when there is no assignment. The module never handles a slice or an `Option`, so it cannot make this mistake. `Param::map_or` does the same for a value the module keeps in another form, such as a whole number or a pitch.

## Breaking a loop: delay the assignment first

An assignment is an edge in the graph, and a source can be fed by the thing it modulates. An LFO on a filter's cutoff is fine, but a filter's output on its own cutoff is a loop, and every loop needs a one-sample delay ([feedback loops](feedback-loops.md)).

When a loop has no delay, the compiler must add one. It has a choice between a cable and an assignment, and it **delays the assignment**.

*Why:* a delay on an audio cable changes the audio path. In a loop whose period sets the pitch, such as a plucked string, one more sample of delay changes the note. A delay on a modulation path changes only when a control value arrives, and a control value that is one sample late is far below what a player hears. So the audio path stays exactly as it was patched.

Two more details from the spec:

- A loop made only of cables still gets a cable delay. The compiler breaks cable loops first. Then it does not delay an assignment that the cable delay has already taken out of the loop.
- A module assigned to its own parameter is a self-loop. It runs one sample at a time, like any loop.

## In other systems

- **Cable-only systems (Bitwig Grid, VCV Rack, Eurorack):** modulation is a cable, often with an attenuator for depth. The wiring is explicit, and a busy patch becomes a web of cables.
- **Soft synths with a modulation matrix:** a table of sources, targets, and depths, which is the idea behind LoomDSP's assignments and its planned matrix.

LoomDSP gives you the assignment for the quick job and keeps the cable for anything that needs one.
