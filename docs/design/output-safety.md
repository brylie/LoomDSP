# Output Safety Specification

A safety stage on the engine's final output, so a patch mistake cannot harm the player's speakers or ears.

- **Kind:** living specification.
- **Status:** Implemented. Rules 1 to 13 were agreed in #131 and built in the same change. Rule 14 (fade-out before an engine stops) was agreed in #274 and built in #26. The app preference is in the [preferences spec](preferences.md).
- **Decision behind it:** [ADR-0019](../adr/0019-output-safety-stage.md), which extends the output rule of [ADR-0007](../adr/0007-polymorphic-signal-model.md).
- **Use case and requirements:** UC3 (perform live), UC8 (a generative system left running), FR-PERF-08, FR-PERF-05, NFR-SND-02, NFR-SND-03, NFR-RT-01.
- **Background:** [Output safety explainer](../learn/output-safety.md): why DC harms speakers, why a hard clip is not enough, and what lookahead buys.
- **Prior art:** most DAWs leave the master bus unprotected and expect the user to add a limiter. Hardware mixers and PA processors often have a fixed output limiter. We describe our own stage here.
- **Issues:** [#131](https://github.com/brylie/LoomDSP/issues/131); rule 14 comes from [#26](https://github.com/brylie/LoomDSP/issues/26) (device selection). Related: [#200](https://github.com/brylie/LoomDSP/issues/200) (listening checks), [#25](https://github.com/brylie/LoomDSP/issues/25) (Panic), [#130](https://github.com/brylie/LoomDSP/issues/130) (silenced samples in the stats bar), [#121](https://github.com/brylie/LoomDSP/issues/121), [#123](https://github.com/brylie/LoomDSP/pull/123).
- **Code:** `crates/loom-core/src/safety.rs`, run at the end of `Engine::run_block`. Mode changes: `Controller::set_output_safety`. Fade-out before a stop: `Controller::fade_out`, `Engine::is_silent`, and `RunningAudio::stop` in `crates/loom-host-desktop`. Flags: `crates/loom-cli/src/main.rs`, `crates/loom-app/src/main.rs`. Tests: `crates/loom-modules/tests/output_safety.rs`.

## Goal

"I patched a Constant into Audio Out by mistake, on stage, through the PA. Nothing bad should happen. And when I load my next patch, it should not start with a bang."

The player does not think about the stage. It is on by default, adds no latency, and only acts on accidents. A player who knows their chain can make it cleaner, or turn it off.

## Behaviour

The engine sums its Audio Out modules, as today. Then the left and right channels pass through these steps, in this order:

| Step | What it does | Modes |
|---|---|---|
| 1. Sanitize | NaN and infinity become 0. Other values are clamped to -16 to 16. | All |
| 2. DC blocker | A first-order high-pass filter at 5 Hz. | Low latency, Clean |
| 3. Safety limiter | Holds every sample at or under the ceiling, -1 dBFS (about 0.891). One gain for both channels. | Low latency, Clean |
| 4. Fade | Ramps the output gain up when audio starts. | All |
| 5. Clip | Clips to -1 to 1 (ADR-0007). | All |

### Modes

| Mode | Limiter | Added latency | Sound when it acts | For |
|---|---|---|---|---|
| **Low latency** (default) | No lookahead. Gain falls at once, in the sample that goes over, and recovers over 100 ms. | None | Can distort briefly on the first peaks. | Live playing, where the limiter should only ever catch accidents. |
| **Clean** | 2 ms lookahead. Gain falls smoothly over the 2 ms before a peak, and recovers over 100 ms. | 2 ms (rounded to whole samples at the device rate) | Transparent. | Studio work, recording, or players who limit on purpose. |
| **Off** | None. No DC blocker either. | None | The raw patch output, clipped to -1 to 1. | Players who know their chain, and DC-coupled interfaces that send control voltages to hardware. |

### Where the setting lives

| Host | Default | How to change it |
|---|---|---|
| Desktop app | Low latency | App preference (per machine). Until the app has a preferences store, a command-line flag `--output-safety low-latency\|clean\|off`. |
| `loom play` | Low latency | `--output-safety` flag. |
| `loom render` | Off | `--output-safety` flag. |
| Exported plugin (later) | Off | Not exposed for now. |
| `EngineConfig` (Rust API) | Low latency | A field on `EngineConfig`, and a command through the `Controller` at run time. |

## Rules

1. **The stage is on by default wherever audio reaches speakers.** *Why:* an accident is the case nobody planned for, so the protection must not depend on the player having set it up.
2. **The mode belongs to the machine, not the patch.** The patch format does not store it. *Why:* it protects the player's own speakers and ears. A patch from someone else must not be able to turn it off.
3. **The default adds no latency.** *Why:* LoomDSP is built for live playing, and players feel added latency. The limiter is a safety net, not a mastering tool, so brief distortion on an accident is the right trade.
4. **The ceiling is -1 dBFS, sample peak.** *Why:* it leaves room for the peaks between samples that a converter can make from a signal clipped at 0 dBFS. Measuring true peaks needs oversampling, which costs too much for a safety net.
5. **The DC blocker is first order at 5 Hz.** *Why:* it removes a constant offset within a few hundred milliseconds (time constant about 32 ms) and is inaudible: at 20 Hz it cuts about 0.3 dB. A higher order would remove DC faster but cut real sub-bass.
6. **The limiter and DC blocker see the signal before the clip.** *Why:* the limiter then lowers the gain of a loud signal instead of meeting flat-topped waves. The final clip stays as the last line of defence, and in Low latency and Clean modes it never acts.
7. **The input is clamped to -16 to 16 before the filter.** *Why:* a finite but huge value (for example 1e38) could overflow the filter state to infinity, and the stage would then output silence for good. 16 is about +24 dBFS, far above any useful signal.
8. **Fade-in: 10 ms, linear, from 0 to 1.** It runs when the engine starts, in every mode. Loading or creating a patch in the app builds a new engine, so it fades in too. In Clean mode the fade starts when the first sample comes out of the lookahead delay. *Why:* any first sample that is not 0 is a click. Patch load is not timed to the beat, so a longer fade than the 5 ms of cable fades (FR-GRAPH-05) costs nothing. A fade over the delay's silence would meet the first real sample part way up, which is a step.
9. **Changing the mode dips the output.** The fade ramps to 0 over 10 ms, the mode changes and the limiter and filter states reset, then the fade ramps back up over 10 ms. *Why:* switching between Low latency and Clean changes the latency, which is a jump in the signal. A dip of 20 ms is better than a click.
10. **One gain for both channels.** The limiter turns left and right down together. *Why:* limiting each side on its own would move a loud sound toward the quieter side.
11. **The stage is real-time safe, block-size invariant, and deterministic.** The lookahead delay line is allocated when the engine is built, at the device rate, even in Low latency mode. All state advances one sample at a time. *Why:* hard rules 1 to 3. Allocating up front means switching to Clean never allocates on the audio thread.
12. **The order with Panic and a master volume.** A future master volume sits before the stage, so the limiter protects whatever volume is set. Panic (#25) may use the stage's fade to ramp out and back in. Panic's own spec decides what it resets. *Why:* a volume after the limiter could push the output over the ceiling again.
13. **Offline render and plugins default to Off.** *Why:* a render is how we check what a patch really does (reference renders, NFR-SND-03 comparisons), and a plugin host has its own master section. Both can opt in.
14. **Fade-out before an engine stops.** The host can ask the engine to fade out. The fade then ramps from where it is now to 0, in a straight line, at the same rate as the fade-in (0 to 1 takes 10 ms), and the output stays at 0 until the engine is dropped. It works in every mode, Off included. The engine reports when its output has reached 0. The host then keeps the stream running until it has handed the device at least two more buffers of silence, and only then stops it. It stops after 100 ms in any case, if no report comes or the device stops asking for buffers. The app fades out before Open, New, a device change ([preferences.md](preferences.md#audio-and-midi-devices)), and quit. *Why:* stopping a stream mid-signal steps the output to 0, which is the same click as a first sample that is not 0 (rule 8). In Off mode a held DC offset, such as a Constant into Audio Out, makes a thump when it stops. The DC blocker cannot help there, because the step happens after it. The engine renders a buffer before the device plays it, so a stream stopped at the report could throw away the fade that was rendered but not yet played, and click anyway. Two more buffers push the whole fade through to the device. The 100 ms limit keeps a stuck audio thread from holding up the app. A device that has already gone (FR-APP-04) cannot be faded, because its stream no longer runs. Rule 14 fades the whole output when an engine stops. A cable plugged or unplugged while the engine runs makes the same kind of step on one signal, and gets its own short fade inside the engine ([#21](https://github.com/brylie/LoomDSP/issues/21), FR-GRAPH-05).

## Acceptance criteria

Each is an engine test in `crates/loom-modules/tests/output_safety.rs` unless it says otherwise. The DC, noise, and fade tests run at host block sizes 1, 77, and 512, and check that every size gives the same output.

- [x] **DC removal.** Given Low latency or Clean, when a Constant of 1.0 plays into Audio Out, then after 0.5 s every output sample is under 0.001 in size.
- [x] **Noise held under the ceiling.** Given Low latency or Clean, when full-scale noise at gain 4 plays, then no output sample is over the ceiling in size.
- [x] **Latency of each mode.** Given a single impulse at 0.5 (under the ceiling) after the fade-in, then it comes out 0 samples late in Low latency and Off, and `round(0.002 * sample_rate)` samples late in Clean, at 44.1 and 48 kHz.
- [x] **Fade-in at patch load.** Given a new engine in any mode, the first output sample is 0. With the stage off, the output is the signal times a gain that rises in a straight line and reaches 1 at 10 ms.
- [x] **Off is the raw output.** Given Off, the output is bit-identical to today's `clip_output` of the summed outputs, after the fade-in.
- [x] **Huge values do not stick.** (Unit test in `safety.rs`.) Given 1e38, then infinity, then NaN, then a 440 Hz sine at 0.5, the sine comes back at its level within 0.5 s.
- [x] **Mode change.** Switching mode while a signal plays dips the output to 0 within 10 ms, along a straight line down, and it plays again in the new mode. (Engine test, and a unit test that checks no step is larger than one fade step.)
- [x] **No allocation.** `tests/no_alloc.rs` covers processing in every mode, and a mode change.
- [x] **Determinism.** Two runs of the same patch in each mode are bit-identical.
- [x] **Fade-out before stop (rule 14).** Given a Constant of 0.5 into Audio Out in each mode, including Off, when the host asks for a fade-out, then the fade gain falls to 0 in a straight line within 10 ms, the output stays at 0 after that, and the engine reports it is silent. In Low latency and Clean, the output equals the same engine's output without the fade-out times that gain, because the DC blocker shapes the signal too. In Off, the output itself falls in a straight line, with no step larger than one fade step. The same at block sizes 1, 77, and 512. A fade-out that starts during the fade-in, or during a mode-change dip, starts from the gain it has reached. `tests/no_alloc.rs` covers the fade-out.
- [ ] **Manual check.** In the app, with each `--output-safety` mode, Noise at gain 4 into Audio Out plays at a steady level under the ceiling (Low latency, Clean) or clips (Off), and loading a patch does not click. The app has no meter on the final output yet, so this is a listening check.

## Out of scope

- A preferences store and a preferences window for the app. This spec only says the mode is a per-machine preference. The store and window are specified in [App preferences](preferences.md).
- A user-facing master volume.
- Showing limiter activity in the stats bar. That belongs to [#130](https://github.com/brylie/LoomDSP/issues/130), which can count limited samples the same way it counts silenced ones.
- Per-channel modes, for DC-coupled multi-channel interfaces. This waits for outputs beyond stereo.
- True-peak (oversampled) limiting, and a configurable ceiling.
- Exposing the mode in exported plugins, and reporting Clean mode's latency to a host (FR-NATIVE-03).

## Open points

- None. *Settled:* when the device drops out and comes back (FR-APP-04), the stage fades in again, because the app builds a new engine for it and every new engine fades in (rule 8).
