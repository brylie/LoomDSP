# Port Ranges Specification

Each port says what range of values it usually carries, so the app can tell the player what a cable holds, warn about likely mistakes, and suggest a fix, without ever changing a module on its own.

- **Kind:** living specification.
- **Status:** Tier 1 is implemented ([#145](https://github.com/brylie/LoomDSP/issues/145)). Tiers 2 and 3 are Agreed, P2 ([#146](https://github.com/brylie/LoomDSP/issues/146), [#147](https://github.com/brylie/LoomDSP/issues/147), 2026-11-01), with the open points settled below.
- **Decision behind it:** no ADR of its own. It builds on [ADR-0007](../adr/0007-polymorphic-signal-model.md), which already says signal types are metadata for colour, default ranges, and module behaviour. Tier 1 does not need a new ADR: it is a small change to `PortSpec`, and modulation is still Draft.
- **Use case and requirements:** FR-SIG-09, FR-VIS-05, FR-MOD-10 (all proposed here). Related: FR-SIG-02, FR-SIG-03, FR-MOD-08, FR-MOD-09.
- **Background:** none yet. An explainer on interval arithmetic, and why its results are too wide, would help (see Open points).
- **Prior art:** Eurorack and VCV Rack publish typical voltage ranges as conventions, for example 0 to 10 V for envelopes and about ±5 V for audio. Module panels often print their output range. In both, a module does not change its settings when you patch it; the player adjusts by ear and by eye.
- **Related specs:** [Control ranges](control-ranges.md) (Draft), [Modulation](modulation.md) rule 4 (Draft), [Port and parameter text](port-and-parameter-text.md) (Implemented), [Telemetry](telemetry.md) (Implemented).
- **Issues:** [#122](https://github.com/brylie/LoomDSP/issues/122) (control ranges, where this idea came up), [#144](https://github.com/brylie/LoomDSP/issues/144) (range replaces polarity).
- **Code:** Tier 1 is in `PortSpec` and the registry in `crates/loom-core`, with port text in `crates/loom-app`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

> My Constant feeds the oscillator's pitch input, and the sound jumped ten octaves. I want the inspector to tell me that the cable carries 0 to 10 while the input expects about -0.5 to 0.5, and to offer a one-click fix. I do not want the module to change itself behind my back.

Every cable is `f32` and polymorphic (ADR-0007). That freedom is the point, but today the only way to see what a cable carries is a scope. The signal type gives a convention ("audio is about -1 to 1"), and many outputs do not follow it: a Constant set to 5 sends 5, and a Multiply with a gain of 4 sends -4 to 4.

During the control ranges discussion ([#122](https://github.com/brylie/LoomDSP/issues/122)), the question came up: could modules publish their output ranges, so connected modules configure themselves? This spec answers: publish ranges as hints (Tier 1), estimate them through the graph for information (Tier 2), and offer suggestions the player accepts with a click (Tier 3). Never reconfigure a module automatically. Rule 13 explains why.

## Terms

| Term | Meaning |
|---|---|
| **Nominal range** | The span a port's signal usually stays in, declared by the module author. A hint, not a limit. |
| **Expected range** | An input's nominal range: the span the input is designed for. Values outside it still work. |
| **Received range** | The controller's estimate of what an input gets in this patch, from cables, parameters, and nominal ranges (Tier 2). |
| **Sent range** | The same estimate for an output. |
| **Unknown** | No estimate. Shown as unknown, never guessed. |

## Behaviour

### Tier 1: ports declare a nominal range

**Implemented** ([#145](https://github.com/brylie/LoomDSP/issues/145)).

- `PortSpec` gets a `range` field. Most ports write `PortRange::Signal`, which means the convention of the port's signal type:

| Signal type | Convention | Notes |
|---|---|---|
| Audio | -1 to 1 | As in ADR-0007. |
| Modulation | -1 to 1 | Unipolar sources (envelopes) declare 0 to 1. |
| Pitch | -0.5 to 0.5 | Ten octaves around middle C, about 8 Hz to 8.4 kHz. ADR-0007 gives the scale; this span is agreed in [#144](https://github.com/brylie/LoomDSP/issues/144). |
| Phase | 0 to 1 | Inputs wrap any value (FR-SIG-04). |
| Gate | 0 to 1 | High above 0.5. |

- A port that differs writes `PortRange::Span(min, max)`. Examples: an envelope output, `Span(0.0, 1.0)`; a future module with a 0 to 5 output, `Span(0.0, 5.0)`.
- On an output, the range says what the port usually sends. On an input, it says what the input is designed for (its expected range).
- Port hover text and the inspector show the nominal range next to the description, for example "Usually 0 to 1."
- **Polarity comes from the range.** A range with `min >= 0` is unipolar; any other range is bipolar. `PortSpec::polarity` is removed: the range replaces it, so a port cannot declare two things that disagree. [Modulation](modulation.md) rule 4 takes the source's polarity from this range. The assignment's own polarity override (ADR-0014) stays.

A sketch of the type. The names `PortRange`, `Signal`, and `Span` may change when Tier 1 is built:

```rust
pub enum PortRange {
    /// The convention of the port's signal type (see the table).
    Signal,
    /// A nominal span. Finite, with min < max.
    Span(f32, f32),
}
```

#### Use in modulation

**Decided:** modulation takes only the **polarity** from the source's range, and does not scale by it. Modulation rule 4 remaps a source by polarity: a bipolar source on a unipolar assignment uses `(s + 1) / 2`. Every built-in source today declares 0 to 1 or -1 to 1, except Pitch, which declares -0.5 to 0.5. Ignoring the Pitch span is intentional: a pitch source on a unipolar assignment maps to 0.25 to 0.75, not 0 to 1. Not scaling keeps declared output ranges out of the sound (rule 5).

*Considered, not adopted:* remap by the range itself, with `(s - min) / (max - min)` on a unipolar assignment, so a 0 to 5 envelope moves a knob by the same share as a 0 to 1 envelope. It can be added later if a source declares another span. That needs its own spec change first, and it must use the source's **declared** range only, never the propagated one.

### Tier 2: the controller estimates ranges through the graph

The controller walks the graph on the control thread and gives every port a received or sent range.

1. **Order.** Modules are visited in the order the compiler already finds (strongly connected components, ADR-0002).
2. **Inputs.** An input with a cable gets the sent range of that cable's output. An unconnected input gets its `PortDefault`: `Value(v)` gives v to v; `BarPhase`, `BeatPhase`, and `Playing` give 0 to 1.
3. **Outputs.** If the module has a range hook, the hook computes each output's range from its input ranges, which inputs are connected, and its parameter ranges. Without a hook, an output gets its declared nominal range, whatever its inputs are. An oscillator always sends -1 to 1.
4. **Parameters.** An unmodulated parameter is its value, v to v. A modulated one is every value the modulation can reach: its selected [control range](control-ranges.md), or its `min` to `max`.
5. **Feedback loops.** An output that has a hook and sits inside a feedback loop is unknown. Unknown spreads through hooks: an unknown input makes a hook's outputs unknown, unless the hook says otherwise. Outputs without hooks keep their declared range inside loops, so the oscillator in `fm-feedback.ron` still sends -1 to 1.
6. **Non-finite results.** A hook result that is not finite becomes unknown.

The built-in math modules get hooks that use interval arithmetic:

| Module | Sent range |
|---|---|
| Constant | `value` to `value` |
| Add | `a + b + offset`, adding the lowest and the highest ends |
| Multiply | `a × b × gain`: the lowest and highest of the products of the ends. An unconnected B counts as 1, as the module does. |

*Why Multiply needs the connected flags:* its B input reads 1 when no cable is connected, but its `PortDefault` is 0. The hook must mirror the module's real behaviour, not the default.

A sketch of the hook, as a plain function in `ModuleDescriptor`:

```rust
pub enum Range { Known(f32, f32), Unknown }

pub type RangeFn = fn(
    inputs: &[Range],
    connected: &[bool],
    params: &[Range],
    outputs: &mut [Range],
);
```

#### What the player sees

- **Inspector, inputs:** "Receives about 0 to 5." When that is wider than the expected range, also "Expects -0.5 to 0.5." Unknown inputs show "Receives an unknown range (feedback loop)."
- **Inspector, outputs:** "Sends about -4 to 4."
- **Scope:** a scope module starts with its vertical scale fitted to the range its input receives. A point range, such as an unmodulated Constant at 5, has no span to fit, so the scope centres on the value with the span of its input's expected range (4 to 6 for an audio input). An unknown range falls back to the samples on screen.
- **Warnings:** a short line under the input in the inspector. They never block a cable and never change the sound.

| Input type | Warning when | Text (draft) |
|---|---|---|
| Gate | received max ≤ 0.5 | "Probably never goes high: receives about 0 to 0.3." |
| Gate | received min > 0.5 | "Always high: receives about 0.8 to 1." |
| Pitch, Audio, Modulation | received span is more than 4 times the expected span, or the two do not overlap | "Receives up to 0 to 10. Expects -0.5 to 0.5. That is 100 octaves of pitch." |
| Phase | never | Phase inputs wrap any value. |

#### Why the estimate is too wide

Interval arithmetic treats every input as independent. For A minus A, it gives -2 to 2, although the result is always 0. For A times A with A in -1 to 1, it gives -1 to 1, although the result is never below 0. So a range is an upper bound on the hints, often wider than the real signal. The text says "about" and "up to" for this reason. How to show it best is an open point.

The width matters for warnings. A warning that a gate *never* goes high is safe, because the true signal stays inside the estimate (as far as the hints are honest). A warning that a span is *too wide* can be wrong, because the estimate can be too wide. Its wording says "up to" for this reason.

### Tier 3: suggestions, never automatic changes

- **Match range.** A parameter that offers ranges ([control ranges](control-ranges.md)) can show a **Match** button next to its Range selector, for example "Match: -1 to 1". Clicking it selects the smallest listed range that holds the expected ranges of every input the module's output feeds. For Constant `value`, that is the whole rule. Add `offset` gets the same rule only while neither A nor B has a cable, because Add then sends just its offset, as a Constant does. With a cable on A or B, the offset that keeps the output in range depends on the inputs, so Add offers no Match (see Open points). For Multiply `gain`, see Open points too.
- **No suggestion** when the module's output has no cable, when the match is the range already selected, or when only the widest range would hold it.
- Clicking Match is the same as picking that range by hand. Control ranges rule 6 applies: a smaller range clamps the value, a wider one keeps it. It is one undo step.
- **New module from a cable.** If the canvas gets a gesture that creates a module from a dragged cable (not specified yet), the new module's ranges are chosen once, by the same match rule, instead of the author's defaults. From then on they are ordinary saved choices, and nothing changes them except the player.

## Rules

### Tier 1

1. **A range is a hint.** It is never enforced, never clipped, and never blocks a cable. *Why:* cables are polymorphic and unclipped (ADR-0007, FR-SIG-03). A pitch input fed 0 to 10 is a strange patch, not an error.
2. **Every port has a range, and most use their signal type's convention.** `range` is a required field, like every `PortSpec` field (ADR-0016). *Why:* struct literals make a missing field a compile error, so no port is forgotten. `PortRange::Signal` keeps the common case short.
3. **A span is finite, with min < max.** The registry refuses others. *Why:* an empty or infinite span says nothing, and it breaks propagation.
4. **An input's range never changes what the module does.** *Why:* it is text for people and input to warnings. The module's behaviour lives in its code and its help page.
5. **Ranges are not saved in patches, and the sound never depends on a propagated range.** Modulation takes only the polarity from a declared range, so changing a span such as 0 to 1 to 0 to 5 does not change the sound, unless it crosses zero and flips the polarity. A flip changes the remap on every assignment that forces a polarity on that source, so after the first release it needs a new module version, like renaming a released port. During the early alpha, such a change updates every example patch and reference render in the same pull request (AGENTS.md, Project stage). *Why:* the same patch must sound the same in every build (hard rule 3). A propagated range changes whenever a cable moves, and a declared range can be tuned between releases, unless the sound depends on it.

### Tier 2

6. **Propagation runs on the control thread only.** It runs when the graph changes and when a parameter that a hook reads changes, at most a few times a second. *Why:* hard rule 1. The audio thread never sees ranges. Propagation is linear in the number of cables, so this is cheap.
7. **Ranges never change the sound.** Output is bit-identical with and without propagation. *Why:* like telemetry probes (telemetry rule 6), ranges are view state.
8. **Hooks are pure functions.** No state, no allocation required, the same result for the same inputs. They live in `loom-core` types and build for `wasm32-unknown-unknown`. *Why:* they run often, and the CLI and a future web build should give the same warnings (hard rule 6).
9. **Loops through hooks give unknown, not a guess.** *Why:* iterating around a loop can grow without end (an Add feeding itself through a delay grows on every pass). Methods that force such an iteration to stop are complex and give poor estimates. Outputs without hooks still give their declared range, so most loops stay mostly known.
10. **An estimate is never narrower than the hints allow.** *Why:* a too-narrow range would make "never goes high" false. Too wide is the safe error, and the text says "about" and "up to".
11. **Warnings never block, and never fire on unknown ranges.** *Why:* "patch anything to anything" (FR-SIG-03). A warning is advice for a learner, not a rule.

### Tier 3

12. **A suggestion acts only when the player clicks it, and it is one undo step.** *Why:* the player stays in charge, and every edit can be undone (FR-CANVAS-04).
13. **Only the player changes a module's settings.** Creating a module from a cable is the one moment the graph may pick a default, and only once. Ongoing automatic configuration is out of scope. *Why:*
    - **Patch meaning would depend on what is connected.** Moving a cable would change another module's knob, so the same knob position would sound different after a re-patch. On stage, re-patching one module must not silently change another.
    - **It conflicts with control ranges rule 6.** Narrowing a range clamps the value. Automatic narrowing would change values with no gesture from the player. Automatic widening would change how a knob feels while the player turns it.
    - **It conflicts with control ranges rule 7.** A saved value never changes on load. If ranges were derived again on load, a hint tuned in a later release, or a different propagation result, would change saved patches.
    - **Estimates are unknown in loops and too wide elsewhere.** Settings built on them would be wrong in exactly the patches that need care, and would flicker as cables change.
    - **A visible suggestion teaches; a silent change hides** (product principle 11). The player learns that the cable carries 0 to 10 and why the knob range matters.

## Acceptance criteria

### Tier 1

- [x] Every built-in port declares a range. Its derived polarity equals the `polarity` it declared before this change. Envelope `out` and MIDI `velocity` declare `Span(0.0, 1.0)`, even though velocity is a Modulation signal, whose convention is -1 to 1. The rest use their signal type's convention unless their help page says otherwise.
- [x] The registry refuses a span that is not finite or has `min >= max`.
- [x] Polarity derived from the range gives unipolar for envelopes, gates, and phases, and bipolar for LFOs, audio, and pitch, as before.
- [x] The existing modulation tests pass; new tests also cover silent unknown sources.
- [x] Port hover text shows the nominal range.
- [x] Output of every reference render is bit-identical before and after ranges are added.

### Tier 2

- [ ] A Constant at 5 into Multiply A, with B unconnected and gain 2, gives Multiply a sent range of 10 to 10.
- [ ] Add with A from an oscillator (-1 to 1), B from an envelope (0 to 1), and offset 0.5 sends -0.5 to 2.5.
- [ ] A modulated parameter uses its selected control range in the hook.
- [ ] In `fm-feedback.ron`, the oscillator sends -1 to 1, and outputs with hooks inside the loop are unknown.
- [ ] A gate input fed 0 to 0.3 shows the "never goes high" warning. A gate input fed by an unknown range shows none.
- [ ] A pitch input fed 0 to 10 shows the span warning.
- [ ] A scope fed by an unmodulated Constant at 5 starts at 4 to 6. A scope fed by an unknown range starts from its samples.
- [ ] Rendered output is bit-identical with propagation on and off.
- [ ] Propagation allocates nothing on the audio thread, because it never runs there (`no_alloc` test unchanged).

### Tier 3

- [ ] A Constant feeding a pitch input offers "Match: -1 to 1" (the smallest listed range that holds -0.5 to 0.5).
- [ ] Add with no cables offers the same Match as a Constant. Add with a cable on A or B offers none.
- [ ] Clicking Match selects that range, follows control ranges rule 6, and undoes in one step.
- [ ] Moving or removing a cable never changes any module's selected range or value.
- [ ] A patch loads with its saved ranges and values, whatever the graph's estimates.

## Out of scope

- Enforcing or clipping ranges anywhere. Cables stay unclipped (ADR-0007). Output clipping is [#121](https://github.com/brylie/LoomDSP/issues/121).
- Ongoing automatic reconfiguration of modules (rule 13).
- Units such as hertz or volts on ranges. A range is in cable units; pitch help text can translate.
- Ranges measured from live audio. Telemetry already gives peaks per probe; a "measured range" view is a separate feature.
- Ranges through sub-patch boundaries (FR-ENCAP, P2). Leaning: a sub-patch output takes the range its inner graph sends.
- Per-voice ranges for polyphonic cables (FR-SIG-08, P2).

## Decisions

Settled in [#144](https://github.com/brylie/LoomDSP/issues/144):

- **The range replaces polarity.** One field, polarity derived from it. *Why:* two fields can disagree. `polarity` is a required `PortSpec` field (ADR-0016), so replacing it touches about 50 port literals, but the early-alpha rule (AGENTS.md, Project stage) says to prefer the clean design over a shim. [Modulation](modulation.md) rule 4 now says polarity comes from the source's range.
- **Modulation takes polarity only, not scale.** *Why:* scaling would make declared output ranges part of the sound. Pitch (-0.5 to 0.5) is the one built-in span that scaling would change: it would map to 0 to 1 on a unipolar assignment instead of 0.25 to 0.75. Scaling needs its own spec change, and must use the source's declared range only.
- **Pitch convention: -0.5 to 0.5** (ten octaves around middle C). *Why:* it covers the audible and musical pitch range with some margin. Tier 2 warnings may tune the thresholds, not this span.
- **No ADR.** *Why:* ADR-0007 already says signal types carry default ranges, and ADR-0014 already gives assignments a polarity. This change only replaces where a port's polarity comes from.

## Settled points

Settled on 2026-11-01 from the written leanings ([#146](https://github.com/brylie/LoomDSP/issues/146), [#147](https://github.com/brylie/LoomDSP/issues/147)):

- **Overestimation** is shown with "about" for received and sent ranges and "up to" in warnings, as drafted above. Hover text on the range explains in one sentence why an estimate can be wider than the signal, and links the explainer.
- **Warnings** use the thresholds in the table: more than 4 times the expected span, or no overlap. They show only in the inspector; canvas ports get no mark. *Why:* marks would add clutter to every patch for advice.
- **Match** is offered for Constant `value`, and for Add `offset` only while A and B have no cable. Multiply `gain` gets no Match. The default for a module made from a dragged cable waits for that gesture.
- **API:** `Controller::port_range(port)` and a list of warnings, so `loom check` prints the same warnings as the app.
- **Phase:** Tiers 2 and 3 are P2.
- **Explainer:** "What is on this cable?" in `docs/learn/`, written with Tier 2 and linked from this spec.

## Open points

None.
