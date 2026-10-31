# Mono engine cost after stereo

On 2026-11-01, the mono add-chain benchmark ran on the same macOS arm64 machine before and after the stereo work. The baseline was commit `fe663f9`. Both versions sent the same mono signal to both output channels. Each measurement used a 128-frame block, 30 samples, one second of warmup, and three seconds of measurement.

| Add modules | Baseline mean | Stereo implementation mean |
|---:|---:|---:|
| 1 | 2.221 µs | 2.224 µs |
| 10 | 5.388 µs | 5.391 µs |
| 50 | 19.538 µs | 19.496 µs |

These measurements show no material change for these mono chains. They do not measure every patch or device. The baseline's two Audio Out ports were both connected; the new Audio Out duplicates its mono input. Run the benchmark with `cargo bench -p loom-modules --bench engine -- feedback/add/chain`.
