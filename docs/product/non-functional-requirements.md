# Non-Functional Requirements

These describe how well the system must work. Numbers marked *(proposal)* should be confirmed after the first benchmarks in Phase 0.

## Real-time safety (`RT`)

The audio thread has a hard deadline. At 48 kHz with a 128-sample buffer, each buffer must finish in under 2.67 ms, every time. Anything with unpredictable duration on that thread can cause a dropout.

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-RT-01 | The audio thread never allocates or frees heap memory, takes a lock, waits on another thread, or makes a system call (including logging and file I/O). | Must | Each of these can take an unbounded amount of time. |
| NFR-RT-02 | Memory for new modules (delay lines, tables) is allocated on a control thread and handed to the audio thread through lock-free queues. | Must | Follows from NFR-RT-01. |
| NFR-RT-03 | Memory from removed modules returns to a control thread for freeing. | Must | Freeing on the audio thread is as risky as allocating. |
| NFR-RT-04 | Control-to-audio and audio-to-control messages use bounded single-producer, single-consumer lock-free queues. | Must | Predictable, wait-free communication. See ADR-0001. |
| NFR-RT-05 | Graph changes are prepared (sorted, flattened, allocated) on a control thread and swapped in atomically at a buffer boundary. | Must | Keeps expensive work off the audio thread. |
| NFR-RT-06 | CI runs the engine under an allocation detector (for example `assert_no_alloc`) in debug builds. Any allocation on the audio path fails the test. | Must | Real-time rules are easy to break by accident. Automatic checks catch it. |
| NFR-RT-07 | If a queue is full, the newest non-critical messages (meters, scopes) are dropped. Structural and parameter messages are never dropped silently. | Must | Visual data can be lost safely. Edits cannot. |
| NFR-RT-08 | Denormal numbers are flushed to zero on the audio thread. | Must | Denormals can make filters and reverbs 10 to 100 times slower as they decay. |
| NFR-RT-09 | A NaN or infinity in a module's output is caught and replaced, and the module is flagged in the UI. | Should | One bad module should not silence or damage the whole output. |

## Performance (`PERF`)

**Reference machine.** The maintainer's laptop: Apple M1-family GPU with the built-in Retina display, measured at full window size. Why a laptop: it is the most likely machine on stage, and its Retina display needs four times the pixels of a 1x monitor at the same window size, so it is the hardest target. Record the exact model in benchmark results.

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-PERF-01 | Reference patch (about 100 modules, 8 voices) at 48 kHz/128 samples uses under 25% of one core on the reference machine *(proposal)*. | Must | Leaves headroom for the host and other plugins. |
| NFR-PERF-02 | Per-module overhead (scheduling, port handling) is under 1% of a simple module's own DSP cost at 128 samples *(proposal)*. | Should | Modular designs fail when overhead dominates small modules. |
| NFR-PERF-03 | Polyphonic voices are processed with SIMD where possible (for example 4 voices per f32x4). | Should | Voices are naturally parallel. This gives large throughput gains on native and WASM SIMD. |
| NFR-PERF-04 | The patch canvas holds 60 fps while panning and zooming, at full window size on the reference laptop, with 100 modules visible and 300 in the patch *(proposal)*. The canvas redraws only when something visible changes: a patch with nothing changing on screen does not redraw, and a display redraws only while it is on screen. | Must | Smooth canvas interaction is part of the product feel. The laptop's Retina display is the hardest target we have measured: rendering cost follows physical pixels (ADR-0009). |
| NFR-PERF-05 | Applying a graph edit (add module or cable) takes under 20 ms from user action to audible result *(proposal)*. | Must | Patching should feel immediate. |
| NFR-PERF-06 | Benchmarks run in CI and report regressions. | Should | Performance is a feature and needs protection. |
| NFR-PERF-07 | Incoming MIDI events are applied in the next audio buffer, with timestamps preserved inside the buffer where the driver provides them. | Must | Controller response must feel immediate and timing must stay tight. |
| NFR-PERF-08 | Benchmark per-sample feedback regions (ADR-0002) in M0 with patches of 1, 10, and 50 modules inside a loop. | Must | Confirms the hybrid execution model before the UI depends on it. |

## Sound quality and determinism (`SND`)

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-SND-01 | Oscillators that produce hard edges are band-limited. Aliasing stays below a documented level. | Must | Aliasing is the most common quality complaint in digital synths. |
| NFR-SND-02 | The same patch, inputs, and settings render the same output on every run on the same platform. | Must | Required for regression tests and reproducible bugs. |
| NFR-SND-03 | Output from the desktop app and exported plugins matches within a documented tolerance. | Must | The same engine runs everywhere. A patch must not change sound after export. |
| NFR-SND-04 | Random sources use a seedable generator. The seed is stored in the patch. | Should | Supports determinism and reproducible patches. |

