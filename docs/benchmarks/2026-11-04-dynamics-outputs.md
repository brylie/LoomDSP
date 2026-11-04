# Dynamics: taking outputs once per call

On 2026-11-04, the `dynamics/still` benchmark ran on one macOS arm64 machine before and after [#616](https://github.com/brylie/LoomDSP/issues/616). Before the change, `Dynamics::process` looked up its gain output and its stereo output on every sample. After it, the module takes them once above its frame loop with `PortIo::split_outputs`. Each measurement used a 128-frame block, 100 samples, two seconds of warmup, and eight seconds of measurement.

The patch is a sine into Dynamics at its default settings, to the audio output. No parameter is modulated.

| Version | Mean per block |
|---:|---:|
| Before | 6.465 µs |
| After | 6.239 µs |

After the change, a block takes about 3.5% less time. The module now checks once per call that no port is named twice, and no longer searches for its right buffer on every sample. The numbers come from one laptop that was doing other work, so treat them as an estimate. They do not measure other patches or devices.

Run it with `cargo bench -p loom-modules --bench engine -- dynamics`. [#559](https://github.com/brylie/LoomDSP/issues/559) adds the modulated cases, the EQ, and the Crossover.
