# Module Families and Colour

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01. Sources were read on this date.
- **Used by:** [ADR-0029](../adr/0029-module-families-and-a-shared-palette.md), [visual system](../design/visual-system.md), [#459](https://github.com/brylie/LoomDSP/issues/459).

This snapshot records how other tools group modules and colour them, and what we measured when choosing family colours. It is a dated record: write a new one rather than update it.

## How other tools group modules

The [prior-art survey](2026-10-prior-art-survey.md) found about 27 families of module across 15 environments. The groups that tools show users are fewer:

- **VCV Rack.** Library modules carry tags from a fixed list. In our words, the tags cluster into about seven groups: sound sources, processors, effects, modulation, time and sequencing, utility and routing, and interface.
- **Cherry Audio Voltage Modular.** The store groups modules into five: sources, processors, effects, controllers, and utilities.
- **Bitwig Grid.** The module browser lists 16 categories. Cables and ports are coloured by signal (logic, pitch, phase, and untyped signals each have a colour). We found no source saying module headers are coloured by category ([Bitwig user guide, The Grid](https://www.bitwig.com/userguide/latest/welcome_to_the_grid); [Sound on Sound, Bitwig Studio 3](https://www.soundonsound.com/reviews/bitwig-studio-3)).
- **Cubase.** Track colours come from a project palette the user can edit; one forum post reports 128 colours in the largest default set ([Steinberg forum](https://forums.steinberg.net/t/color-palette/788581)). Track types can get default colours in Preferences ([Cubase help](https://archive.steinberg.help/cubase_pro_artist/v9/en/cubase_nuendo/topics/customizing/track_colors_automatically_applying_r.html)).

## How many colours

Guidance for categorical colour puts the practical limit at about seven or eight colours, and recommends a second cue beyond that ([plotstyle palette guide](https://plotstyle.readthedocs.io/en/stable/guides/palettes.html)). The Okabe-Ito set of eight is the common colour-blind-safe starting point ([R thematic, okabe_ito](https://search.r-project.org/CRAN/refmans/thematic/html/okabe_ito.html)). These sources are guides, not studies.

## What we measured

Method: WCAG contrast against the header text `#ECEEF2`, and the CIE76 colour difference (ΔE) between every pair under normal vision and under protanopia, deuteranopia, and tritanopia simulated with the matrices of Machado, Oliveira, and Fernandes (IEEE TVCG 2009, severity 1). About ΔE 10 or more reads as clearly different.

| Palette | Text | Worst ΔE over all four views |
|---|---|---|
| P1 headers (8 colours and one grey for 9 categories) | Light; 4 headers under 4.5:1 | 1.5 (I/O and Time, protanopia) |
| Okabe-Ito, darkened until light text passes | Light | 0.6 (blue and sky blue, tritanopia) |
| Best 5, muted, one dark band | Light | 31.4 |
| Best 8, muted, one dark band, free hues | 6 light, 2 dark | 18.2 |
| Best 8, muted, each near its family's signal hue | Light | 14.2 |
| Best 7, the same with Notes and pitch merged | Light | 14.9 |

Every search left out the hues kept for orange values and pink modulation, and kept each colour at least ΔE 15 from the reserved colours.

Findings:

1. Darkening a standard set breaks it: colours that differ mainly in lightness collapse once all are darkened for the same text.
2. Each family beyond five costs distance. Eight is still workable.
3. Echoing signal colours costs about 4 ΔE against free hues, and keeps every header the same light-text style.
4. Merging two families gains little (14.2 to 14.9).