## Portability (`PORT`)

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-PORT-01 | The engine core compiles to `wasm32-unknown-unknown`. CI checks this on every change. | Should | Cheap to keep from the start, expensive to retrofit. Keeps the optional, deferred web target possible. |
| NFR-PORT-02 | The engine core does not depend on threads, the file system, or a specific audio API. Those live in host adapters. | Must | Same core for the desktop app, plugins, and any future target. |
| NFR-PORT-03 | Desktop targets: macOS (Apple Silicon), Windows 10+, Linux (x86_64, with ALSA/PipeWire/JACK). | Must | Covers the main music production platforms. |
| NFR-PORT-04 | Browser targets: current Chrome, Firefox, and Safari with WebGPU. | Could | Only if the optional web target goes ahead. |
| NFR-PORT-05 | ARM Linux (for example Raspberry Pi) for standalone performance rigs. | Could | Interesting for hardware builders. |

## Web runtime (`WEB`), optional

These apply only if the optional, deferred web target goes ahead.

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-WEB-01 | With cross-origin isolation, control and telemetry use SharedArrayBuffer ring buffers. | Must | Lowest latency and no per-message allocation. |
| NFR-WEB-02 | Without cross-origin isolation, the runtime falls back to `postMessage`. Audio must stay glitch-free; only UI update rate may drop. | Must | Many hosts cannot set COOP/COEP headers. Sound must never depend on hosting setup. |
| NFR-WEB-03 | The WASM engine binary for an exported plugin is under 2 MB compressed *(proposal)*. | Should | Fast loading in web hosts. |
| NFR-WEB-04 | The audio path does not depend on worker threads inside the AudioWorklet. | Must | AudioWorklet scope cannot reliably host a thread pool, and waiting on workers breaks NFR-RT-01. Parallelism is a later, optional optimisation. |

## Reliability (`REL`)

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-REL-01 | The app autosaves and recovers after a crash. | Must | Losing an hour of patching ends trust. |
| NFR-REL-02 | A panic in UI code does not stop audio. A panic in a module silences that module, not the process, where the platform allows. | Should | Live performance needs graceful failure. |
| NFR-REL-03 | One-hour soak test with random graph edits and parameter changes shows zero dropouts and no memory growth. | Must | Catches leaks and rare timing bugs. |
| NFR-REL-04 | The patch loader is fuzz-tested. Malformed files produce an error, never a crash. | Must | Patches will be shared from untrusted sources. |

## Security (`SEC`)

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-SEC-01 | Loading a patch never executes code. Patches contain only data. | Must | Safe sharing. See principle 7 in the vision. |
| NFR-SEC-02 | Resource limits on patches (maximum modules, delay length, voices) prevent a patch from exhausting memory. | Should | A shared patch should not crash a host. |

## Usability and accessibility (`UX`)

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-UX-01 | Every panel widget, on tiles and on front panels, is operable by keyboard, with visible focus. | Should | Accessibility and fast workflows. |
| NFR-UX-02 | Exported panels expose widget names and values to screen readers where the host platform allows. | Could | Blind musicians use DAWs with screen readers. Most plugin UIs ignore them. |
| NFR-UX-03 | Colour is never the only way to show signal type. Ports also have shape or label differences. | Should | Colour vision deficiency is common. |
| NFR-UX-04 | UI text is translatable. English is the source language. | Could | Wider community reach. |
| NFR-UX-05 | Error messages say what happened and what the user can do next, in plain language. | Must | Learners are a key audience. |
| NFR-UX-06 | The desktop app exposes its toolbar, inspector, module browser, and canvas controls to the platform accessibility API, with names, values with their units ("800 Hz", not "800"), and states, so a screen reader and keyboard users can work it. | Should | Blind and low-vision musicians, and people who cannot use a mouse. Waits for AccessKit support in Iced (ADR-0028). |

## Maintainability and openness (`OSS`)

| ID | Requirement | Priority | Rationale |
|---|---|---|---|
| NFR-OSS-01 | Only permissively licensed dependencies (allow list in ADR-0006). No GPL, LGPL, AGPL, SSPL, or similar. CI enforces this with `cargo-deny`. | Must | Users must be able to ship exported plugins without licence surprises. Copyleft terms would pass to every exported plugin. |
| NFR-OSS-02 | Exported plugins carry the Apache-2.0 licence and NOTICE for the runtime and its dependencies. | Must | Meets Apache-2.0 attribution terms. Plugin authors need to know their obligations. |
| NFR-OSS-03 | Public APIs are documented. `cargo doc` builds without warnings. | Should | Module developers depend on it. |
| NFR-OSS-04 | Major decisions are recorded as ADRs in `docs/adr`. | Must | Contributors need to know why things are the way they are. |
| NFR-OSS-05 | Test coverage includes unit tests per module, golden-audio tests per example patch, and integration tests per host target. | Must | Audio bugs are hard to spot by reading code. Tests are also the main safety net for AI-generated code. |
| NFR-OSS-06 | When choosing between comparable dependencies, prefer projects whose contribution policy welcomes AI-assisted work. Record the policy in the dependency review. | Should | We want to contribute fixes upstream. A project that rejects AI-assisted pull requests cannot receive them. |
| NFR-OSS-07 | The repository includes an AI contribution policy that welcomes AI-assisted work and sets quality rules (tests, human review, clear descriptions). | Must | Sets clear expectations for contributors, and for the project's own AI-assisted workflow. |
| NFR-OSS-08 | Agent context files (for example `CLAUDE.md` or `AGENTS.md`) describe the architecture, rules, and commands. They link to these docs. | Should | AI agents work better with stable, explicit context. So do new human contributors. |
