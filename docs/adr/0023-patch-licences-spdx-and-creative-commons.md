# ADR-0023: Patch licences as SPDX expressions, chosen through Creative Commons questions

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-20
- **Related requirements:** FR-FILE-10, FR-NATIVE-09, FR-SHARE-01, FR-SHARE-04
- **Specification:** [Patch metadata](../design/patch-metadata.md)

## Context

LoomDSP is built around sharing patches (UC7, UC9, the P4 phase). A shared patch with no stated licence leaves the people who receive it unsure what they may do: change it, perform with it, sell a track made with it, or put it in a library. FR-NATIVE-09 already says the patch author chooses the licence for their patch. Nothing yet says how a licence is written in the file, or how a musician who is not a lawyer picks one.

Forces:

- **Most authors are musicians,** not lawyers. They need a few plain questions, not a list of hundreds of licences.
- **Tools must read it.** A library browser should filter by licence, and an export should be able to copy it into a plugin. Free text cannot be filtered reliably.
- **Is a patch software?** A patch is a graph that the engine runs, and it is also a creative work: a sound design, like a preset or a sample. Creative Commons advises against its licences for software, because they say nothing about source code or patents. Synth presets, samples, and sound packs are commonly shared under Creative Commons licences.
- **The runtime has its own licence.** LoomDSP is Apache-2.0. An exported plugin contains the runtime and the patch (ADR-0004), each under its own licence.

## Options considered

1. **No licence field.** Authors write a licence in the description if they want. Simple, but tools cannot read it, and most patches would say nothing.
2. **Free text.** A licence field holding anything. Readable by people, but "CC by-sa", "CC-BY-SA 4" and "Attribution-ShareAlike" are the same licence to a person and three to a filter.
3. **An SPDX expression, chosen with Creative Commons questions.** The field holds a standard SPDX expression. The app's chooser asks two plain questions that pick one of the six Creative Commons 4.0 licences, or CC0 for "no rights reserved". Any other SPDX expression (for example `MIT`) can be typed. Readable by tools and people; the chooser fits musicians.
4. **A software licence by default.** Offer MIT, Apache-2.0, or GPL first. Accurate for the code-like side of a patch, but the questions they answer (patents, source code) mean little to most patch authors, and they do not express "no commercial use", which some sound designers want.

## Decision

Option 3. A patch states its licence as an SPDX expression in an optional `license` field. The app offers the Creative Commons 4.0 licences and CC0 through a two-question chooser, accepts any other SPDX expression, and never sets a licence the user did not choose (directly, or as their default).

The example patches shipped with LoomDSP use `CC0-1.0`, so anyone can copy them into their own work, whatever licence that work has.

## Consequences

- The patch format gains an optional `license` field (ADR-0008); a patch without it loads as before. Details are in the [patch metadata spec](../design/patch-metadata.md).
- Licences must credit authors, so the format also needs an authors list (FR-FILE-08).
- The app needs an SPDX parser to show names and check expressions. The `spdx` crate (Apache-2.0) is the candidate, under ADR-0006.
- The app explains each choice in plain words and links to the licence text. It must not present this as legal advice.
- **Export.** A NoDerivatives (ND) licence forbids sharing changed versions. Whether exporting a patch into a plugin counts as a change, and what an export does with NonCommercial (NC) licences, is decided with export (FR-NATIVE-09). The export must at least show the patch licence next to the runtime's Apache-2.0 licence.
- **Embedded assets** (samples, wavetables) may carry their own licences. That is decided with the self-contained patch file (FR-SHARE-01).
- The example patches in `examples/patches/` get `license: "CC0-1.0"` when the field exists.

## Prior art

Patch-sharing sites already offer Creative Commons licences when a patch is uploaded (PatchStorage, for example). Creative Commons and SPDX are both standards, so a LoomDSP patch keeps the same terms, written the same way, when it is shared on such a site. This is one reason to accept Creative Commons for patches, although a patch is partly software.
