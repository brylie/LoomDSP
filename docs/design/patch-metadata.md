# Patch Metadata Specification

Authors, tags, and a licence stored in every patch, so people who share patches get credit, find patches again, and know what others may do with them.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** [ADR-0023](../adr/0023-patch-licences-spdx-and-creative-commons.md) (licences); the patch format is [ADR-0008](../adr/0008-patch-format-ron.md).
- **Use case and requirements:** UC7, UC9, FR-FILE-08, FR-FILE-09, FR-FILE-10, FR-SHARE-06, FR-SHARE-07, FR-NATIVE-09.
- **Prior art:** package manifests list several authors, one per entry (as `Cargo.toml` does). Topic tags on code hosting sites are lowercase words joined by hyphens. Creative Commons offers a licence chooser that asks two questions. We use those conventions, in our own words and UI.
- **Issues:** [#243](https://github.com/brylie/LoomDSP/issues/243) (decide), [#244](https://github.com/brylie/LoomDSP/issues/244) (umbrella: [#266](https://github.com/brylie/LoomDSP/issues/266) file fields, [#267](https://github.com/brylie/LoomDSP/issues/267) patch info, [#268](https://github.com/brylie/LoomDSP/issues/268) licence chooser), [#245](https://github.com/brylie/LoomDSP/issues/245) (preferences), [#246](https://github.com/brylie/LoomDSP/issues/246) (own patch browser), [#247](https://github.com/brylie/LoomDSP/issues/247) (library filters).
- **Code:** none yet. The patch format is `crates/loom-patch/src/lib.rs` (`Patch`).

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"I made a drone patch with a friend. When I post it, I want both our names on it, so people know who made it. I want to say that anyone may change it and share it, as long as they credit us. And next month I want to find all my drone patches again without opening each file."

Three things answer this: **authors** (who made it), **tags** (what it is, for finding it), and a **licence** (what others may do with it). All three are stored in the patch file, so they travel with it. Two preferences make them cheap to fill in: the user's own author name, and a default licence.

## Terms

- **Author:** a person or group credited for a patch, as one line of text, for example `Ada Lovelace` or `The Drone Club`.
- **Tag:** a short word or phrase that describes a patch, for example `drone`, `fm`, `live-set`.
- **Licence:** what others may do with the patch, written as an [SPDX](https://spdx.org/licenses/) licence expression, for example `CC-BY-SA-4.0`. SPDX is the standard list of short licence names that tools can read.
- **Licence chooser:** a few plain questions whose answers pick a licence.
- **Patch info:** the place in the app where a patch's name, description, authors, tags, and licence are shown and edited.
- **Patch browser:** a view that lists patches and filters them. It covers the user's own patches on this computer (FR-SHARE-06) and the example and community library (FR-SHARE-04).

## Behaviour

### In the patch file

The three fields are optional and are left out of the file when empty, so a patch without them reads as before:

```ron
Patch(
    version: 1,
    name: "Low Drone",
    description: "Two detuned sines through a slow filter.",
    authors: ["Ada Lovelace", "Sam Rivera"],
    tags: ["drone", "ambient", "fm"],
    license: "CC-BY-SA-4.0",
    modules: [ ... ],
    cables: [ ... ],
)
```

The field is spelled `license`, as SPDX spells it, so tools that read SPDX find it. Docs and the UI say "licence".

### Patch info

| Field | Shown as | Edited with |
|---|---|---|
| Authors | One name per line, in credit order | A text box with one author per line. Empty lines are dropped. |
| Tags | Small chips | A text field: type a tag and press Enter or comma to add it. Each chip has a remove button. |
| Licence | The licence's short name and a one-line summary, with a link to the full text. "No licence stated" when empty. | A **Choose** button opens the licence chooser. An **Other** choice takes any SPDX expression. |

### Licence chooser

Two questions pick one of the six Creative Commons 4.0 licences. A third answer gives up all rights:

| Allow others to change it and share the result? | Allow commercial use? | Licence |
|---|---|---|
| Yes | Yes | `CC-BY-4.0` |
| Yes, if they share it under the same licence | Yes | `CC-BY-SA-4.0` |
| No, only unchanged copies | Yes | `CC-BY-ND-4.0` |
| Yes | No | `CC-BY-NC-4.0` |
| Yes, if they share it under the same licence | No | `CC-BY-NC-SA-4.0` |
| No, only unchanged copies | No | `CC-BY-NC-ND-4.0` |
| "I give up all rights: anyone may do anything with it" | | `CC0-1.0` |

Every licence except `CC0-1.0` requires credit to the authors, which is why the authors field matters. Each answer shows a one-sentence explanation in plain words, and the chooser links to the licence's own page. The app explains; it does not give legal advice.

### Defaults (preferences)

Two preferences, in the [preferences](preferences.md) window:

- **Your name:** the author name to add to new patches.
- **Default licence:** chosen with the same chooser, or "No default".

A new patch starts with the user's name as its only author and the default licence, when they are set. The user chose these once, so a new patch never gets a licence the user did not pick.

### Patch browser filters

Both browsers (own patches and library) filter the same way:

- **Author:** shows patches with an author that matches, ignoring case.
- **Tags:** pick one or more tags; a patch must have every picked tag.
- **Licence:** optional. Two filters, **Lets me change it** and **Allows commercial use**, and a third, **No licence stated**. Rule 9 says which patches each one matches.

What each licence allows, for the licence filters:

| Licence | Lets me change it | Allows commercial use |
|---|---|---|
| `CC0-1.0`, `CC-BY-4.0`, `CC-BY-SA-4.0` | Yes | Yes |
| `CC-BY-ND-4.0` | No | Yes |
| `CC-BY-NC-4.0`, `CC-BY-NC-SA-4.0` | Yes | No |
| `CC-BY-NC-ND-4.0` | No | No |
| Any other licence that SPDX marks as OSI-approved or FSF free (for example `MIT`, `Apache-2.0`, `GPL-3.0-or-later`) | Yes | Yes |
| Any other licence | Unknown | Unknown |

Open source and free software licences must allow both, by their own definitions, so the SPDX list answers for them. "Lets me change it" means the licence allows sharing a changed version; conditions such as credit or share-alike still apply and are shown with the licence.

The browsers themselves (layout, where the user's patches are found, the library source) are specified separately. This spec gives only the data and the filter rules they share.

## Rules

1. **Optional and lenient.** A patch opens whether or not it has these fields, and whatever they contain. A licence that is not a valid SPDX expression is kept as written and shown as "Unrecognised licence". *Why:* metadata must never stop someone opening a patch, least of all on stage.
2. **Authors are a list, in credit order.** Each author is one line of free text, trimmed. Duplicates (ignoring case) are dropped. *Why:* several authors must stay distinct, and the order is how people choose to be credited.
3. **Credit is kept when a patch changes hands.** Opening someone else's patch and saving it never removes an author. If the user's name is set and not in the list, Save offers once per patch to add it at the end. *Why:* most licences here require credit to the original authors, and a remix should credit both.
4. **Names only by default.** The app adds only the name from preferences, never an email address or other contact details. A user can type more, knowing the file is shared. *Why:* shared files spread; personal data in them is hard to take back.
5. **Tags are lowercase words joined by hyphens.** Letters (in any script), digits, and single hyphens; no spaces; 1 to 32 characters; at most 20 tags per patch. Typed text is tidied: trimmed, lowercased, spaces and underscores become hyphens, other characters are dropped, and duplicates are removed. *Why:* one spelling per idea makes filters match (`Drone`, `drone ` and `drone` are one tag), and allowing every script keeps tags usable in any language.
6. **A licence is an SPDX expression.** Usually a single identifier from the chooser. **Other** accepts any valid expression, such as `MIT` or `CC-BY-4.0 OR MIT`. *Why:* SPDX names are short, exact, and read by many tools, so a licence written once is understood everywhere.
7. **No licence unless the user chose one.** The app never fills in a licence by itself: only the user's default or the user's choice for this patch. *Why:* a licence is the author's legal decision (FR-NATIVE-09).
8. **Filters match the tidied form.** Author filters ignore case; tag filters compare tidied tags. *Why:* the same tidying on both sides means a search finds what a person expects.
9. **A licence filter matches only what is known to be allowed.** For an expression, `A OR B` allows something if either side does, because the person who receives it may pick either licence. `A AND B` allows it only if both sides do, because both apply. `A WITH exception` counts as `A`. A patch with no licence, or with an unrecognised licence, matches neither **Lets me change it** nor **Allows commercial use**; **No licence stated** matches only patches with no licence. Unknown counts as not allowed. *Why:* a filter that promises reuse must never show a patch whose terms forbid it or say nothing. Showing too few patches is safe; showing too many is not.

## Acceptance criteria

- [ ] A patch with no metadata loads, saves, and produces the same file as before (no new empty fields).
- [ ] A patch with authors, tags, and a licence keeps all three, exactly, through load and save.
- [ ] A patch whose licence is not valid SPDX loads, shows "Unrecognised licence", and saves the text unchanged.
- [ ] Typing ` Bass Drone ` as a tag stores `bass-drone`; adding `drone` twice stores it once; a 21st tag is refused with a message.
- [ ] Each of the seven chooser answers stores the SPDX identifier in the table above.
- [ ] With "Your name" and a default licence set, a new patch starts with that author and licence.
- [ ] Opening a patch by other authors and saving it keeps every author, and offers once to add the user's name at the end.
- [ ] In a browser, filtering by two tags shows only patches with both, and filtering by author ignores case.
- [ ] **Allows commercial use** matches `CC-BY-SA-4.0`, `MIT`, and `MIT OR CC-BY-NC-4.0`, and does not match `CC-BY-NC-4.0`, `MIT AND CC-BY-NC-4.0`, an unrecognised licence, or no licence.
- [ ] **Lets me change it** matches `CC-BY-NC-4.0` and `CC-BY-ND-4.0 OR MIT`, and does not match `CC-BY-ND-4.0`.
- [ ] **No licence stated** matches only patches without a `license` field.

## Out of scope

- **The patch browsers themselves.** Their layout and sources get their own specs (FR-SHARE-04, FR-SHARE-06, and the learning library in #240).
- **Credit for parts.** Modules copied from another patch with the clipboard do not bring their authors along. Revisit with sub-patches (FR-ENCAP), which make parts shareable on their own.
- **Licences of embedded assets** (samples, wavetables, FR-FILE-06). They may need their own licence and credit; decide with the self-contained file (FR-SHARE-01).
- **What an exported plugin shows** of the patch licence (FR-NATIVE-09). Decide with export.
- **Checking licence compatibility** when patches are combined.

## Open points

1. **Phase.** Leaning: the file fields and patch info in P2, with the user's own patch browser (FR-SHARE-06); the library filters in P4 with the library (FR-SHARE-04). The fields are cheap while the format is still at version 1.
2. **Where patch info lives.** Leaning: the inspector shows it when no module is selected, under the patch name. A separate window is the alternative.
3. **Tag characters.** Leaning: letters in any script (as rule 5). Plain ASCII is simpler to type and compare, but would shut out tags in Finnish, Japanese, and other languages.
4. **The "add me as author" offer.** Leaning: once per patch, at the first Save after opening it. The alternative is never asking and adding a menu command instead.
5. **The example patches' own licence.** Decided in ADR-0023: `CC0-1.0`.
6. **Validating SPDX.** Leaning: the `spdx` crate (Apache-2.0, accepted under ADR-0006), which knows the official list and parses expressions.
