# ADR-0004: Export plugins as a prebuilt runtime plus patch data

- **Status:** Deferred
- **Date:** 2026-10-13
- **Related requirements:** FR-NATIVE-01, 02, 05, 08, 09, FR-FILE-06, NFR-SEC-01, FR-WAM-01 (optional)

## Context

Users export patches as CLAP and VST3 plugins and as standalone apps. WAM2 is an optional later target. Most users have no Rust toolchain and should not need one (FR-NATIVE-05). Exported plugins must sound identical to the patch in the editor (NFR-SND-03).

## Options considered

1. **Prebuilt runtime plus patch data.** Ship one compiled engine per target. Export copies it and attaches the patch (and assets) as data. RNBO's web export works in a similar way.
2. **Generate Rust code from the patch and compile it.** Possibly faster DSP, smaller binaries. Needs a toolchain (local or a build server) and a separate code path that can drift from the editor's engine.
3. **Build server.** Option 2 run in the cloud. Adds hosting cost and a service to maintain, against the non-goals.

## Decision

Use option 1 for version 1.

- **CLAP/VST3:** the release process builds one runtime plugin per platform. It can expose both formats from one binary. Export copies it, sets the plugin ID, name, and vendor, and embeds the patch data. The exact embedding method (resource section, appended data, or bundle resources folder) is decided per platform when export is scheduled.
- **Standalone:** the same approach with a prebuilt standalone runtime app that opens straight into the patch's front panel.
- **WAM2 (optional, deferred):** export writes the generic engine `.wasm`, a small generated JS wrapper and descriptor (required by the WAM2 standard), and the patch file into one folder.

## Consequences

- No toolchain needed by users. Export is fast.
- Sound parity is guaranteed, because it is the same engine.
- Every exported plugin contains the whole module library, so binaries are larger. Later we can strip unused modules with feature flags at release time, or offer option 2 as an advanced path.
- Each plugin needs a unique, stable plugin ID derived from user-supplied vendor and name. The export UI must explain this, since changing it breaks DAW projects.
- macOS plugins may need re-signing after embedding data. Document this when export is scheduled.
- Every export includes the Apache-2.0 licence and NOTICE for the runtime (FR-NATIVE-09).
