# Patch Metadata Specification

Authors, tags, and a licence stored in every patch, so people who share patches get credit, find patches again, and know what others may do with them.

- **Kind:** living specification.
- **Status:** Agreed (2026-11-01, [#243](https://github.com/brylie/LoomDSP/issues/243)). M2: the file fields, patch info, the licence chooser, the defaults, credit for parts, and [your patches browser](#your-patches-browser). M4: the same filters in the library browser (FR-SHARE-04). Implemented in M2, including [credit for parts](#credit-for-parts). See [Notes from building](#notes-from-building).
- **Decision behind it:** [ADR-0023](../adr/0023-patch-licences-spdx-and-creative-commons.md) (licences); the patch format is [ADR-0008](../adr/0008-patch-format-ron.md).
- **Use case and requirements:** UC7, UC9, FR-FILE-08, FR-FILE-09, FR-FILE-10, FR-SHARE-06, FR-SHARE-07, FR-NATIVE-09.
- **Prior art:** package manifests list several authors, one per entry (as `Cargo.toml` does). Topic tags on code hosting sites are lowercase words joined by hyphens. Creative Commons offers a licence chooser that asks two questions. We use those conventions, in our own words and UI.
- **Issues:** [#243](https://github.com/brylie/LoomDSP/issues/243) (decide), [#244](https://github.com/brylie/LoomDSP/issues/244) (umbrella: [#266](https://github.com/brylie/LoomDSP/issues/266) file fields, [#267](https://github.com/brylie/LoomDSP/issues/267) patch info, [#268](https://github.com/brylie/LoomDSP/issues/268) licence chooser), [#245](https://github.com/brylie/LoomDSP/issues/245) (preferences), [#246](https://github.com/brylie/LoomDSP/issues/246) (own patch browser), [#247](https://github.com/brylie/LoomDSP/issues/247) (library filters).
- **Code:** the file fields on `Patch` in `crates/loom-patch/src/lib.rs`; tag and author tidying, licences, the chooser's answers, and the filters in `crates/loom-patch/src/metadata.rs`. In the app: the metadata of the open patch in `document.rs`, patch info in `patch_info.rs`, the chooser in `licence_chooser.rs`, your patches browser in `patches.rs`, the defaults in `preferences.rs` and `settings.rs`, and the credit question in `file_commands.rs` and `files.rs`. Tests: the `tests` modules of those files. SPDX expressions are parsed with the [`spdx`](https://crates.io/crates/spdx) crate (Apache-2.0, accepted under ADR-0006).
- **Background:** [Licences for shared patches](../learn/patch-licences.md) (explainer).

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
    metadata: Metadata(
        name: "Low Drone",
        description: "Two detuned sines through a slow filter.",
        authors: ["Ada Lovelace", "Sam Rivera"],
        tags: ["drone", "ambient", "fm"],
        license: "CC-BY-SA-4.0",
    ),
    modules: [ ... ],
    cables: [ ... ],
)
```

The field is spelled `license`, as SPDX spells it, so tools that read SPDX find it. Docs and the UI say "licence".

### Patch info

Patch info is shown in the inspector when no module is selected, under the patch name. *Why:* the inspector already shows "the thing selected", and with nothing selected that thing is the patch.

| Field | Shown as | Edited with |
|---|---|---|
| Authors | One name per line, in credit order | A text box with one author per line. Empty lines are dropped. |
| Tags | Small chips | A text field: type a tag and press Enter or comma to add it. Leaving the field by any means adds what is typed too, and Escape drops it, as the inspector's value fields commit (Brylie's decision, 2026-11-25; [panel widgets](panel-widgets.md), Typing a value). Each chip has a remove button. |
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

With a screen reader, the chooser is a dialog named by its title. Each question is a radio group whose answers carry their explanations as descriptions; **Other** is a radio button with its text field after it; it sits in a group of its own, which gives no node, so its name is heard once (see Keys below, and [UI layer](ui-layer.md), section 7, rule 29), and the field's text is typed, not used, since Enter there would use it. What the answers give is read as text, and **Use this licence** is disabled until they give a licence. Opened from Preferences, it hides Preferences until it closes ([UI layer](ui-layer.md), section 7, rule 27).

**Keys** (from row 3c of the UI layer plan, the chooser is `loom-ui`'s; Brylie's standing rule of 2026-11-20 is to follow the WAI-ARIA default where there is doubt, here the APG's modal dialog pattern, keyboard interaction):

- **Opening it** focuses the first question's group: its chosen answer, or its first when none is. That is the pattern's first focusable element. A screen reader's Click on **Choose** opens it the same way, so no field under it keeps the keys.
- **Tab and Shift+Tab** move through the chooser and wrap at its ends: the first question, the second when it shows, **Other**, the Other field, Clear, Cancel, and **Use this licence**. **Other** is a radio in a group of its own, since a radio always sits in a group (UI layer, section 6, rule 9); the arrows do not choose it, Space does, and its field is the stop after it. Use is skipped while it is turned off ([UI layer](ui-layer.md), section 6, rule 2). The focus stays inside, as in the question: SC 2.1.2 allows it because Escape always leaves.
- **In a question,** the arrows choose, and Space chooses the focused answer (UI layer, section 6, rule 9). Typing in the Other field chooses **Other**.
- **Enter** in the Other field is Use when Use is on, as above. On a button, Enter is that button's. In a question it does nothing.
- **Escape** closes the chooser, as Cancel does, from anywhere in it, the Other field included. The exception is while an input method is composing in the field: that Escape is the composition's. *Departure from before row 3c:* the field took the first Escape, and a second one closed the chooser.
- **The questions and what they give scroll** in the runtime's scroll container (UI layer, section 5, rule 12), between the title and the buttons, which stay in view. Focus moving to a part reveals it, and the wheel scrolls.
- **Closing it** gives the focus back by UI layer, section 6, rule 4: to the field it opened from, or over Preferences to **Choose**, whatever had the focus when it opened, since a screen reader's Click may not move the focus first (the WAI-ARIA modal dialog pattern returns it to the opener).

The dialog is named by its title and has no description, since nothing stands between the title and the first question.

### Credit for parts

A sub-patch keeps the authors of the patch it came from ([sub-patches](sub-patches.md#library)). Inserting a library item copies its authors into the sub-patch's definition, and a copy of the tile keeps them.

- Patch info shows an **Includes parts by** line: every author of every sub-patch in the patch, at any depth, who is not already in the patch's own authors, in the order first found. It is computed, not stored. *Why:* the patch's author list stays the player's own choice, and part credit appears and disappears with the parts.
- Editing inside a sub-patch does not change its authors. Select a sub-patch tile and edit the Authors box in its inspector to change its credit. The edit marks the patch unsaved. Typing makes one [undo step](undo.md#what-is-undoable) when the box loses focus, the selection changes, or Undo or Save is requested. Changing the stored authors of a linked sub-patch unlinks it so a library update cannot overwrite the edit. The status bar says the sub-patch is now a copy, and undo restores the link with the previous authors.
- Clipboard copies of plain modules still carry no authors.

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

The library browser (M4) is specified with the library (FR-SHARE-04). Your patches browser is below.

### Your patches browser

FR-SHARE-06, [#246](https://github.com/brylie/LoomDSP/issues/246).

- **Where patches are found:** the library folder ([preferences](preferences.md), by default `LoomDSP/Library` in Documents), with its subfolders. It holds the player's patches and saved sub-patches alike. Save and Save As start in the last folder used in a dialog when the patch has no file yet, and otherwise here ([files](files.md)). A patch elsewhere still opens with File > Open, but is not listed. *Why:* one folder to back up and one place to look.
- **Opening:** File > Browse patches, or **Ctrl+Shift+O** (Cmd+Shift+O), opens an overlay over the canvas, like the [module browser](module-browser.md). The shortcut applies when the canvas owns input.
- **What it shows:** a search field, the author, tag, and licence filters above, and a list. Each row shows the name, authors, tags as chips, the licence's short name, and the date changed. Newest first. A file that does not load shows its file name and "Could not read", and cannot be opened from here.
- **Search** matches the name, description, and tags, ignoring case. It combines with the filters (all must match).
- **Keys:** typing goes to the search field. Up and Down move in the list, and Page Up and Page Down by a page of ten rows, as in the [module browser](module-browser.md) (the WAI-ARIA list box pattern's optional keys; Brylie's choice, 2026-11-21). Enter opens the highlighted patch, asking first about unsaved changes as Open does. Escape closes the overlay. *Built in `loom-ui`* (plan 3.2, row 5b, agreed 2026-11-21, built 2026-11-24), it is a modal dialog ([APG Dialog (Modal)](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/)) whose Tab loop is, in reading order: **Close**, the search field, the Author field, each licence check box (Space toggles), then the tags, as one stop: the WAI-ARIA toolbar pattern ([APG Toolbar](https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/)), as the app's toolbar is (Brylie's decision, 2026-11-21). Tab lands on the last tag used, or the first; Left and Right move and stop at the ends; Home and End go to the ends; Space and Enter toggle the focused tag. Each tag keeps its own node, a toggle button with its pressed state, in a toolbar named "Tags". The list is never a Tab stop of its own: Up, Down, Page Up, Page Down, Enter, and Escape reach it from either field before the field edits ([UI layer](ui-layer.md), section 6, rule 6, step 2b), and the highlighted row is each field's active descendant, as in the module browser. The fields' edits reach the browser as field events ([UI layer](ui-layer.md), section 8), so neither field commits. Escape closes it from any other part too, and so does a press on the backdrop. The pointer over a row highlights it (NFR-UX-01).
- **Reading files:** the folder is read on a thread off the UI thread when the overlay opens; rows appear as files are read. Only the metadata fields are kept. *Why:* a folder of hundreds of patches must not freeze the window.
- **With a screen reader:** the overlay is a dialog named "Your patches". The search field has the focus; the list is a list box whose selected option is the highlighted row; the licence filters are check boxes and the tags toggle buttons. A file that does not read is named "... could not read" and opens nothing. As rows arrive, the focus stays where it is and the list's size grows ([UI layer](ui-layer.md), section 7, rule 27).
- Inserting a patch as a sub-patch is done from the module browser ([sub-patches](sub-patches.md#library)), not here. *Why:* this browser opens patches; the module browser adds things to the canvas.

## Rules

1. **Optional and lenient.** A patch opens whether or not it has these fields, and whatever they contain. A licence that is not a valid SPDX expression is kept as written and shown as "Unrecognised licence". *Why:* metadata must never stop someone opening a patch, least of all on stage.
2. **Authors are a list, in credit order.** Each author is one line of free text, trimmed. Duplicates (ignoring case) are dropped. *Why:* several authors must stay distinct, and the order is how people choose to be credited.
3. **Credit is kept when a patch changes hands.** Opening someone else's patch and saving it never removes an author. If the user's name is set and not in the list, the first Save after opening the patch offers once to add it at the end. Declining is remembered for that patch until it is opened again. *Why:* most licences here require credit to the original authors, and a remix should credit both.
4. **Names only by default.** The app adds only the name from preferences, never an email address or other contact details. A user can type more, knowing the file is shared. *Why:* shared files spread; personal data in them is hard to take back.
5. **Tags are lowercase words joined by hyphens.** Letters (in any script), digits, and single hyphens; no spaces; 1 to 32 characters; at most 20 tags per patch. Typed text is tidied: trimmed, lowercased, spaces and underscores become hyphens, other characters are dropped, and duplicates are removed. *Why:* one spelling per idea makes filters match (`Drone`, `drone ` and `drone` are one tag), and allowing every script keeps tags usable in any language.
6. **A licence is an SPDX expression.** Usually a single identifier from the chooser. **Other** accepts any valid expression, such as `MIT` or `CC-BY-4.0 OR MIT`. *Why:* SPDX names are short, exact, and read by many tools, so a licence written once is understood everywhere.
7. **No licence unless the user chose one.** The app never fills in a licence by itself: only the user's default or the user's choice for this patch. *Why:* a licence is the author's legal decision (FR-NATIVE-09).
8. **Filters match the tidied form.** Author filters ignore case; tag filters compare tidied tags. *Why:* the same tidying on both sides means a search finds what a person expects.
9. **A licence filter matches only what is known to be allowed.** For an expression, `A OR B` allows something if either side does, because the person who receives it may pick either licence. `A AND B` allows it only if both sides do, because both apply. `A WITH exception` counts as `A`. A patch with no licence, or with an unrecognised licence, matches neither **Lets me change it** nor **Allows commercial use**; **No licence stated** matches only patches with no licence. Unknown counts as not allowed. *Why:* a filter that promises reuse must never show a patch whose terms forbid it or say nothing. Showing too few patches is safe; showing too many is not.

## Acceptance criteria

- [x] A patch with no metadata loads, saves, and produces the same file as before (no new empty fields).
- [x] A patch with authors, tags, and a licence keeps all three, exactly, through load and save.
- [x] A patch whose licence is not valid SPDX loads, shows "Unrecognised licence", and saves the text unchanged.
- [x] Typing ` Bass Drone ` as a tag stores `bass-drone`; adding `drone` twice stores it once; a 21st tag is refused with a message.
- [x] Each of the seven chooser answers stores the SPDX identifier in the table above.
- [x] With "Your name" and a default licence set, a new patch starts with that author and licence.
- [x] Opening a patch by other authors and saving it keeps every author, and offers once to add the user's name at the end.
- [x] In a browser, filtering by two tags shows only patches with both, and filtering by author ignores case.
- [x] **Allows commercial use** matches `CC-BY-SA-4.0`, `MIT`, and `MIT OR CC-BY-NC-4.0`, and does not match `CC-BY-NC-4.0`, `MIT AND CC-BY-NC-4.0`, an unrecognised licence, or no licence.
- [x] **Lets me change it** matches `CC-BY-NC-4.0` and `CC-BY-ND-4.0 OR MIT`, and does not match `CC-BY-ND-4.0`.
- [x] **No licence stated** matches only patches without a `license` field.

## Out of scope

- **The library browser** (FR-SHARE-04, M4) and the learning library (#240). They use the filters here.
- **Credit for plain modules** copied with the clipboard.
- **Thumbnails or previews** in your patches browser.
- **Licences of embedded assets** (samples, wavetables, FR-FILE-06). They may need their own licence and credit; decide with the self-contained file (FR-SHARE-01).
- **What an exported plugin shows** of the patch licence (FR-NATIVE-09). Decide with export.
- **Checking licence compatibility** when patches are combined.

## Settled points

Settled on 2026-11-01 from the written leanings: the phase split above; patch info in the inspector; tags in any script (rule 5); the "add me as author" offer once per patch at the first Save after opening (rule 3); the example patches under `CC0-1.0` (ADR-0023); SPDX parsing with the `spdx` crate. Credit for parts and your patches browser were added in the same review.

## Notes from building

Points the spec left open, settled while building (M2). Each is small; say so in review if one should change.

- **Root Patch info edits are not undo steps.** Authors, tags, and the licence change at once and mark the patch unsaved, but undo and redo do not cover them. The text fields have no undo of their own, so while one has focus the undo shortcuts do nothing, rather than undo a canvas edit behind it. Added to [undo: what is not undoable](undo.md#what-is-not-undoable).
- **Author filter.** "Matches" means the author contains the typed text, ignoring case, so `ada` finds `Ada Lovelace`.
- **Search** matches when every word typed is in the name, description, or tags, as the module browser does.
- **Tags longer than 32 characters** are cut to 32 by tidying, not refused.
- **The tag filter** offers every tag found in the library folder as a button; pressing one picks or unpicks it.
- **A file that does not read** is listed only while no search or filter is set, since nothing about it can match.
- **The licence link** is shown as the page's address in text. The app cannot open a web page yet.
- **"No licence" and "No default".** The chooser has a button that clears the licence (or the default). It never sets one by itself.
- **Credit question.** It appears at the first Save of each opened or new patch, when "Your name" is set and not among the authors. Its two buttons are **Add my name** and **Not for this patch**; the save goes ahead either way. With no dialog available (Linux without zenity or kdialog) or with the control channel on, nothing is asked or added, and the next Save asks again.
- **The library folder** is the `library` field of the preferences file. It has no control in the Preferences window yet. It is made the first time Save As, the browser, or Open with no folder to start in needs it. For a patch with no file, and for a copy of a bundled example, Save As starts in the last folder used in a dialog, or otherwise here, suggesting a free name from the patch name or the example's file name (`Name.ron`, then `Name 2.ron`). A patch saved outside this folder does not show in Your patches. A recovered copy still starts next to its file ([autosave](autosave.md)).
- **"Your name"** is saved on Enter or when the Preferences window closes, so a New made with the window still open uses the name saved before.
- **Deprecated SPDX identifiers**, such as `GPL-3.0` (now `GPL-3.0-only`), are read strictly: they show as "Unrecognised licence" and **Other** refuses them. Identifiers newer than the `spdx` crate's list do too, until the crate is updated.
- **Browser rows** show tags as text joined by dots, not as chips, to keep rows one line high. The browser opens from a **Browse** toolbar button (the app has no menu bar) and Ctrl+Shift+O.
- **The description** is kept on save but not shown or edited in patch info yet (#512).

## Open points

None.
